use anyhow::Result;
use std::path::PathBuf;

mod formatter;
mod writer;

pub use formatter::*;
pub use writer::*;

/// A structure to hold URL data with optional status information
#[derive(Debug, Clone)]
pub struct UrlData {
    pub url: String,
    pub status: Option<String>,
}

impl UrlData {
    pub fn new(url: String) -> Self {
        UrlData { url, status: None }
    }

    pub fn with_status(url: String, status: String) -> Self {
        UrlData {
            url,
            status: Some(status),
        }
    }

    pub fn from_string(data: String) -> Self {
        // Parse strings in the format "{url} - {status}" if possible
        if let Some(idx) = data.find(" - ") {
            let (url, status) = data.split_at(idx);
            // Remove the " - " prefix from status
            let status = status[3..].to_string();
            UrlData {
                url: url.to_string(),
                status: Some(status),
            }
        } else {
            // No status information found
            UrlData {
                url: data,
                status: None,
            }
        }
    }
}

pub trait Outputter: Send + Sync {
    fn format(&self, url_data: &UrlData, is_last: bool) -> String;
    fn output(&self, urls: &[UrlData], output_path: Option<PathBuf>, silent: bool) -> Result<()>;
}

pub fn create_outputter(format: &str) -> Box<dyn Outputter> {
    match format.to_lowercase().as_str() {
        "json" => Box::new(JsonOutputter::new()),
        "csv" => Box::new(CsvOutputter::new()),
        _ => Box::new(PlainOutputter::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_outputter_json() {
        let outputter = create_outputter("json");
        // Checks the output of the format method
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            outputter.format(&url_data, false),
            "{\"url\":\"https://example.com\"},"
        );
    }

    #[test]
    fn test_create_outputter_csv() {
        let outputter = create_outputter("csv");
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(outputter.format(&url_data, false), "https://example.com,\n");
    }

    #[test]
    fn test_create_outputter_plain() {
        let outputter = create_outputter("plain");
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(outputter.format(&url_data, false), "https://example.com\n");
    }

    #[test]
    fn test_create_outputter_default_for_unknown() {
        let outputter = create_outputter("unknown");
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(outputter.format(&url_data, false), "https://example.com\n");
    }

    #[test]
    fn test_create_outputter_case_insensitive() {
        let json_outputter = create_outputter("JSON");
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            json_outputter.format(&url_data, false),
            "{\"url\":\"https://example.com\"},"
        );

        let csv_outputter = create_outputter("CSV");
        assert_eq!(
            csv_outputter.format(&url_data, false),
            "https://example.com,\n"
        );
    }

    #[test]
    fn test_url_data_from_string() {
        let url_only = UrlData::from_string("https://example.com".to_string());
        assert_eq!(url_only.url, "https://example.com");
        assert_eq!(url_only.status, None);

        let with_status = UrlData::from_string("https://example.com - 200 OK".to_string());
        assert_eq!(with_status.url, "https://example.com");
        assert_eq!(with_status.status, Some("200 OK".to_string()));
    }
}
