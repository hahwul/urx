use super::FileReader;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// Reader for URLTeam compressed files (typically gzip format)
pub struct UrlTeamFileReader;

impl UrlTeamFileReader {
    pub fn new() -> Self {
        Self
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
}

impl FileReader for UrlTeamFileReader {
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>> {
        let file = File::open(file_path)
            .with_context(|| format!("Failed to open URLTeam file: {}", file_path.display()))?;

        let reader: Box<dyn BufRead> = if Self::is_gzip(file_path)? {
            // File is gzip compressed
            let decoder = GzDecoder::new(file);
            Box::new(BufReader::new(decoder))
        } else {
            // File is not compressed, read as plain text
            Box::new(BufReader::new(file))
        };

        let mut urls = Vec::new();
        for (line_num, line) in reader.lines().enumerate() {
            let line = line.with_context(|| {
                format!("Failed to read line {} from URLTeam file: {}", line_num + 1, file_path.display())
            })?;
            
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // URLTeam files often contain URLs in various formats
                // Try to extract URL from the line (may have timestamps or other data)
                if let Some(url) = extract_url_from_line(trimmed) {
                    urls.push(url);
                }
            }
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
            let mut encoder = GzEncoder::new(File::create(temp_file.path())?, Compression::default());
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
        
        assert_eq!(
            extract_url_from_line("some text without url"),
            None
        );
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
            let mut encoder = GzEncoder::new(File::create(gzip_file.path())?, Compression::default());
            writeln!(encoder, "compressed text")?;
            encoder.finish()?;
        }
        
        assert!(UrlTeamFileReader::is_gzip(gzip_file.path())?);

        Ok(())
    }
}