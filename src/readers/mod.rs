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

/// Call `f` for each line of `reader`, decoding lossily so binary content
/// (common inside WARC response bodies) doesn't abort the whole read the way
/// `BufRead::lines()` does on invalid UTF-8. Lines longer than
/// `MAX_LINE_BYTES` are truncated and the remainder skipped.
fn for_each_line_lossy<R: BufRead>(mut reader: R, mut f: impl FnMut(&str)) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(8 * 1024);
    loop {
        buf.clear();
        let n = reader
            .by_ref()
            .take(MAX_LINE_BYTES as u64)
            .read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let hit_cap = n == MAX_LINE_BYTES && buf.last() != Some(&b'\n');
        let line = String::from_utf8_lossy(&buf);
        f(line.trim_end_matches(['\n', '\r']));
        if hit_cap {
            skip_to_newline(&mut reader)?;
        }
    }
    Ok(())
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
    // First try to detect based on file extension
    if let Some(extension) = file_path.extension() {
        let ext = extension.to_string_lossy().to_lowercase();

        match ext.as_str() {
            "warc" => return Ok(FileFormat::Warc),
            "gz" | "bz2" => {
                // For compressed files, check if it's likely URLTeam format
                // URLTeam files typically have names containing "urlteam" or similar patterns
                let filename = file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if filename.contains("urlteam") || filename.contains("url_team") {
                    return Ok(FileFormat::UrlTeam);
                }

                // For other .gz/.bz2 files, default to URLTeam format
                return Ok(FileFormat::UrlTeam);
            }
            "txt" | "list" => return Ok(FileFormat::Text),
            _ => {}
        }
    }

    // If extension doesn't help, check filename patterns
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

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

        let path = PathBuf::from("unknown_file");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);
    }

    #[test]
    fn test_for_each_line_lossy_handles_invalid_utf8() {
        // Binary content (e.g. inside a WARC response body) must not abort
        // the read; subsequent valid lines still come through.
        let data = b"https://example.com/a\n\xff\xfe\x00binary\nhttps://example.com/b\n";
        let mut lines = Vec::new();
        for_each_line_lossy(&data[..], |line| lines.push(line.to_string())).unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "https://example.com/a");
        assert_eq!(lines[2], "https://example.com/b");
    }

    #[test]
    fn test_for_each_line_lossy_caps_long_lines() {
        // One enormous "line" is truncated at the cap and skipped to the next
        // newline instead of buffering it all in memory.
        let mut data = vec![b'x'; MAX_LINE_BYTES * 2];
        data.push(b'\n');
        data.extend_from_slice(b"https://example.com/after\n");
        let mut lines = Vec::new();
        for_each_line_lossy(&data[..], |line| lines.push(line.to_string())).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), MAX_LINE_BYTES);
        assert_eq!(lines[1], "https://example.com/after");
    }

    #[test]
    fn test_for_each_line_lossy_no_trailing_newline() {
        let data = b"https://example.com/a\nhttps://example.com/b";
        let mut lines = Vec::new();
        for_each_line_lossy(&data[..], |line| lines.push(line.to_string())).unwrap();
        assert_eq!(
            lines,
            vec!["https://example.com/a", "https://example.com/b"]
        );
    }
}
