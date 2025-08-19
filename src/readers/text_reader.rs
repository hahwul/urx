use super::FileReader;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Reader for plain text files containing URLs (one per line)
pub struct TextFileReader;

impl TextFileReader {
    pub fn new() -> Self {
        Self
    }
}

impl FileReader for TextFileReader {
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>> {
        let file = File::open(file_path)
            .with_context(|| format!("Failed to open text file: {}", file_path.display()))?;

        let reader = BufReader::new(file);
        let mut urls = Vec::new();

        for (line_num, line) in reader.lines().enumerate() {
            let line = line.with_context(|| {
                format!(
                    "Failed to read line {} from file: {}",
                    line_num + 1,
                    file_path.display()
                )
            })?;

            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // Basic URL validation - must start with http or https
                if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                    urls.push(trimmed.to_string());
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
    fn test_read_urls_from_text_file() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "https://example.com/page1")?;
        writeln!(temp_file, "http://example.org/page2")?;
        writeln!(temp_file, "# This is a comment")?;
        writeln!(temp_file)?; // Empty line
        writeln!(temp_file, "https://example.net/page3")?;
        writeln!(temp_file, "not-a-url")?; // Invalid URL
        temp_file.flush()?;

        let reader = TextFileReader::new();
        let urls = reader.read_urls(temp_file.path())?;

        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://example.com/page1".to_string()));
        assert!(urls.contains(&"http://example.org/page2".to_string()));
        assert!(urls.contains(&"https://example.net/page3".to_string()));

        Ok(())
    }

    #[test]
    fn test_read_urls_from_empty_file() -> Result<()> {
        let temp_file = NamedTempFile::new()?;

        let reader = TextFileReader::new();
        let urls = reader.read_urls(temp_file.path())?;

        assert!(urls.is_empty());

        Ok(())
    }
}
