use crate::output::Formatter;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

// Outputter implementations for different formats
use super::Outputter;

#[derive(Debug, Clone)]
pub struct PlainOutputter {
    formatter: Box<dyn Formatter>,
}

impl PlainOutputter {
    pub fn new() -> Self {
        PlainOutputter {
            formatter: Box::new(super::PlainFormatter::new()),
        }
    }
}

impl Outputter for PlainOutputter {
    fn format(&self, url: &str, is_last: bool) -> String {
        self.formatter.format(url, is_last)
    }

    fn output(&self, urls: &[String], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                let mut file = File::create(&path).context("Failed to create output file")?;

                for (i, url) in urls.iter().enumerate() {
                    let formatted = self.format(url, i == urls.len() - 1);
                    file.write_all(formatted.as_bytes())
                        .context("Failed to write to output file")?;
                }
                Ok(())
            }
            None => {
                if silent {
                    return Ok(());
                };

                for (i, url) in urls.iter().enumerate() {
                    let formatted = self.format(url, i == urls.len() - 1);
                    print!("{}", formatted);
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct JsonOutputter {
    formatter: Box<dyn Formatter>,
}

impl JsonOutputter {
    pub fn new() -> Self {
        JsonOutputter {
            formatter: Box::new(super::JsonFormatter::new()),
        }
    }
}

impl Outputter for JsonOutputter {
    fn format(&self, url: &str, is_last: bool) -> String {
        self.formatter.format(url, is_last)
    }

    fn output(&self, urls: &[String], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                let mut file = File::create(&path).context("Failed to create output file")?;

                file.write_all(b"[")
                    .context("Failed to write JSON opening bracket")?;

                for (i, url) in urls.iter().enumerate() {
                    let formatted = self.format(url, i == urls.len() - 1);
                    file.write_all(formatted.as_bytes())
                        .context("Failed to write to output file")?;
                }

                file.write_all(b"]")
                    .context("Failed to write JSON closing bracket")?;
                Ok(())
            }
            None => {
                if silent {
                    return Ok(());
                };

                print!("[");

                for (i, url) in urls.iter().enumerate() {
                    let formatted = self.format(url, i == urls.len() - 1);
                    print!("{}", formatted);
                }

                println!("]");
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CsvOutputter {
    formatter: Box<dyn Formatter>,
}

impl CsvOutputter {
    pub fn new() -> Self {
        CsvOutputter {
            formatter: Box::new(super::CsvFormatter::new()),
        }
    }
}

impl Outputter for CsvOutputter {
    fn format(&self, url: &str, is_last: bool) -> String {
        self.formatter.format(url, is_last)
    }

    fn output(&self, urls: &[String], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                let mut file = File::create(&path).context("Failed to create output file")?;

                file.write_all(b"url\n")
                    .context("Failed to write CSV header")?;

                for (i, url) in urls.iter().enumerate() {
                    let formatted = self.format(url, i == urls.len() - 1);
                    file.write_all(formatted.as_bytes())
                        .context("Failed to write to output file")?;
                }

                Ok(())
            }
            None => {
                if silent {
                    return Ok(());
                };

                println!("url");

                for (i, url) in urls.iter().enumerate() {
                    let formatted = self.format(url, i == urls.len() - 1);
                    print!("{}", formatted);
                }

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::NamedTempFile;

    #[test]
    fn test_plain_outputter_format() {
        let outputter = PlainOutputter::new();
        assert_eq!(
            outputter.format("https://example.com", false),
            "https://example.com\n"
        );
        assert_eq!(
            outputter.format("https://example.com", true),
            "https://example.com\n"
        );
    }

    #[test]
    fn test_json_outputter_format() {
        let outputter = JsonOutputter::new();
        assert_eq!(
            outputter.format("https://example.com", false),
            "\"https://example.com\","
        );
        assert_eq!(
            outputter.format("https://example.com", true),
            "\"https://example.com\"\n"
        );
    }

    #[test]
    fn test_csv_outputter_format() {
        let outputter = CsvOutputter::new();
        assert_eq!(
            outputter.format("https://example.com", false),
            "https://example.com\n"
        );
        assert_eq!(
            outputter.format("https://example.com", true),
            "https://example.com\n"
        );
    }

    #[test]
    fn test_plain_outputter_file_output() -> Result<()> {
        let outputter = PlainOutputter::new();
        let urls = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();

        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        assert_eq!(
            content,
            "https://example.com/page1\nhttps://example.com/page2\n"
        );

        Ok(())
    }

    #[test]
    fn test_json_outputter_file_output() -> Result<()> {
        let outputter = JsonOutputter::new();
        let urls = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();

        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        assert_eq!(
            content,
            "[\"https://example.com/page1\",\"https://example.com/page2\"\n]"
        );

        Ok(())
    }

    #[test]
    fn test_csv_outputter_file_output() -> Result<()> {
        let outputter = CsvOutputter::new();
        let urls = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();

        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        assert_eq!(
            content,
            "url\nhttps://example.com/page1\nhttps://example.com/page2\n"
        );

        Ok(())
    }

    #[test]
    fn test_empty_urls() -> Result<()> {
        let outputter = PlainOutputter::new();
        let urls: Vec<String> = vec![];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();

        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        assert_eq!(content, "");

        Ok(())
    }
}
