use anyhow::Result;
use std::io::{BufRead, Read};
use std::path::Path;

mod text_reader;
mod urlteam_reader;
mod warc_reader;

pub use text_reader::TextFileReader;
pub use urlteam_reader::UrlTeamFileReader;
pub use warc_reader::WarcFileReader;

/// Maximum bytes buffered for a single input line. Real URL lines are far
/// shorter; the cap keeps a corrupt or malicious file (e.g. a gzip bomb that
/// decompresses to one enormous "line") from exhausting memory.
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// UTF-8 byte order mark, stripped from the first line of every input.
const BOM: &[u8] = b"\xef\xbb\xbf";

/// Call `f` for each line of `reader`, decoding lossily so binary content
/// (common inside WARC response bodies) doesn't abort the whole read the way
/// `BufRead::lines()` does on invalid UTF-8. The boolean says whether the line
/// ended with a newline. Overlong lines are skipped whole instead of exposing
/// a truncated prefix to URL extraction.
fn for_each_line_lossy<R: BufRead>(
    mut reader: R,
    mut f: impl FnMut(&str, bool),
) -> std::io::Result<bool> {
    let mut buf = Vec::with_capacity(8 * 1024);
    let mut first_line = true;
    let mut line_capped = false;
    loop {
        buf.clear();
        let n = reader
            .by_ref()
            .take(MAX_LINE_BYTES as u64)
            .read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let terminated = buf.last() == Some(&b'\n');
        let hit_cap = n == MAX_LINE_BYTES && !terminated;
        let mut bytes = &buf[..];
        if first_line {
            first_line = false;
            // A UTF-8 BOM is invisible but glues itself to the first line, so
            // "https://…" no longer starts with its scheme and every reader
            // silently drops that URL. Editors on Windows add one routinely.
            if let Some(rest) = bytes.strip_prefix(BOM) {
                bytes = rest;
            }
        }
        let line = String::from_utf8_lossy(bytes);
        if !hit_cap {
            f(line.trim_end_matches(['\n', '\r']), terminated);
        }
        if hit_cap {
            line_capped = true;
            skip_to_newline(&mut reader)?;
        }
    }
    Ok(line_capped)
}

/// Discard input up to and including the next newline (or EOF).
fn skip_to_newline<R: BufRead>(reader: &mut R) -> std::io::Result<()> {
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(pos) => {
                reader.consume(pos + 1);
                return Ok(());
            }
            None => {
                let len = available.len();
                reader.consume(len);
            }
        }
    }
}

/// Default cap on URLs collected from one input file.
///
/// Every reader needs one: `--files` accepts whatever the user points it at,
/// and a file that is mostly URL lines grows the result `Vec` in step with its
/// size. 1M URLs is far more than any real list.
pub(crate) const MAX_FILE_URLS: usize = 1_000_000;

/// Default cap on bytes consumed from one input file (after decompression).
///
/// The URL cap above bounds the *parsed output*; this bounds the *input*, so a
/// file made of non-URL lines — or a gzip bomb — can't keep the reader running
/// indefinitely. The URL cap fires first for any real URL-dense file.
pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;

