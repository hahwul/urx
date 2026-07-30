use super::FileReader;
use anyhow::{Context, Result};
use std::path::Path;

/// Reader for WARC (Web ARChive) files
/// Note: This is a basic implementation that extracts URLs from WARC headers
pub struct WarcFileReader;

impl WarcFileReader {
    pub fn new() -> Self {
        Self
    }
}

/// Pull a URL out of one WARC line, if it carries one.
///
/// A WARC mixes headers with raw response bodies, so both the
/// `WARC-Target-URI:` header and bare URLs in the payload are collected. Header
/// names are case-insensitive per the WARC spec.
fn extract_url_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();

    let after_header = trimmed
        .split_once(':')
        .filter(|(name, _)| name.eq_ignore_ascii_case("WARC-Target-URI"))
        .map(|(_, value)| value.trim());

    let candidate = match after_header {
        Some(value) => value,
        None => trimmed,
    };

    if (candidate.starts_with("http://") || candidate.starts_with("https://"))
        && !candidate.contains(' ')
    {
        return Some(candidate.to_string());
    }
    None
}

impl FileReader for WarcFileReader {
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>> {
        use std::fs::File;

        let file = File::open(file_path)
            .with_context(|| format!("Failed to open WARC file: {}", file_path.display()))?;

        // WARCs are routinely gigabytes, and this reader used to collect every
        // matching line with no bound at all — unlike the URLTeam and sitemap
        // readers, which have capped their input from the start. Lines are read
        // lossily because a WARC embeds raw response bodies: binary content must
        // not abort the read.
        let (urls, url_capped, byte_capped) = super::collect_capped(
            file,
            super::MAX_FILE_URLS,
            super::MAX_FILE_BYTES,
            extract_url_from_line,
        )
        .with_context(|| format!("Failed to read WARC file: {}", file_path.display()))?;

        super::warn_if_truncated(
            file_path,
            url_capped,
            byte_capped,
            super::MAX_FILE_URLS,
            super::MAX_FILE_BYTES,
        );

        Ok(urls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_warc_file_reader_creation() {
        let reader = WarcFileReader::new();
        // Just test that we can create the reader without issues
        assert_eq!(std::mem::size_of_val(&reader), 0); // Zero-sized type
    }

    #[test]
    fn test_read_warc_headers() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "WARC/1.0")?;
        writeln!(temp_file, "WARC-Type: response")?;
        writeln!(temp_file, "WARC-Target-URI: https://example.com/page1")?;
        writeln!(temp_file, "Content-Length: 100")?;
        writeln!(temp_file)?;
        writeln!(temp_file, "HTTP response content here")?;
        writeln!(temp_file, "WARC-Target-URI: http://example.org/page2")?;
        temp_file.flush()?;

        let reader = WarcFileReader::new();
        let urls = reader.read_urls(temp_file.path())?;

        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/page1".to_string()));
        assert!(urls.contains(&"http://example.org/page2".to_string()));

        Ok(())
    }

    #[test]
    fn test_url_collection_is_bounded() -> Result<()> {
        // Regression: this reader had no cap of any kind, while the URLTeam and
        // sitemap readers have bounded their input from the start. WARCs are
        // routinely gigabytes, so an unbounded collect grows with the file.
        let mut temp_file = NamedTempFile::new()?;
        for i in 0..500 {
            writeln!(temp_file, "WARC-Target-URI: https://example.com/{i}")?;
        }
        temp_file.flush()?;

        let (urls, url_capped, _) = super::super::collect_capped(
            File::open(temp_file.path())?,
            10,
            super::super::MAX_FILE_BYTES,
            extract_url_from_line,
        )?;

        assert_eq!(urls.len(), 10, "collection must stop at the URL cap");
        assert!(url_capped, "the cap being hit must be reported");
        Ok(())
    }

    #[test]
    fn test_byte_cap_bounds_the_read() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        for i in 0..2000 {
            writeln!(temp_file, "WARC-Target-URI: https://example.com/{i}")?;
        }
        temp_file.flush()?;

        let (urls, _, byte_capped) = super::super::collect_capped(
            File::open(temp_file.path())?,
            super::super::MAX_FILE_URLS,
            200,
            extract_url_from_line,
        )?;

        assert!(byte_capped, "the byte cap must be reported");
        assert!(
            urls.len() < 2000,
            "read should stop early, got {}",
            urls.len()
        );
        Ok(())
    }

    #[test]
    fn test_target_uri_header_is_case_insensitive() {
        // WARC field names are case-insensitive per the spec; only the exact
        // "WARC-Target-URI:" spelling used to be recognised.
        assert_eq!(
            extract_url_from_line("warc-target-uri: https://example.com/a").as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(
            extract_url_from_line("WARC-Target-URI: https://example.com/b").as_deref(),
            Some("https://example.com/b")
        );
        // A non-URL value is still rejected.
        assert!(extract_url_from_line("WARC-Type: response").is_none());
    }

    #[test]
    fn test_read_warc_content_urls() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "WARC/1.0")?;
        writeln!(temp_file, "WARC-Type: response")?;
        writeln!(temp_file, "WARC-Target-URI: https://example.com/header")?;
        writeln!(temp_file, "Content-Length: 100")?;
        writeln!(temp_file)?;
        writeln!(temp_file, "Some text content here")?;
        writeln!(temp_file, "http://example.org/content1")?;
        writeln!(temp_file, "  https://example.net/content2  ")?;
        writeln!(temp_file, "http://invalid-url-with space")?;
        temp_file.flush()?;

        let reader = WarcFileReader::new();
        let urls = reader.read_urls(temp_file.path())?;

        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://example.com/header".to_string()));
        assert!(urls.contains(&"http://example.org/content1".to_string()));
        assert!(urls.contains(&"https://example.net/content2".to_string()));
        assert!(!urls.contains(&"http://invalid-url-with space".to_string()));

        Ok(())
    }
}
