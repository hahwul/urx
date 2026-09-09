use anyhow::Result;
use serde::{Deserialize, Serialize};
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

/// Marks a tester result line that carries a serialized [`TesterRecord`] rather
/// than the plain `"{url} - {status}"` form.
///
/// [`crate::testers::Tester::test_url`] hands results back as strings, which was
/// enough while a status code was the only thing the checker reported. Response
/// metadata does not fit that shape — a header value or a page title can contain
/// anything, `" - "` included — so the checker serializes the record it built and
/// [`UrlData::from_string`] reads it back. U+0001 cannot occur in a URL, so a
/// line starting with it is unambiguously ours, and one that starts with it but
/// fails to parse still falls back to the plain form.
const RECORD_SENTINEL: char = '\u{1}';

/// The part of a [`UrlData`] a tester can produce, as it travels back through
/// the string channel.
///
/// Sources and archive capture metadata are absent on purpose: they are
/// attached later, from the provider run result, and a tester knows nothing
/// about them.
#[derive(Debug, Default, Serialize, Deserialize)]
struct TesterRecord {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_length: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
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

    /// Encode this entry as one [`crate::testers::Tester`] result line.
    ///
    /// Without response metadata the historical `"{url} - {status}"` spelling is
    /// kept, so a run that collected none produces exactly the bytes it always
    /// did. With it, the record is serialized behind [`RECORD_SENTINEL`], which
    /// [`UrlData::from_string`] recognises.
    pub fn to_tester_line(&self) -> String {
        let plain = || match &self.status {
            Some(status) => format!("{} - {}", self.url, status),
            None => self.url.clone(),
        };

        if !self.has_response_meta() {
            return plain();
        }

        let record = TesterRecord {
            url: self.url.clone(),
            status: self.status.clone(),
            location: self.location.clone(),
            content_length: self.content_length.clone(),
            content_type: self.content_type.clone(),
            title: self.title.clone(),
        };
        match serde_json::to_string(&record) {
            Ok(json) => format!("{RECORD_SENTINEL}{json}"),
            // Unreachable for a struct of plain strings, but losing the URL
            // entirely would be far worse than losing the metadata.
            Err(_) => plain(),
        }
    }

    /// Parse a URL data entry from a string
    ///
    /// Handles a [`RECORD_SENTINEL`]-prefixed serialized record, strings in the
    /// format "{url} - {status}", and plain URLs.
    /// Archived URLs can themselves contain " - " (unencoded spaces do show up
    /// in CDX rows), so the split takes the *last* separator: the status the
    /// checker appends is always the final field, whereas the URL is not
    /// guaranteed to be separator-free.
    pub fn from_string(data: String) -> Self {
        // A serialized record, written by a tester that had more than a status
        // code to report. Anything that does not parse falls through to the
        // plain form below rather than being lost.
        if let Some(json) = data.strip_prefix(RECORD_SENTINEL) {
            if let Ok(record) = serde_json::from_str::<TesterRecord>(json) {
                return UrlData {
                    url: record.url,
                    status: record.status,
                    location: record.location,
                    content_length: record.content_length,
                    content_type: record.content_type,
                    title: record.title,
                    ..Default::default()
                };
            }
        }

        // Parse strings in the format "{url} - {status}" if possible
        if let Some((url, status)) = data.rsplit_once(" - ") {
            UrlData {
                url: url.to_string(),
                status: Some(status.to_string()),
                ..Default::default()
            }
        } else {
            // No status information found
            UrlData {
                url: data,
                ..Default::default()
            }
        }
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
    fn test_url_data_from_string() {
        let url_only = UrlData::from_string("https://example.com".to_string());
        assert_eq!(url_only.url, "https://example.com");
        assert_eq!(url_only.status, None);

        let with_status = UrlData::from_string("https://example.com - 200 OK".to_string());
        assert_eq!(with_status.url, "https://example.com");
        assert_eq!(with_status.status, Some("200 OK".to_string()));
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
    fn test_url_data_from_string_multiple_dashes() {
        // Test URL that contains " - " in the path should be correctly parsed
        let with_status = UrlData::from_string(
            "https://example.com/path-to-page - 301 Moved Permanently".to_string(),
        );
        assert_eq!(with_status.url, "https://example.com/path-to-page");
        assert_eq!(
            with_status.status,
            Some("301 Moved Permanently".to_string())
        );
    }

    #[test]
    fn test_url_data_from_string_url_containing_the_separator() {
        // A URL with a literal " - " in it used to be truncated at that point,
        // with the rest of the URL swallowed into the status field.
        let parsed = UrlData::from_string("https://example.com/a - b/c.pdf - 200 OK".to_string());
        assert_eq!(parsed.url, "https://example.com/a - b/c.pdf");
        assert_eq!(parsed.status.as_deref(), Some("200 OK"));
    }

    #[test]
    fn test_url_data_from_string_with_complex_status() {
        let with_status = UrlData::from_string(
            "https://example.com/api/v1/users?id=123 - 500 Internal Server Error".to_string(),
        );
        assert_eq!(with_status.url, "https://example.com/api/v1/users?id=123");
        assert_eq!(
            with_status.status,
            Some("500 Internal Server Error".to_string())
        );
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

    /// A record carrying everything a live response can report.
    fn with_response_meta(url: &str) -> UrlData {
        let mut data = UrlData::with_status(url.to_string(), "301 Moved Permanently".to_string());
        data.location = Some("https://example.com/final".to_string());
        data.content_length = Some("1234".to_string());
        data.content_type = Some("text/html; charset=utf-8".to_string());
        data.title = Some("Example — Home".to_string());
        data
    }

    #[test]
    fn test_tester_line_without_response_metadata_is_the_historical_form() {
        // The contract that keeps an existing --check-status run byte-identical.
        let data = UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        assert_eq!(data.to_tester_line(), "https://example.com - 200 OK");
        assert!(!data.has_response_meta());

        let bare = UrlData::new("https://example.com".to_string());
        assert_eq!(bare.to_tester_line(), "https://example.com");
    }

    #[test]
    fn test_tester_line_round_trips_response_metadata() {
        let original = with_response_meta("https://example.com/redir");
        let parsed = UrlData::from_string(original.to_tester_line());

        assert_eq!(parsed.url, original.url);
        assert_eq!(parsed.status, original.status);
        assert_eq!(parsed.location, original.location);
        assert_eq!(parsed.content_length, original.content_length);
        assert_eq!(parsed.content_type, original.content_type);
        assert_eq!(parsed.title, original.title);
    }

    #[test]
    fn test_tester_line_survives_values_containing_the_plain_separator() {
        // Exactly why the record channel exists: a title or a URL may contain
        // " - ", which the plain form would split on.
        let mut data = UrlData::with_status(
            "https://example.com/a - b/c.pdf".to_string(),
            "200 OK".to_string(),
        );
        data.title = Some("Q1 - Q2 - annual report".to_string());

        let parsed = UrlData::from_string(data.to_tester_line());
        assert_eq!(parsed.url, "https://example.com/a - b/c.pdf");
        assert_eq!(parsed.status.as_deref(), Some("200 OK"));
        assert_eq!(parsed.title.as_deref(), Some("Q1 - Q2 - annual report"));
    }

    #[test]
    fn test_a_sentinel_line_that_is_not_a_record_falls_back_to_the_plain_form() {
        // Nothing is lost when the payload does not parse.
        let parsed = UrlData::from_string("\u{1}not json - 200 OK".to_string());
        assert_eq!(parsed.url, "\u{1}not json");
        assert_eq!(parsed.status.as_deref(), Some("200 OK"));
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