/// Read URL lines from `src`, bounding both the number of URLs collected and the
/// number of bytes consumed. `extract` turns one line into a URL, or `None` if
/// the line carries none.
///
/// Returns the URLs, cap flags, and whether an accepted URL came from an
/// unterminated final line. The last flag lets compressed readers discard that
/// one URL if decompression later reports that the stream was damaged.
///
/// The byte bound is enforced with `Read::take`, which caps the stream no matter
/// how a compressed source expands — that is the decompression-bomb guard. When
/// the limit is reached, one additional byte is probed only to distinguish an
/// exact-size input from a truncated one; that byte is never parsed.
pub(crate) fn collect_capped<R: Read>(
    mut src: R,
    max_urls: usize,
    max_bytes: u64,
    mut extract: impl FnMut(&str) -> Option<String>,
) -> std::io::Result<(Vec<String>, bool, bool, bool, bool)> {
    let mut urls = Vec::new();
    let mut url_capped = false;

    let mut collect_line = |line: &str| -> bool {
        if let Some(url) = extract(line) {
            if urls.len() >= max_urls {
                // Stop collecting; the byte bound still drains the rest so we
                // never parse beyond `max_bytes`. The flag is set only once a
                // real URL is dropped — testing before extraction reported
                // "truncated" for a trailing blank line.
                url_capped = true;
                false
            } else {
                urls.push(url);
                true
            }
        } else {
            false
        }
    };

    let (reached_byte_limit, final_fragment, line_capped) = {
        let mut limited = src.by_ref().take(max_bytes);
        let mut final_fragment = None;
        let line_capped =
            for_each_line_lossy(std::io::BufReader::new(&mut limited), |line, terminated| {
                if terminated {
                    let _ = collect_line(line);
                } else {
                    // The last fragment may be a real final line without a newline,
                    // or merely the part of a line cut off at the byte cap. Decide
                    // after probing the source once beyond the cap.
                    final_fragment = Some(line.to_string());
                }
            })?;
        (limited.limit() == 0, final_fragment, line_capped)
    };

    let byte_capped = if reached_byte_limit {
        let mut probe = [0u8; 1];
        src.read(&mut probe)? != 0
    } else {
        false
    };
    let mut final_fragment_url = false;
    if !byte_capped {
        if let Some(line) = final_fragment {
            final_fragment_url = collect_line(&line);
        }
    }

    Ok((
        urls,
        url_capped,
        byte_capped,
        final_fragment_url,
        line_capped,
    ))
}

/// Turn a decompression error into EOF while retaining the error for a warning.
/// This keeps complete records decoded before a damaged gzip trailer.
pub(crate) struct StopOnDecodeError<R> {
    inner: R,
    error: Option<std::io::Error>,
}

impl<R: Read> StopOnDecodeError<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self { inner, error: None }
    }
}

impl<R: Read> Read for StopOnDecodeError<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.error.is_some() {
            return Ok(0);
        }
        match self.inner.read(buf) {
            Ok(n) => Ok(n),
            // Interrupted is retryable and not a decode failure.
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Err(e),
            Err(e) => {
                self.error = Some(e);
                Ok(0)
            }
        }
    }
}

/// Tell the user on stderr when a read stopped early. Truncation is rare and
/// means the output is incomplete, so it must not pass in silence.
pub(crate) fn warn_if_truncated(
    file_path: &Path,
    url_capped: bool,
    byte_capped: bool,
    line_capped: bool,
    max_urls: usize,
    max_bytes: u64,
) {
    if url_capped {
        eprintln!(
            "[urx] {}: stopped at the {}-URL cap; results truncated",
            file_path.display(),
            max_urls
        );
    }
    if byte_capped {
        eprintln!(
            "[urx] {}: stopped after {} bytes read (possible decompression bomb); results truncated",
            file_path.display(),
            max_bytes
        );
    }
    if line_capped {
        eprintln!(
            "[urx] {}: skipped one or more lines longer than {} bytes; results may be incomplete",
            file_path.display(),
            MAX_LINE_BYTES
        );
    }
}

/// Trait for reading URLs from different file formats
pub trait FileReader {
    /// Read URLs from a file and return them as a vector of strings
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>>;
}

/// Enum representing different file formats
#[derive(Debug, Clone, PartialEq)]
pub enum FileFormat {
    Warc,
    UrlTeam,
    Text,
}

/// Auto-detect file format based on file extension and content
pub fn detect_file_format(file_path: &Path) -> Result<FileFormat> {
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Explicit format extensions take precedence over filename clues.
    if let Some(extension) = file_path.extension() {
        let ext = extension.to_string_lossy().to_lowercase();

        match ext.as_str() {
            "warc" => return Ok(FileFormat::Warc),
            "gz" | "bz2" => {
                // Compression extensions do not identify the archive format.
                // Use the WARC filename clue before the URLTeam default.
                if filename.contains("warc") {
                    return Ok(FileFormat::Warc);
                }

                // Compressed files without a WARC clue default to URLTeam.
                return Ok(FileFormat::UrlTeam);
            }
            "txt" | "list" => return Ok(FileFormat::Text),
            _ => {}
        }
    }

    // For unknown or missing extensions, fall back to filename patterns.
    if filename.contains("warc") {
        return Ok(FileFormat::Warc);
    }

    if filename.contains("urlteam") || filename.contains("url_team") {
        return Ok(FileFormat::UrlTeam);
    }

    // Default to text format for unknown files
    Ok(FileFormat::Text)
}

