use super::FileReader;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Overall cap on URLs collected from one URLTeam file, mirroring
/// [`MAX_SITEMAP_URLS`]. A small gzip can decompress to a vast stream of short,
/// valid URL lines; without a cap that grows the result `Vec` without bound.
///
/// [`MAX_SITEMAP_URLS`]: crate::providers
const MAX_URLTEAM_URLS: usize = 1_000_000;

/// Hard cap on bytes read from the *decompressed* stream. The per-URL cap above
/// only bounds the parsed output; a decompression bomb made of non-URL lines
/// (or one giant line) would otherwise keep the decoder running indefinitely.
/// The URL cap fires first for any real URL-dense file (1M URLs ≈ 50 MB), so
/// this only ever bites pathological input. 1 GiB is a comfortable ceiling.
const MAX_URLTEAM_DECOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;

/// Reader for URLTeam compressed files (typically gzip format)
pub struct UrlTeamFileReader {
    /// Maximum URLs collected before truncating (see [`MAX_URLTEAM_URLS`]).
    max_urls: usize,
    /// Maximum decompressed bytes read (see [`MAX_URLTEAM_DECOMPRESSED_BYTES`]).
    max_bytes: u64,
}

impl UrlTeamFileReader {
    pub fn new() -> Self {
        Self {
            max_urls: MAX_URLTEAM_URLS,
            max_bytes: MAX_URLTEAM_DECOMPRESSED_BYTES,
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

    /// Determine if file is gzip compressed based on magic bytes
    fn is_gzip(file_path: &Path) -> Result<bool> {
        let mut file = File::open(file_path)
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

        let mut magic = [0u8; 2];
        match file.read_exact(&mut magic) {
            Ok(()) => Ok(magic[0] == 0x1f && magic[1] == 0x8b),
            Err(_) => Ok(false), // File too small or other read error
        }
    }

    /// Read URL lines from `src`, bounding both the number of URLs collected and
    /// the number of (decompressed) bytes consumed. Returns the URLs plus flags
    /// indicating whether the URL cap or the byte cap was hit, so the caller can
    /// warn that results were truncated.
    ///
    /// The byte bound is enforced with `Read::take`, which caps the stream
    /// regardless of how a malicious gzip expands — that is the decompression-
    /// bomb guard. We allow one byte past the cap so a file that is *exactly*
    /// `max_bytes` long isn't falsely flagged as truncated.
    fn collect_capped<R: Read>(
        src: R,
        max_urls: usize,
        max_bytes: u64,
    ) -> std::io::Result<(Vec<String>, bool, bool)> {
        let mut limited = src.take(max_bytes.saturating_add(1));
        let mut urls = Vec::new();
        let mut url_capped = false;

        super::for_each_line_lossy(BufReader::new(&mut limited), |line| {
            if urls.len() >= max_urls {
                // Stop collecting; the `take` bound still drains the rest so we
                // never read more than `max_bytes (+1)` total.
                url_capped = true;
                return;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // URLTeam files often contain URLs in various formats
                // Try to extract URL from the line (may have timestamps or other data)
                if let Some(url) = extract_url_from_line(trimmed) {
                    urls.push(url);
                }
            }
        })?;

        // `limit()` is the unused remainder of the (max_bytes + 1) allowance; a
        // remainder of 0 means the source ran past the cap and was truncated.
        let byte_capped = limited.limit() == 0;
        Ok((urls, url_capped, byte_capped))
    }
}

impl FileReader for UrlTeamFileReader {
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>> {
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

        // Truncation is rare and means the output is incomplete, so surface it
        // on stderr rather than silently returning a partial list.
        if url_capped {
            eprintln!(
                "[urx] {}: stopped at the {}-URL cap; results truncated",
                file_path.display(),
                self.max_urls
            );
        } else if byte_capped {
            eprintln!(
                "[urx] {}: stopped after {} decompressed bytes (possible decompression bomb); results truncated",
                file_path.display(),
                self.max_bytes
            );
        }

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

        let reader = UrlTeamFileReader::with_caps(10, MAX_URLTEAM_DECOMPRESSED_BYTES);
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
        let reader = UrlTeamFileReader::with_caps(MAX_URLTEAM_URLS, 200);
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

        let reader = UrlTeamFileReader::with_caps(MAX_URLTEAM_URLS, 4096);
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
