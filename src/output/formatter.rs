/// Implements different URL output formatters
use super::UrlData;
use colored::*;
use serde::Serialize;
use std::fmt;

/// Helper struct for JSON serialization with guaranteed field order
/// (url, status, sources). `sources` is omitted when empty so the output
/// stays backward-compatible with callers that don't ask for attribution.
#[derive(Serialize)]
struct JsonUrlEntry<'a> {
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'a str>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    sources: &'a [String],
}

/// Formatter trait for converting URL data to different output formats
pub trait Formatter: fmt::Debug + Send + Sync {
    /// Format a URL data entry to a string representation
    ///
    /// The is_last parameter indicates whether this is the last item
    /// in a sequence, which can be important for certain formats like JSON
    fn format(&self, url_data: &UrlData, is_last: bool) -> String;

    /// Create a boxed clone of this formatter
    fn clone_box(&self) -> Box<dyn Formatter>;
}

impl Clone for Box<dyn Formatter> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Plain text formatter that outputs URLs one per line
#[derive(Debug, Clone)]
pub struct PlainFormatter;

impl PlainFormatter {
    /// Create a new plain text formatter
    pub fn new() -> Self {
        PlainFormatter
    }
}

impl Formatter for PlainFormatter {
    fn format(&self, url_data: &UrlData, _is_last: bool) -> String {
        let mut line = match &url_data.status {
            Some(status) => {
                let status_code_str = status.split_whitespace().next().unwrap_or("");
                let colored_status = match status_code_str.parse::<u16>() {
                    Ok(code) => match code {
                        200..=299 => status.green(),
                        300..=399 => status.yellow(),
                        400..=499 => status.red(),
                        500..=599 => status.magenta(),
                        _ => status.normal(),
                    },
                    Err(_) => status.normal(),
                };
                format!("{} [{}]", url_data.url, colored_status)
            }
            None => url_data.url.clone(),
        };
        if !url_data.sources.is_empty() {
            line.push_str(&format!(" [{}]", url_data.sources.join(",").cyan()));
        }
        line.push('\n');
        line
    }

    fn clone_box(&self) -> Box<dyn Formatter> {
        Box::new(self.clone())
    }
}

/// JSON formatter that outputs URLs as JSON objects
#[derive(Debug, Clone)]
pub struct JsonFormatter;

impl JsonFormatter {
    /// Create a new JSON formatter
    pub fn new() -> Self {
        JsonFormatter
    }
}

impl Formatter for JsonFormatter {
    fn format(&self, url_data: &UrlData, is_last: bool) -> String {
        let entry = JsonUrlEntry {
            url: &url_data.url,
            status: url_data.status.as_deref(),
            sources: &url_data.sources,
        };
        let json = serde_json::to_string(&entry).unwrap_or_default();

        if is_last {
            format!("{json}\n")
        } else {
            format!("{json},")
        }
    }

    fn clone_box(&self) -> Box<dyn Formatter> {
        Box::new(self.clone())
    }
}

/// CSV formatter that outputs URLs in comma-separated format
#[derive(Debug, Clone)]
pub struct CsvFormatter;

impl CsvFormatter {
    /// Create a new CSV formatter
    pub fn new() -> Self {
        CsvFormatter
    }
}

impl Formatter for CsvFormatter {
    fn format(&self, url_data: &UrlData, _is_last: bool) -> String {
        // Standalone row: include only the columns this entry actually has,
        // so a single formatted row is self-consistent (no dangling commas).
        csv_row(
            url_data,
            url_data.status.is_some(),
            !url_data.sources.is_empty(),
        )
    }

    fn clone_box(&self) -> Box<dyn Formatter> {
        Box::new(self.clone())
    }
}

/// Build the CSV header line for the given column layout. The `url` column is
/// always present; `status` / `sources` are included only when the run carries
/// that data, and the row formatter mirrors exactly the same layout so every
/// line has an identical column count.
pub(crate) fn csv_header(has_status: bool, has_sources: bool) -> String {
    let mut cols = vec!["url"];
    if has_status {
        cols.push("status");
    }
    if has_sources {
        cols.push("sources");
    }
    let mut line = cols.join(",");
    line.push('\n');
    line
}