/// Read URLs from a file using auto-detected format
pub fn read_urls_from_file(file_path: &Path) -> Result<Vec<String>> {
    let format = detect_file_format(file_path)?;

    match format {
        FileFormat::Warc => {
            let reader = WarcFileReader::new();
            reader.read_urls(file_path)
        }
        FileFormat::UrlTeam => {
            let reader = UrlTeamFileReader::new();
            reader.read_urls(file_path)
        }
        FileFormat::Text => {
            let reader = TextFileReader::new();
            reader.read_urls(file_path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_warc_format() {
        let path = PathBuf::from("test.warc");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);

        let path = PathBuf::from("archive.warc");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);

        let path = PathBuf::from("some_warc_file.dat");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);

        let path = PathBuf::from("some_warc_file.gz");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);

        let path = PathBuf::from("foo.warc.gz");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);

        let path = PathBuf::from("crawl-warc.dat");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);
    }

    #[test]
    fn test_detect_urlteam_format() {
        let path = PathBuf::from("urlteam_data.gz");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::UrlTeam);

        let path = PathBuf::from("url_team_archive.bz2");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::UrlTeam);

        let path = PathBuf::from("data.gz");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::UrlTeam);
    }

    #[test]
    fn test_detect_text_format() {
        let path = PathBuf::from("urls.txt");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);

        let path = PathBuf::from("list.list");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);

        let path = PathBuf::from("warc-targets.txt");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);

        let path = PathBuf::from("my_warc_urls.list");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);

        let path = PathBuf::from("unknown_file");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);
    }

    #[test]
    fn test_for_each_line_lossy_handles_invalid_utf8() {
        // Binary content (e.g. inside a WARC response body) must not abort
        // the read; subsequent valid lines still come through.
        let data = b"https://example.com/a\n\xff\xfe\x00binary\nhttps://example.com/b\n";
        let mut lines = Vec::new();
        for_each_line_lossy(&data[..], |line, _| lines.push(line.to_string())).unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "https://example.com/a");
        assert_eq!(lines[2], "https://example.com/b");
    }

    #[test]
    fn test_for_each_line_lossy_skips_overlong_lines() {
        // One enormous "line" is skipped whole instead of exposing a truncated
        // prefix to the URL extractor or buffering the rest in memory.
        let mut data = vec![b'x'; MAX_LINE_BYTES * 2];
        data.push(b'\n');
        data.extend_from_slice(b"https://example.com/after\n");
        let mut lines = Vec::new();
        let line_capped =
            for_each_line_lossy(&data[..], |line, _| lines.push(line.to_string())).unwrap();
        assert_eq!(lines, vec!["https://example.com/after"]);
        assert!(
            line_capped,
            "the skipped line should be reported to the caller"
        );
    }

    #[test]
    fn test_for_each_line_lossy_strips_a_leading_utf8_bom() {
        // Regression: a BOM glued itself to the first line, so
        // "\u{feff}https://example.com/a" did not start with "http" and every
        // reader dropped that URL without a word.
        let data = b"\xef\xbb\xbfhttps://example.com/a\nhttps://example.com/b\n";
        let mut lines = Vec::new();
        for_each_line_lossy(&data[..], |line, _| lines.push(line.to_string())).unwrap();
        assert_eq!(
            lines,
            vec!["https://example.com/a", "https://example.com/b"]
        );
    }

    #[test]
    fn test_for_each_line_lossy_only_strips_the_bom_at_the_start() {
        // A BOM-looking sequence later in the file is data, not a marker.
        let data = "a\n\u{feff}b\n".as_bytes();
        let mut lines = Vec::new();
        for_each_line_lossy(data, |line, _| lines.push(line.to_string())).unwrap();
        assert_eq!(lines, vec!["a", "\u{feff}b"]);
    }

    #[test]
    fn test_url_cap_flag_reflects_a_dropped_url_not_a_trailing_line() {
        // Regression: the cap was tested before extraction, so a file holding
        // exactly `max_urls` URLs followed by a blank or comment line reported
        // "results truncated" while nothing had been dropped.
        let data = b"https://example.com/a\nhttps://example.com/b\n\n# done\n";
        let (urls, url_capped, _, _, _) = collect_capped(&data[..], 2, MAX_FILE_BYTES, |line| {
            let t = line.trim();
            (t.starts_with("http://") || t.starts_with("https://")).then(|| t.to_string())
        })
        .unwrap();
        assert_eq!(urls.len(), 2);
        assert!(!url_capped, "nothing was dropped, so nothing was truncated");

        // A third URL past the cap is a genuine truncation.
        let data = b"https://example.com/a\nhttps://example.com/b\nhttps://example.com/c\n";
        let (urls, url_capped, _, _, _) = collect_capped(&data[..], 2, MAX_FILE_BYTES, |line| {
            let t = line.trim();
            (t.starts_with("http://") || t.starts_with("https://")).then(|| t.to_string())
        })
        .unwrap();
        assert_eq!(urls.len(), 2);
        assert!(url_capped);
    }

    #[test]
    fn test_byte_cap_drops_an_unterminated_partial_url() {
        let complete = b"https://example.com/complete\n";
        let partial = b"https://example.net/partial\n";
        let mut data = complete.to_vec();
        data.extend_from_slice(partial);
        let max_bytes = (complete.len() + 12) as u64;

        let (urls, _, byte_capped, _, _) = collect_capped(&data[..], 10, max_bytes, |line| {
            let trimmed = line.trim();
            (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                .then(|| trimmed.to_string())
        })
        .unwrap();

        assert!(byte_capped);
        assert_eq!(urls, vec!["https://example.com/complete"]);
    }

    #[test]
    fn test_exact_byte_cap_keeps_an_unterminated_final_url() {
        let data = b"https://example.com/final";
        let (urls, url_capped, byte_capped, final_fragment_url, _) =
            collect_capped(&data[..], 10, data.len() as u64, |line| {
                let trimmed = line.trim();
                (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                    .then(|| trimmed.to_string())
            })
            .unwrap();

        assert_eq!(urls, vec!["https://example.com/final"]);
        assert!(!url_capped);
        assert!(!byte_capped);
        assert!(final_fragment_url);
    }

    #[test]
    fn test_compressed_warc_uses_warc_extraction_rules() -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let first_record = concat!(
            "WARC/1.0\n",
            "WARC-Type: response\n",
            "WARC-Target-URI: https://example.com/archive\n",
            "Content-Length: 80\n\n",
            "body text mentioning https://example.net/inline\n",
        );
        let second_record = concat!(
            "WARC/1.0\n",
            "WARC-Type: response\n",
            "WARC-Target-URI: https://example.org/second\n\n",
        );
        let warc = format!("{first_record}{second_record}");
        let plain = tempfile::Builder::new().suffix(".warc").tempfile()?;
        std::fs::write(plain.path(), &warc)?;
        let compressed = tempfile::Builder::new().suffix(".warc.gz").tempfile()?;
        let mut compressed_members = Vec::new();
        for record in [first_record, second_record] {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(record.as_bytes())?;
            compressed_members.extend_from_slice(&encoder.finish()?);
        }
        std::fs::write(compressed.path(), compressed_members)?;

        let plain_urls = read_urls_from_file(plain.path())?;
        let compressed_urls = read_urls_from_file(compressed.path())?;
        assert_eq!(compressed_urls, plain_urls);
        assert_eq!(
            plain_urls,
            vec!["https://example.com/archive", "https://example.org/second"]
        );
        Ok(())
    }

    #[test]
    fn test_compressed_warc_bzip2_is_reported_as_unsupported() -> Result<()> {
        let compressed = tempfile::Builder::new().suffix(".warc.bz2").tempfile()?;
        std::fs::write(compressed.path(), b"BZh91AY&SY\0\0\0\0")?;

        assert_eq!(detect_file_format(compressed.path())?, FileFormat::Warc);
        let error = read_urls_from_file(compressed.path()).unwrap_err();
        assert!(error
            .to_string()
            .contains("bzip2 WARC input is not supported"));
        Ok(())
    }

    #[test]
    fn test_for_each_line_lossy_no_trailing_newline() {
        let data = b"https://example.com/a\nhttps://example.com/b";
        let mut lines = Vec::new();
        for_each_line_lossy(&data[..], |line, _| lines.push(line.to_string())).unwrap();
        assert_eq!(
            lines,
            vec!["https://example.com/a", "https://example.com/b"]
        );
    }
}
