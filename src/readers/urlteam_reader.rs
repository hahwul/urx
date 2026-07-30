use super::FileReader;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::{MAX_FILE_BYTES, MAX_FILE_URLS};

/// Reader for URLTeam compressed files (typically gzip format)
pub struct UrlTeamFileReader {
    /// Maximum URLs collected before truncating (see [`MAX_FILE_URLS`]).
    max_urls: usize,
    /// Maximum decompressed bytes read (see [`MAX_FILE_BYTES`]).
    max_bytes: u64,
}

impl UrlTeamFileReader {
    pub fn new() -> Self {
        Self {
            max_urls: MAX_FILE_URLS,
            max_bytes: MAX_FILE_BYTES,
        }
    }

    /// Construct with explicit caps so tests can exercise the truncation paths
    /// without generating gigabytes of input.
    #[cfg(test)]
    fn with_caps(max_urls: usize, max_bytes: u64) -> Self {
        Self {
            max_urls,
            max_bytes,
        }
    }

    /// Read the file's leading magic bytes, or an empty slice when the file is
    /// too short to have any.
    fn magic(file_path: &Path) -> Result<[u8; 3]> {
        let mut file = File::open(file_path)
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

        let mut magic = [0u8; 3];
        // A short read leaves the tail zeroed, which matches no signature.
        let _ = file.read(&mut magic);
        Ok(magic)
    }

    /// Determine if file is gzip compressed based on magic bytes
    fn is_gzip(file_path: &Path) -> Result<bool> {
        let magic = Self::magic(file_path)?;
        Ok(magic[0] == 0x1f && magic[1] == 0x8b)
    }

    /// Whether the file is bzip2-compressed. urx has no bzip2 decoder, and
    /// reading such a file as text yields binary noise with no extractable
    /// URLs — an empty result that reads as "this archive had nothing". Detect
    /// it so the user gets told what actually happened.
    fn is_bzip2(file_path: &Path) -> Result<bool> {
        Ok(&Self::magic(file_path)? == b"BZh")
    }

    /// Read URL lines from `src`, bounded by the shared caps. URLTeam lines can
    /// carry timestamps or status columns alongside the URL, so extraction picks
    /// the first http(s) token rather than taking the whole line.
    fn collect_capped<R: Read>(
        src: R,
        max_urls: usize,
        max_bytes: u64,
    ) -> std::io::Result<(Vec<String>, bool, bool)> {
        super::collect_capped(src, max_urls, max_bytes, |line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            extract_url_from_line(trimmed)
        })
    }
}

impl FileReader for UrlTeamFileReader {
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>> {
        if Self::is_bzip2(file_path)? {
            anyhow::bail!(
                "{}: bzip2 input is not supported. Decompress it first \
                 (`bunzip2 -k <file>`) and pass the result to --files.",
                file_path.display()
            );
        }

        let file = File::open(file_path)
            .with_context(|| format!("Failed to open URLTeam file: {}", file_path.display()))?;

        let (urls, url_capped, byte_capped) = if Self::is_gzip(file_path)? {
            // File is gzip compressed: bound the *decompressed* stream.
            Self::collect_capped(GzDecoder::new(file), self.max_urls, self.max_bytes)
        } else {
            // File is not compressed, read as plain text.
            Self::collect_capped(file, self.max_urls, self.max_bytes)
        }
        .with_context(|| format!("Failed to read URLTeam file: {}", file_path.display()))?;

        super::warn_if_truncated(
            file_path,
            url_capped,
            byte_capped,
            self.max_urls,
            self.max_bytes,
        );

        Ok(urls)
    }
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

        let reader = UrlTeamFileReader::new();
        let urls = reader.read_urls(temp_file.path())?;

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

        let reader = UrlTeamFileReader::new();
        let urls = reader.read_urls(temp_file.path())?;

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

        let reader = UrlTeamFileReader::with_caps(10, MAX_FILE_BYTES);
        let urls = reader.read_urls(temp_file.path())?;
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
        let reader = UrlTeamFileReader::with_caps(MAX_FILE_URLS, 200);
        let urls = reader.read_urls(temp_file.path())?;
        assert!(
            !urls.is_empty() && urls.len() < 1000,
            "byte cap should truncate the stream, got {} URLs",
            urls.len()
        );
        Ok(())
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

        let reader = UrlTeamFileReader::with_caps(MAX_FILE_URLS, 4096);
        let urls = reader.read_urls(temp_file.path())?;
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
        // falsely flagged (the +1 byte allowance guards the exact-size edge).
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "https://example.com/a")?;
        writeln!(temp_file, "https://example.com/b")?;
        temp_file.flush()?;

        let (urls, url_capped, byte_capped) =
            UrlTeamFileReader::collect_capped(File::open(temp_file.path())?, 1000, 1024)?;
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

        let reader = UrlTeamFileReader::new();
        let err = reader.read_urls(temp_file.path()).unwrap_err();
        assert!(err.to_string().contains("bzip2"), "{err}");
        Ok(())
    }

    #[test]
    fn test_is_gzip() -> Result<()> {
        // Test with non-gzip file
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "plain text")?;
        temp_file.flush()?;

        assert!(!UrlTeamFileReader::is_gzip(temp_file.path())?);

        // Test with gzip file
        let gzip_file = NamedTempFile::new()?;
        {
            let mut encoder =
                GzEncoder::new(File::create(gzip_file.path())?, Compression::default());
            writeln!(encoder, "compressed text")?;
            encoder.finish()?;
        }

        assert!(UrlTeamFileReader::is_gzip(gzip_file.path())?);

        Ok(())
    }
}
