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

impl FileReader for WarcFileReader {
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        let file = File::open(file_path)
            .with_context(|| format!("Failed to open WARC file: {}", file_path.display()))?;

        let reader = BufReader::new(file);
        let mut urls = Vec::new();

        for line in reader.lines() {
            let line = line?;
            // Look for WARC-Target-URI headers
            if line.starts_with("WARC-Target-URI:") {
                if let Some(url) = line.strip_prefix("WARC-Target-URI:") {
                    let url = url.trim();
                    if url.starts_with("http://") || url.starts_with("https://") {
                        urls.push(url.to_string());
                    }
                }
            }
            // Also look for plain URLs in the content
            else if line.trim().starts_with("http://") || line.trim().starts_with("https://") {
                let url = line.trim();
                // Basic URL validation - check if it looks like a complete URL
                if url.contains("://") && !url.contains(' ') {
                    urls.push(url.to_string());
                }
            }
        }

        Ok(urls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
