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

    fn output(&self, urls: &[String], output_path: Option<PathBuf>) -> Result<()> {
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

    fn output(&self, urls: &[String], output_path: Option<PathBuf>) -> Result<()> {
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

    fn output(&self, urls: &[String], output_path: Option<PathBuf>) -> Result<()> {
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
