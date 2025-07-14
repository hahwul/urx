use crate::output::Formatter;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

// Outputter implementations for different formats
use super::{Outputter, UrlData};

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
    fn format(&self, url_data: &UrlData, is_last: bool) -> String {
        self.formatter.format(url_data, is_last)
    }

    fn output(&self, urls: &[UrlData], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                let mut file = File::create(&path).context("Failed to create output file")?;

                for (i, url_data) in urls.iter().enumerate() {
                    let formatted = self.format(url_data, i == urls.len() - 1);
                    file.write_all(formatted.as_bytes())
                        .context("Failed to write to output file")?;
                }
                Ok(())
            }
            None => {
                if silent {
                    return Ok(());
                };

                for (i, url_data) in urls.iter().enumerate() {
                    let formatted = self.format(url_data, i == urls.len() - 1);
                    print!("{formatted}");
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
    fn format(&self, url_data: &UrlData, is_last: bool) -> String {
        self.formatter.format(url_data, is_last)
    }

    fn output(&self, urls: &[UrlData], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                let mut file = File::create(&path).context("Failed to create output file")?;

                file.write_all(b"[")
                    .context("Failed to write JSON opening bracket")?;

                for (i, url_data) in urls.iter().enumerate() {
                    let formatted = self.format(url_data, i == urls.len() - 1);
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

                for (i, url_data) in urls.iter().enumerate() {
                    let formatted = self.format(url_data, i == urls.len() - 1);
                    print!("{formatted}");
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
    fn format(&self, url_data: &UrlData, is_last: bool) -> String {
        self.formatter.format(url_data, is_last)
    }

    fn output(&self, urls: &[UrlData], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                let mut file = File::create(&path).context("Failed to create output file")?;

                // Write CSV header (including status if any URLs have status info)
                let has_status = urls.iter().any(|url| url.status.is_some());
                if has_status {
                    file.write_all(b"url,status\n")
                        .context("Failed to write CSV header")?;
                } else {
                    file.write_all(b"url\n")
                        .context("Failed to write CSV header")?;
                }

                for (i, url_data) in urls.iter().enumerate() {
                    let formatted = self.format(url_data, i == urls.len() - 1);
                    file.write_all(formatted.as_bytes())
                        .context("Failed to write to output file")?;
                }

                Ok(())
            }
            None => {
                if silent {
                    return Ok(());
                };

                // Determine if we should include status in header
                let has_status = urls.iter().any(|url| url.status.is_some());
                if has_status {
                    println!("url,status");
                } else {
                    println!("url");
                }

                for (i, url_data) in urls.iter().enumerate() {
                    let formatted = self.format(url_data, i == urls.len() - 1);
                    print!("{formatted}");
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
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(outputter.format(&url_data, false), "https://example.com\n");

        // Test URL with status - checking only that it contains the URL and status text
        // We don't check exact equality because of ANSI color codes
        let url_data_status =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        let formatted = outputter.format(&url_data_status, true);
        assert!(formatted.contains("https://example.com"));
        assert!(formatted.contains("200 OK"));
    }

    #[test]
    fn test_json_outputter_format() {
        let outputter = JsonOutputter::new();
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            outputter.format(&url_data, false),
            "{\"url\":\"https://example.com\"},"
        );

        let url_data_status =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        assert_eq!(
            outputter.format(&url_data_status, true),
            "{\"url\":\"https://example.com\",\"status\":\"200 OK\"}\n"
        );
    }

    #[test]
    fn test_csv_outputter_format() {
        let outputter = CsvOutputter::new();
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(outputter.format(&url_data, false), "https://example.com,\n");

        let url_data_status =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        assert_eq!(
            outputter.format(&url_data_status, true),
            "https://example.com,200 OK\n"
        );
    }

    #[test]
    fn test_plain_outputter_file_output() -> Result<()> {
        let outputter = PlainOutputter::new();
        let urls = vec![
            UrlData::new("https://example.com/page1".to_string()),
            UrlData::with_status(
                "https://example.com/page2".to_string(),
                "200 OK".to_string(),
            ),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();

        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        // Check content contains the URLs and status without asserting exact string equality (due to ANSI color codes)
        assert!(content.contains("https://example.com/page1"));
        assert!(content.contains("https://example.com/page2"));
        assert!(content.contains("200 OK"));

        Ok(())
    }

    #[test]
    fn test_json_outputter_file_output() -> Result<()> {
        let outputter = JsonOutputter::new();
        let urls = vec![
            UrlData::new("https://example.com/page1".to_string()),
            UrlData::with_status(
                "https://example.com/page2".to_string(),
                "200 OK".to_string(),
            ),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();

        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        assert_eq!(
            content,
            "[{\"url\":\"https://example.com/page1\"},{\"url\":\"https://example.com/page2\",\"status\":\"200 OK\"}\n]"
        );

        Ok(())
    }

    #[test]
    fn test_csv_outputter_file_output() -> Result<()> {
        let outputter = CsvOutputter::new();
        let urls = vec![
            UrlData::new("https://example.com/page1".to_string()),
            UrlData::with_status(
                "https://example.com/page2".to_string(),
                "200 OK".to_string(),
            ),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();

        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        assert_eq!(
            content,
            "url,status\nhttps://example.com/page1,\nhttps://example.com/page2,200 OK\n"
        );

        Ok(())
    }

    #[test]
    fn test_empty_urls() -> Result<()> {
        let outputter = PlainOutputter::new();
        let urls: Vec<UrlData> = vec![];

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
