use anyhow::Result;
use std::path::Path;

mod text_reader;
mod warc_reader;
mod urlteam_reader;

pub use text_reader::TextFileReader;
pub use warc_reader::WarcFileReader;
pub use urlteam_reader::UrlTeamFileReader;

/// Trait for reading URLs from different file formats
pub trait FileReader {
    /// Read URLs from a file and return them as a vector of strings
    fn read_urls(&self, file_path: &Path) -> Result<Vec<String>>;
}