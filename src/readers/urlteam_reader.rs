use anyhow::{Context, Result};
use flate2::read::MultiGzDecoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::{StopOnDecodeError, MAX_FILE_BYTES, MAX_FILE_URLS};

/// Read URL lines from `src`, bounded by the given caps. URLTeam lines can
/// carry timestamps or status columns alongside the URL, so extraction picks
/// the first http(s) token rather than taking the whole line.
fn collect_lines<R: Read>(
    src: R,
    max_urls: usize,
    max_bytes: u64,
) -> std::io::Result<(Vec<String>, bool, bool, bool, bool)> {
    super::collect_capped(src, max_urls, max_bytes, |line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        extract_url_from_line(trimmed)
    })
}

/// Read a URLTeam file (typically gzip-compressed), one URL per line.
pub fn read_urls(file_path: &Path) -> Result<Vec<String>> {
    read_urls_capped(file_path, MAX_FILE_URLS, MAX_FILE_BYTES)
}

/// [`read_urls`] with explicit caps, so tests can exercise the truncation
/// paths without generating gigabytes of input.
fn read_urls_capped(file_path: &Path, max_urls: usize, max_bytes: u64) -> Result<Vec<String>> {
    let mut file = File::open(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;
    let magic = super::magic(&mut file)?;

    // urx has no bzip2 decoder, and reading such a file as text yields
    // binary noise with no extractable URLs — an empty result that reads
    // as "this archive had nothing". Tell the user what actually happened.
    if &magic == b"BZh" {
        anyhow::bail!(
            "{}: bzip2 input is not supported. Decompress it first \
                 (`bunzip2 -k <file>`) and pass the result to --files.",
            file_path.display()
        );
    }

    let (urls, url_capped, byte_capped, _, line_capped) = if magic.starts_with(super::GZIP_MAGIC) {
            // File is gzip compressed: bound the *decompressed* stream.
            //
            // MultiGzDecoder, not GzDecoder: gzip members concatenate, and both
            // `.warc.gz` (one member per record) and any `cat a.gz b.gz` archive
            // are made that way. GzDecoder stops after the first member, so urx
            // returned the first record's URLs and silently dropped the rest.
            let mut src = StopOnDecodeError::new(MultiGzDecoder::new(file));
            let collected = collect_lines(&mut src, max_urls, max_bytes);
            let decode_error = src.error;
            collected.map(
                |(mut urls, url_capped, byte_capped, final_fragment_url, line_capped)| {
                if let Some(error) = decode_error {
                    eprintln!(
                        "[urx] {}: gzip stream ended early ({error}); keeping the URLs decoded so far",
                        file_path.display()
                    );
                    // A URL without its terminating newline may be the partial
                    // record at the damaged end of the compressed stream.
                    if final_fragment_url {
                        urls.pop();
                    }
                }
                    (urls, url_capped, byte_capped, false, line_capped)
                },
            )
        } else {
            // File is not compressed, read as plain text.
            collect_lines(file, max_urls, max_bytes)
        }
        .with_context(|| format!("Failed to read URLTeam file: {}", file_path.display()))?;

    super::warn_if_truncated(
        file_path,
        url_capped,
        byte_capped,
        line_capped,
        max_urls,
        max_bytes,
    );

    Ok(urls)
}

/// Extract URL from a line that might contain additional data
fn extract_url_from_line(line: &str) -> Option<String> {
    // Split by whitespace and look for URL-like strings
    for part in line.split_whitespace() {
        if part.starts_with("http://") || part.starts_with("https://") {
            return Some(part.to_string());
        }
    }

    // If no http/https found, check if the whole line looks like a URL
    if line.starts_with("http://") || line.starts_with("https://") {
        Some(line.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_urls_from_uncompressed_file() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "https://example.com/page1")?;
        writeln!(temp_file, "2023-01-01 12:00:00 http://example.org/page2")?;
        writeln!(temp_file, "# Comment")?;
        writeln!(temp_file, "https://example.net/page3 200 OK")?;
        temp_file.flush()?;
        let urls = read_urls(temp_file.path())?;

        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://example.com/page1".to_string()));
        assert!(urls.contains(&"http://example.org/page2".to_string()));
        assert!(urls.contains(&"https://example.net/page3".to_string()));

        Ok(())
    }

    #[test]
    fn test_read_urls_from_gzip_file() -> Result<()> {
        let temp_file = NamedTempFile::new()?;

        // Create gzip compressed content
        {
            let mut encoder =
                GzEncoder::new(File::create(temp_file.path())?, Compression::default());
            writeln!(encoder, "https://example.com/compressed1")?;
            writeln!(encoder, "2023-01-01 http://example.org/compressed2")?;
            encoder.finish()?;
        }
        let urls = read_urls(temp_file.path())?;

        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/compressed1".to_string()));
        assert!(urls.contains(&"http://example.org/compressed2".to_string()));

        Ok(())
    }

    #[test]
    fn test_extract_url_from_line() {
        assert_eq!(
            extract_url_from_line("https://example.com/page1"),
            Some("https://example.com/page1".to_string())
        );

        assert_eq!(
            extract_url_from_line("2023-01-01 12:00:00 https://example.com/page2 200"),
            Some("https://example.com/page2".to_string())
        );

        assert_eq!(extract_url_from_line("some text without url"), None);
    }

    #[test]
    fn test_url_cap_truncates_results() -> Result<()> {
        // Far more URL lines than the cap allows: collection stops at the cap.
        let mut temp_file = NamedTempFile::new()?;
        for i in 0..50 {
            writeln!(temp_file, "https://example.com/page{i}")?;
        }
        temp_file.flush()?;

        let urls = read_urls_capped(temp_file.path(), 10, MAX_FILE_BYTES)?;
        assert_eq!(urls.len(), 10, "URL collection should stop at the cap");
        Ok(())
    }

    #[test]
    fn test_byte_cap_truncates_results() -> Result<()> {
        // A tiny byte cap stops the read partway through, regardless of URL count.
        let mut temp_file = NamedTempFile::new()?;
        for i in 0..1000 {
            writeln!(temp_file, "https://example.com/page{i}")?;
        }
        temp_file.flush()?;

        // ~25 bytes/line; a 200-byte cap admits only the first handful of lines.
        let urls = read_urls_capped(temp_file.path(), MAX_FILE_URLS, 200)?;
        assert!(
            !urls.is_empty() && urls.len() < 1000,
            "byte cap should truncate the stream, got {} URLs",
            urls.len()
        );
        Ok(())
    }

    /// Concatenate independently-gzipped payloads, which is exactly how
    /// `.warc.gz` (one member per record) and `cat a.gz b.gz` are built.
    fn multi_member_gzip(chunks: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in chunks {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(chunk.as_bytes())
                .expect("writing to an in-memory encoder cannot fail");
            out.extend_from_slice(
                &encoder
                    .finish()
                    .expect("finishing an in-memory encoder cannot fail"),
            );
        }
        out
    }

    #[test]
    fn test_reads_every_member_of_a_multi_member_gzip() -> Result<()> {
        // Regression: GzDecoder stops after the first member, so a `.warc.gz`
        // (gzip per record) or any concatenated .gz silently yielded only the
        // first record's URLs — `gunzip -c` showed all of them.
        let temp_file = NamedTempFile::new()?;
        let bytes = multi_member_gzip(&[
            "https://example.com/one\n",
            "https://example.com/two\n",
            "https://example.com/three\n",
        ]);
        std::fs::write(temp_file.path(), bytes)?;
        let urls = read_urls(temp_file.path())?;

        assert_eq!(
            urls,
            vec![
                "https://example.com/one",
                "https://example.com/two",
                "https://example.com/three",
            ]
        );
        Ok(())
    }

    #[test]
    fn test_truncated_gzip_keeps_what_it_decoded() -> Result<()> {
        // Regression: a half-downloaded archive aborted the read with
        // "unexpected end of file" and threw away every URL already decoded.
        let source = NamedTempFile::new()?;
        {
            let mut encoder = GzEncoder::new(File::create(source.path())?, Compression::default());
            for i in 0..5_000 {
                writeln!(encoder, "https://example.com/page{i}")?;
            }
            encoder.finish()?;
        }
        let full = std::fs::read(source.path())?;
        let truncated = NamedTempFile::new()?;
        std::fs::write(truncated.path(), &full[..full.len() / 2])?;
        let recovered = read_urls(truncated.path())?.len();
        assert!(
            recovered > 0 && recovered < 5_000,
            "a truncated archive should yield what decoded, got {recovered}"
        );
        Ok(())
    }

    #[test]
    fn test_truncated_gzip_discards_a_partial_final_url() -> Result<()> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"https://example.com/complete\nhttps://example.com/partial")?;
        let full = encoder.finish()?;
        let truncated = NamedTempFile::new()?;
        // Drop part of the gzip trailer after all URL bytes were produced.
        std::fs::write(truncated.path(), &full[..full.len() - 4])?;

        let urls = read_urls(truncated.path())?;
        assert_eq!(urls, vec!["https://example.com/complete"]);
        Ok(())
    }

    #[test]
    fn test_trailing_garbage_after_the_last_member_is_not_fatal() -> Result<()> {
        // MultiGzDecoder reports junk after the final member as a bad header;
        // that must not lose the members that decoded cleanly.
        let temp_file = NamedTempFile::new()?;
        let mut bytes = multi_member_gzip(&["https://example.com/kept\n"]);
        bytes.extend_from_slice(b"NOT-A-GZIP-HEADER");
        std::fs::write(temp_file.path(), bytes)?;
        assert_eq!(
            read_urls(temp_file.path())?,
            vec!["https://example.com/kept"]
        );
        Ok(())
    }

    #[test]
    fn test_byte_cap_still_bounds_a_multi_member_bomb() -> Result<()> {
        // Reading every member must not weaken the decompression-bomb guard:
        // the cap applies to the decompressed stream as a whole.
        let temp_file = NamedTempFile::new()?;
        let member: String = (0..50_000)
            .map(|i| format!("https://example.com/bomb/{i}\n"))
            .collect();
        let bytes = multi_member_gzip(&[&member, &member, &member]);
        std::fs::write(temp_file.path(), bytes)?;

        let found = read_urls_capped(temp_file.path(), MAX_FILE_URLS, 4096)?.len();
        assert!(
            found > 0 && found < 1_000,
            "the byte cap must bound every member, got {found} URLs"
        );
        Ok(())
    }

    /// A reader that yields `head`, then fails with `kind` on the next call.
    struct FailAfter {
        head: Vec<u8>,
        kind: std::io::ErrorKind,
        interrupts_left: usize,
    }

    impl Read for FailAfter {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.head.is_empty() {
                let n = self.head.len().min(buf.len());
                buf[..n].copy_from_slice(&self.head[..n]);
                self.head.drain(..n);
                return Ok(n);
            }
            if self.interrupts_left > 0 {
                self.interrupts_left -= 1;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "eintr",
                ));
            }
            Err(std::io::Error::new(self.kind, "synthetic decode failure"))
        }
    }

    #[test]
    fn test_stop_on_decode_error_ends_the_read_and_records_why() {
        // The adapter is what keeps a half-decoded archive's URLs: it converts
        // the failure into a clean EOF so the caller's Vec survives, and holds
        // on to the error so the reader can say what happened.
        let mut src = StopOnDecodeError::new(FailAfter {
            head: b"https://example.com/a\nhttps://example.com/b\n".to_vec(),
            kind: std::io::ErrorKind::UnexpectedEof,
            interrupts_left: 0,
        });

        let (urls, _, _, _, _) = collect_lines(&mut src, MAX_FILE_URLS, 1 << 20)
            .expect("a decode failure must not surface as an error");
        assert_eq!(
            urls,
            vec!["https://example.com/a", "https://example.com/b"],
            "everything decoded before the failure must survive"
        );

        let recorded = src.error.as_ref().expect("the failure must be recorded");
        assert_eq!(recorded.kind(), std::io::ErrorKind::UnexpectedEof);

        // Once it has failed, further reads report EOF rather than re-failing.
        assert_eq!(src.read(&mut [0u8; 8]).unwrap(), 0);
    }

    #[test]
    fn test_stop_on_decode_error_passes_interrupted_through() {
        // EINTR is retryable, not a decode failure: swallowing it as EOF would
        // truncate a perfectly good stream.
        let mut src = StopOnDecodeError::new(FailAfter {
            head: b"x".to_vec(),
            kind: std::io::ErrorKind::UnexpectedEof,
            interrupts_left: 1,
        });

        let mut buf = [0u8; 8];
        assert_eq!(src.read(&mut buf).unwrap(), 1);
        assert_eq!(
            src.read(&mut buf).unwrap_err().kind(),
            std::io::ErrorKind::Interrupted
        );
        assert!(
            src.error.is_none(),
            "a retryable error must not end the read"
        );
    }

    #[test]
    fn test_byte_cap_truncates_gzip_decompression_bomb() -> Result<()> {
        // A small .gz that decompresses to a large URL stream — the essence of a
        // decompression bomb. The decompressed-byte cap must bound it.
        let temp_file = NamedTempFile::new()?;
        {
            let mut encoder = GzEncoder::new(File::create(temp_file.path())?, Compression::best());
            for i in 0..100_000 {
                writeln!(encoder, "https://example.com/bomb/{i}")?;
            }
            encoder.finish()?;
        }

        let urls = read_urls_capped(temp_file.path(), MAX_FILE_URLS, 4096)?;
        assert!(
            !urls.is_empty() && urls.len() < 100_000,
            "decompressed-byte cap should truncate the bomb, got {} URLs",
            urls.len()
        );
        Ok(())
    }

    #[test]
    fn test_no_truncation_when_under_caps() -> Result<()> {
        // A small, legitimate file under both caps is read in full and not
        // falsely flagged (the one-byte probe only observes data after the cap).
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "https://example.com/a")?;
        writeln!(temp_file, "https://example.com/b")?;
        temp_file.flush()?;

        let (urls, url_capped, byte_capped, _, _) =
            collect_lines(File::open(temp_file.path())?, 1000, 1024)?;
        assert_eq!(urls.len(), 2);
        assert!(!url_capped);
        assert!(!byte_capped);
        Ok(())
    }

    #[test]
    fn test_bzip2_input_is_reported_not_silently_empty() -> Result<()> {
        // `--files foo.bz2` routes here (detect_file_format maps .bz2 to
        // URLTeam), but there is no bzip2 decoder — so the bytes used to be read
        // as text, yield zero URLs, and look exactly like an empty archive.
        let mut temp_file = NamedTempFile::new()?;
        // A bzip2 header is enough; we reject before decoding anything.
        temp_file.write_all(b"BZh91AY&SY\x00\x00\x00\x00")?;
        temp_file.flush()?;
        let err = read_urls(temp_file.path()).unwrap_err();
        assert!(err.to_string().contains("bzip2"), "{err}");
        Ok(())
    }
}