/// Format one CSV data row for the given column layout. Must agree with
/// [`csv_header`] on which columns are emitted so header and body stay aligned.
pub(crate) fn csv_row(url_data: &UrlData, has_status: bool, has_sources: bool) -> String {
    let mut fields = vec![csv_escape(&url_data.url)];
    if has_status {
        fields.push(
            url_data
                .status
                .as_deref()
                .map(csv_escape)
                .unwrap_or_default(),
        );
    }
    if has_sources {
        fields.push(if url_data.sources.is_empty() {
            String::new()
        } else {
            csv_escape(&url_data.sources.join("|"))
        });
    }
    let mut line = fields.join(",");
    line.push('\n');
    line
}

/// Escape a field value for CSV output per RFC 4180.
/// If the value contains a comma, double-quote, or newline, wrap it in
/// double-quotes and escape any internal double-quotes by doubling them.
pub(crate) fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        let escaped = value.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_formatter() {
        let formatter = PlainFormatter::new();

        // Test URL without status
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(formatter.format(&url_data, false), "https://example.com\n");

        // Test URL with status - checking only that it contains the URL and status text
        // We don't check exact equality because of ANSI color codes
        let url_data_status =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        let formatted = formatter.format(&url_data_status, true);
        assert!(formatted.contains("https://example.com"));
        assert!(formatted.contains("200 OK"));
    }

    #[test]
    fn test_plain_formatter_status_coloring() {
        let formatter = PlainFormatter::new();

        // Test 2xx status codes (green)
        let url_data_200 =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        let url_data_201 =
            UrlData::with_status("https://example.com".to_string(), "201 Created".to_string());
        let url_data_299 =
            UrlData::with_status("https://example.com".to_string(), "299 Custom".to_string());

        // Test 3xx status codes (yellow)
        let url_data_301 = UrlData::with_status(
            "https://example.com".to_string(),
            "301 Moved Permanently".to_string(),
        );
        let url_data_302 =
            UrlData::with_status("https://example.com".to_string(), "302 Found".to_string());
        let url_data_307 = UrlData::with_status(
            "https://example.com".to_string(),
            "307 Temporary Redirect".to_string(),
        );

        // Test 4xx status codes (red)
        let url_data_400 = UrlData::with_status(
            "https://example.com".to_string(),
            "400 Bad Request".to_string(),
        );
        let url_data_404 = UrlData::with_status(
            "https://example.com".to_string(),
            "404 Not Found".to_string(),
        );
        let url_data_429 = UrlData::with_status(
            "https://example.com".to_string(),
            "429 Too Many Requests".to_string(),
        );

        // Test 5xx status codes (magenta)
        let url_data_500 = UrlData::with_status(
            "https://example.com".to_string(),
            "500 Internal Server Error".to_string(),
        );
        let url_data_502 = UrlData::with_status(
            "https://example.com".to_string(),
            "502 Bad Gateway".to_string(),
        );
        let url_data_503 = UrlData::with_status(
            "https://example.com".to_string(),
            "503 Service Unavailable".to_string(),
        );

        // Test other status codes (normal)
        let url_data_000 =
            UrlData::with_status("https://example.com".to_string(), "000 Custom".to_string());
        let url_data_600 =
            UrlData::with_status("https://example.com".to_string(), "600 Custom".to_string());
        let url_data_invalid = UrlData::with_status(
            "https://example.com".to_string(),
            "Invalid Status".to_string(),
        );

        // Note: We can't easily test the exact color output since colored crate renders
        // terminal color codes, but we can at least verify that the formatting works
        // by checking the output contains the status

        // Format and verify each status code is included in output
        assert!(formatter.format(&url_data_200, false).contains("200 OK"));
        assert!(formatter
            .format(&url_data_201, false)
            .contains("201 Created"));
        assert!(formatter
            .format(&url_data_299, false)
            .contains("299 Custom"));

        assert!(formatter
            .format(&url_data_301, false)
            .contains("301 Moved Permanently"));
        assert!(formatter.format(&url_data_302, false).contains("302 Found"));
        assert!(formatter
            .format(&url_data_307, false)
            .contains("307 Temporary Redirect"));

        assert!(formatter
            .format(&url_data_400, false)
            .contains("400 Bad Request"));
        assert!(formatter
            .format(&url_data_404, false)
            .contains("404 Not Found"));
        assert!(formatter
            .format(&url_data_429, false)
            .contains("429 Too Many Requests"));

        assert!(formatter
            .format(&url_data_500, false)
            .contains("500 Internal Server Error"));
        assert!(formatter
            .format(&url_data_502, false)
            .contains("502 Bad Gateway"));
        assert!(formatter
            .format(&url_data_503, false)
            .contains("503 Service Unavailable"));

        assert!(formatter
            .format(&url_data_000, false)
            .contains("000 Custom"));
        assert!(formatter
            .format(&url_data_600, false)
            .contains("600 Custom"));
        assert!(formatter
            .format(&url_data_invalid, false)
            .contains("Invalid Status"));
    }

    #[test]
    fn test_json_formatter() {
        let formatter = JsonFormatter::new();

        // Test URL without status
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            formatter.format(&url_data, false),
            "{\"url\":\"https://example.com\"},"
        );
        assert_eq!(
            formatter.format(&url_data, true),
            "{\"url\":\"https://example.com\"}\n"
        );

        // Test URL with status
        let url_data_status =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        assert_eq!(
            formatter.format(&url_data_status, false),
            "{\"url\":\"https://example.com\",\"status\":\"200 OK\"},"
        );
    }

    #[test]
    fn test_csv_formatter() {
        let formatter = CsvFormatter::new();

        // Test URL without status: a lone url is a single column, no dangling comma
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(formatter.format(&url_data, false), "https://example.com\n");

        // Test URL with status
        let url_data_status =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        assert_eq!(
            formatter.format(&url_data_status, true),
            "https://example.com,200 OK\n"
        );
    }

    #[test]
    fn test_csv_escape_plain() {
        assert_eq!(csv_escape("hello"), "hello");
    }

    #[test]
    fn test_csv_escape_with_comma() {
        assert_eq!(
            csv_escape("https://example.com/path?a=1,2"),
            "\"https://example.com/path?a=1,2\""
        );
    }

    #[test]
    fn test_csv_escape_with_quote() {
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_csv_formatter_with_special_chars() {
        let formatter = CsvFormatter::new();
        let url_data = UrlData::new("https://example.com/path?a=1,2&b=3".to_string());
        assert_eq!(
            formatter.format(&url_data, false),
            "\"https://example.com/path?a=1,2&b=3\"\n"
        );
    }

    #[test]
    fn test_json_formatter_with_sources() {
        let formatter = JsonFormatter::new();
        let url_data = UrlData::new("https://example.com".to_string()).with_sources(vec![
            "wayback".into(),
            "otx".into(),
            "wayback".into(),
        ]);
        // Sources are sorted and deduped; field appears after url/status.
        assert_eq!(
            formatter.format(&url_data, true),
            "{\"url\":\"https://example.com\",\"sources\":[\"otx\",\"wayback\"]}\n"
        );
    }

    #[test]
    fn test_csv_formatter_with_sources() {
        let formatter = CsvFormatter::new();
        let url_data =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string())
                .with_sources(vec!["wayback".into(), "cc".into()]);
        // Sources column is pipe-separated when present.
        assert_eq!(
            formatter.format(&url_data, true),
            "https://example.com,200 OK,cc|wayback\n"
        );
    }

    #[test]
    fn test_plain_formatter_with_sources() {
        let formatter = PlainFormatter::new();
        let url_data = UrlData::new("https://example.com".to_string())
            .with_sources(vec!["wayback".into(), "cc".into()]);
        let out = formatter.format(&url_data, true);
        // Plain output appends [provider,provider] (ANSI codes may be present).
        assert!(out.starts_with("https://example.com "));
        assert!(out.contains("cc"));
        assert!(out.contains("wayback"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn test_formatter_clone() {
        let plain_formatter: Box<dyn Formatter> = Box::new(PlainFormatter::new());
        let cloned_formatter = plain_formatter.clone();

        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            plain_formatter.format(&url_data, false),
            cloned_formatter.format(&url_data, false)
        );
    }
}
