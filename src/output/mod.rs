use anyhow::Result;
use std::path::PathBuf;

use crate::providers::CaptureMeta;

mod formatter;
mod stream;
mod writer;

pub use formatter::*;
pub use stream::{format_supports_streaming, streaming_format_error, StreamSink};
pub use writer::*;

/// A structure to hold URL data with optional status information
#[derive(Debug, Clone, Default)]
pub struct UrlData {
    /// The URL string
    pub url: String,
    /// Optional status information (e.g., HTTP status code)
    pub status: Option<String>,
    /// Providers that reported this URL (sorted, deduped). Empty when unknown.
    pub sources: Vec<String>,
    /// Timestamp of the oldest archived capture, 14-digit CDX form. `None`
    /// unless a provider with a capture index reported this URL.
    pub first_seen: Option<String>,
    /// Timestamp of the newest archived capture, 14-digit CDX form.
    pub last_seen: Option<String>,
    /// MIME type the archive recorded for the newest capture.
    pub mime: Option<String>,
    /// HTTP status the *archive* recorded at capture time. Distinct from
    /// [`UrlData::status`], which `--check-status` produces by re-requesting
    /// the URL live.
    pub archive_status: Option<String>,
    /// One representative content digest across the captures of this URL.
    pub digest: Option<String>,
    /// The `Location` header of a live response. `--check-status` deliberately
    /// does not follow redirects, so this is where a 3xx actually pointed —
    /// recorded, never chased.
    pub location: Option<String>,
    /// The `Content-Length` header of a live response, verbatim.
    pub content_length: Option<String>,
    /// The `Content-Type` header of a live response, verbatim.
    pub content_type: Option<String>,
    /// The HTML `<title>` of a live response, when `--check-title` asked for it.
    pub title: Option<String>,
}

impl UrlData {
    /// Create a new URL data entry without status information
    pub fn new(url: String) -> Self {
        UrlData {
            url,
            ..Default::default()
        }
    }

    /// Create a new URL data entry with status information
    pub fn with_status(url: String, status: String) -> Self {
        UrlData {
            url,
            status: Some(status),
            ..Default::default()
        }
    }

    /// Attach the list of providers that reported this URL. The input is
    /// sorted and deduplicated so output ordering is deterministic.
    pub fn with_sources(mut self, mut sources: Vec<String>) -> Self {
        sources.sort();
        sources.dedup();
        self.sources = sources;
        self
    }

    /// Copy the archive metadata a provider reported for this URL onto the
    /// output record. Absent fields stay absent — the formatters omit them
    /// entirely rather than emitting a placeholder.
    pub fn set_capture_meta(&mut self, meta: &CaptureMeta) {
        self.first_seen = meta.first_seen().map(str::to_string);
        self.last_seen = meta.last_seen().map(str::to_string);
        self.mime = meta.mime().map(str::to_string);
        self.archive_status = meta.archive_status().map(str::to_string);
        self.digest = meta.digest().map(str::to_string);
    }

    /// True when this entry carries any archive metadata at all.
    pub fn has_capture_meta(&self) -> bool {
        self.first_seen.is_some()
            || self.last_seen.is_some()
            || self.mime.is_some()
            || self.archive_status.is_some()
            || self.digest.is_some()
    }

    /// True when this entry carries anything a live response reported beyond
    /// its status code.
    pub fn has_response_meta(&self) -> bool {
        self.location.is_some()
            || self.content_length.is_some()
            || self.content_type.is_some()
            || self.title.is_some()
    }
}

/// Interface for URL output handlers that can format and write URL data
pub trait Outputter: Send + Sync {
    /// Format a URL data entry to a string
    fn format(&self, url_data: &UrlData, is_last: bool) -> String;

    /// Output URL data to console or file
    fn output(&self, urls: &[UrlData], output_path: Option<PathBuf>, silent: bool) -> Result<()>;
}

/// Create an appropriate outputter based on the specified format
///
/// Supported formats:
/// - "json": a single JSON array of entries
/// - "jsonl": JSON Lines — one independent JSON object per line
/// - "csv": CSV format with URL and optional status
/// - "wordlist": the path segments and parameter names the URLs are built from,
///   deduplicated across the run, one term per line
/// - any other value: Plain text format with one URL per line
pub fn create_outputter(format: &str) -> Box<dyn Outputter> {
    match format.to_lowercase().as_str() {
        "json" => Box::new(JsonOutputter::new()),
        "jsonl" => Box::new(JsonLinesOutputter::new()),
        "csv" => Box::new(CsvOutputter::new()),
        "wordlist" => Box::new(WordlistOutputter::new()),
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
        assert_eq!(outputter.format(&url_data, false), "https://example.com\n");
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
            "https://example.com\n"
        );
    }

    #[test]
    fn test_url_data_new() {
        let url_data = UrlData::new("https://example.com/path".to_string());
        assert_eq!(url_data.url, "https://example.com/path");
        assert_eq!(url_data.status, None);
    }

    #[test]
    fn test_url_data_with_status() {
        let url_data = UrlData::with_status(
            "https://example.com".to_string(),
            "404 Not Found".to_string(),
        );
        assert_eq!(url_data.url, "https://example.com");
        assert_eq!(url_data.status, Some("404 Not Found".to_string()));
    }

    #[test]
    fn test_url_data_clone() {
        let original =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        let cloned = original.clone();

        assert_eq!(original.url, cloned.url);
        assert_eq!(original.status, cloned.status);
    }

    #[test]
    fn test_url_data_debug() {
        let url_data = UrlData::new("https://example.com".to_string());
        let debug_str = format!("{:?}", url_data);
        assert!(debug_str.contains("https://example.com"));
    }

    #[test]
    fn test_url_data_with_sources_sorts_and_dedupes() {
        let data = UrlData::new("https://example.com".to_string()).with_sources(vec![
            "wayback".into(),
            "otx".into(),
            "wayback".into(),
            "cc".into(),
        ]);
        assert_eq!(data.sources, vec!["cc", "otx", "wayback"]);
    }

    #[test]
    fn test_create_outputter_empty_format() {
        let outputter = create_outputter("");
        let url_data = UrlData::new("https://example.com".to_string());
        // Empty format should default to plain
        assert_eq!(outputter.format(&url_data, false), "https://example.com\n");
    }

    #[test]
    fn test_has_response_meta_tracks_each_field() {
        assert!(!UrlData::new("https://example.com".to_string()).has_response_meta());
        for set in [
            |d: &mut UrlData| d.location = Some("/x".into()),
            |d: &mut UrlData| d.content_length = Some("1".into()),
            |d: &mut UrlData| d.content_type = Some("text/html".into()),
            |d: &mut UrlData| d.title = Some("t".into()),
        ] {
            let mut data = UrlData::new("https://example.com".to_string());
            set(&mut data);
            assert!(data.has_response_meta());
            // Response metadata is not archive metadata.
            assert!(!data.has_capture_meta());
        }
    }

    #[test]
    fn test_create_outputter_wordlist() {
        let outputter = create_outputter("wordlist");
        let url_data = UrlData::new("https://example.com/admin".to_string());
        assert_eq!(outputter.format(&url_data, false), "admin\n");
    }

    #[test]
    fn test_create_outputter_mixed_case() {
        let outputter = create_outputter("JsOn");
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            outputter.format(&url_data, false),
            "{\"url\":\"https://example.com\"},"
        );
    }
}
