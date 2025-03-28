use anyhow::Result;
use std::path::PathBuf;

mod formatter;
mod writer;

pub use formatter::*;
pub use writer::*;

pub trait Outputter: Send + Sync {
    fn clone_box(&self) -> Box<dyn Outputter>;
    fn format(&self, url: &str, is_last: bool) -> String;
    fn output(&self, urls: &[String], output_path: Option<PathBuf>) -> Result<()>;
}

// Factory function to create outputter based on format
pub fn create_outputter(format: &str) -> Box<dyn Outputter> {
    match format.to_lowercase().as_str() {
        "json" => Box::new(JsonOutputter::new()),
        "csv" => Box::new(CsvOutputter::new()),
        _ => Box::new(PlainOutputter::new()),
    }
}