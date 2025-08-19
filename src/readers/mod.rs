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

/// Enum representing different file formats
#[derive(Debug, Clone, PartialEq)]
pub enum FileFormat {
    Warc,
    UrlTeam,
    Text,
}

/// Auto-detect file format based on file extension and content
pub fn detect_file_format(file_path: &Path) -> Result<FileFormat> {
    // First try to detect based on file extension
    if let Some(extension) = file_path.extension() {
        let ext = extension.to_string_lossy().to_lowercase();
        
        match ext.as_str() {
            "warc" => return Ok(FileFormat::Warc),
            "gz" | "bz2" => {
                // For compressed files, check if it's likely URLTeam format
                // URLTeam files typically have names containing "urlteam" or similar patterns
                let filename = file_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                
                if filename.contains("urlteam") || filename.contains("url_team") {
                    return Ok(FileFormat::UrlTeam);
                }
                
                // For other .gz/.bz2 files, default to URLTeam format
                return Ok(FileFormat::UrlTeam);
            }
            "txt" | "list" => return Ok(FileFormat::Text),
            _ => {}
        }
    }
    
    // If extension doesn't help, check filename patterns
    let filename = file_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    if filename.contains("warc") {
        return Ok(FileFormat::Warc);
    }
    
    if filename.contains("urlteam") || filename.contains("url_team") {
        return Ok(FileFormat::UrlTeam);
    }
    
    // Default to text format for unknown files
    Ok(FileFormat::Text)
}

/// Read URLs from a file using auto-detected format
pub fn read_urls_from_file(file_path: &Path) -> Result<Vec<String>> {
    let format = detect_file_format(file_path)?;
    
    match format {
        FileFormat::Warc => {
            let reader = WarcFileReader::new();
            reader.read_urls(file_path)
        }
        FileFormat::UrlTeam => {
            let reader = UrlTeamFileReader::new();
            reader.read_urls(file_path)
        }
        FileFormat::Text => {
            let reader = TextFileReader::new();
            reader.read_urls(file_path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_warc_format() {
        let path = PathBuf::from("test.warc");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);
        
        let path = PathBuf::from("archive.warc");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);
        
        let path = PathBuf::from("some_warc_file.dat");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Warc);
    }

    #[test]
    fn test_detect_urlteam_format() {
        let path = PathBuf::from("urlteam_data.gz");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::UrlTeam);
        
        let path = PathBuf::from("url_team_archive.bz2");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::UrlTeam);
        
        let path = PathBuf::from("data.gz");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::UrlTeam);
    }

    #[test]
    fn test_detect_text_format() {
        let path = PathBuf::from("urls.txt");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);
        
        let path = PathBuf::from("list.list");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);
        
        let path = PathBuf::from("unknown_file");
        assert_eq!(detect_file_format(&path).unwrap(), FileFormat::Text);
    }
}