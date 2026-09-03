/// Implements different URL output formatters
use super::UrlData;
use colored::*;
use serde::Serialize;
use std::borrow::Cow;
use std::fmt;

/// Helper struct for JSON serialization with guaranteed field order
/// (url, status, sources, then the archive metadata). Every optional field is
/// omitted when absent rather than emitted as `null`, so a run that collected
/// no metadata produces byte-identical output to before it existed.
#[derive(Serialize)]
struct JsonUrlEntry<'a> {
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'a str>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    sources: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    first_seen: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<&'a str>,
}

impl<'a> JsonUrlEntry<'a> {
    fn from_data(url_data: &'a UrlData) -> Self {
        JsonUrlEntry {
            url: &url_data.url,
            status: url_data.status.as_deref(),
            sources: &url_data.sources,
            first_seen: url_data.first_seen.as_deref(),
            last_seen: url_data.last_seen.as_deref(),
            mime: url_data.mime.as_deref(),
            archive_status: url_data.archive_status.as_deref(),
            digest: url_data.digest.as_deref(),
        }
    }
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
        // Only reached when `--show-meta` asked for it: the caller leaves these
        // fields empty otherwise, so plain output stays a stable pipeline
        // contract by default.
        let meta = plain_meta(url_data);
        if !meta.is_empty() {
            line.push_str(&format!(" [{}]", meta.blue()));
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
        let json = serde_json::to_string(&JsonUrlEntry::from_data(url_data)).unwrap_or_default();

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

/// JSON Lines formatter: one self-contained JSON object per line.
///
/// Unlike [`JsonFormatter`], no entry depends on its position — there is no
/// enclosing array and no separating comma — so output stays valid when it is
/// truncated, appended to, or consumed a line at a time (`jq -c`, `head`, or a
/// streaming run that cannot know which record is last).
#[derive(Debug, Clone)]
pub struct JsonLinesFormatter;

impl JsonLinesFormatter {
    /// Create a new JSON Lines formatter
    pub fn new() -> Self {
        JsonLinesFormatter
    }
}

impl Formatter for JsonLinesFormatter {
    fn format(&self, url_data: &UrlData, _is_last: bool) -> String {
        let json = serde_json::to_string(&JsonUrlEntry::from_data(url_data)).unwrap_or_default();
        format!("{json}\n")
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
        csv_row(url_data, &CsvLayout::for_row(url_data))
    }

    fn clone_box(&self) -> Box<dyn Formatter> {
        Box::new(self.clone())
    }
}

/// Render the archive metadata of one entry as a compact `key=value` list for
/// plain-text output. Empty when the entry carries none.
fn plain_meta(url_data: &UrlData) -> String {
    let fields = [
        ("first_seen", url_data.first_seen.as_deref()),
        ("last_seen", url_data.last_seen.as_deref()),
        ("mime", url_data.mime.as_deref()),
        ("archive_status", url_data.archive_status.as_deref()),
        ("digest", url_data.digest.as_deref()),
    ];
    fields
        .iter()
        .filter_map(|(name, value)| value.map(|v| format!("{name}={v}")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The optional CSV columns, in output order. `url` is always first and is not
/// listed here. [`csv_optional_values`] returns one value per entry in exactly
/// this order, which is what keeps header and rows aligned.
const CSV_OPTIONAL_COLUMNS: [&str; 7] = [
    "status",
    "sources",
    "first_seen",
    "last_seen",
    "mime",
    "archive_status",
    "digest",
];

/// This entry's value for each optional column, `None` where it has none.
fn csv_optional_values(url_data: &UrlData) -> [Option<Cow<'_, str>>; 7] {
    [
        url_data.status.as_deref().map(Cow::Borrowed),
        (!url_data.sources.is_empty()).then(|| Cow::Owned(url_data.sources.join("|"))),
        url_data.first_seen.as_deref().map(Cow::Borrowed),
        url_data.last_seen.as_deref().map(Cow::Borrowed),
        url_data.mime.as_deref().map(Cow::Borrowed),
        url_data.archive_status.as_deref().map(Cow::Borrowed),
        url_data.digest.as_deref().map(Cow::Borrowed),
    ]
}

/// Which optional CSV columns a result set needs.
///
/// A column is emitted only when at least one row has a value for it, which is
/// what keeps a plain URL run's CSV a single `url` column. Header and row are
/// built from the same layout, so every line has an identical column count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CsvLayout([bool; CSV_OPTIONAL_COLUMNS.len()]);

impl CsvLayout {
    /// The layout covering every column at least one of `urls` populates.
    pub(crate) fn for_rows(urls: &[UrlData]) -> Self {
        let mut on = [false; CSV_OPTIONAL_COLUMNS.len()];
        for url_data in urls {
            for (slot, value) in on.iter_mut().zip(csv_optional_values(url_data)) {
                *slot |= value.is_some();
            }
        }
        CsvLayout(on)
    }

    /// The layout for a single standalone row, so a row formatted on its own is
    /// self-consistent (no dangling commas).
    fn for_row(url_data: &UrlData) -> Self {
        CsvLayout::for_rows(std::slice::from_ref(url_data))
    }
}

/// Build the CSV header line for the given column layout. The `url` column is
/// always present; the rest follow [`CsvLayout`].
pub(crate) fn csv_header(layout: &CsvLayout) -> String {
    let mut line = String::from("url");
    for (name, on) in CSV_OPTIONAL_COLUMNS.iter().zip(layout.0) {
        if on {
            line.push(',');
            line.push_str(name);
        }
    }
    line.push('\n');
    line
}

/// Format one CSV data row for the given column layout. Must agree with
/// [`csv_header`] on which columns are emitted so header and body stay aligned.
pub(crate) fn csv_row(url_data: &UrlData, layout: &CsvLayout) -> String {
    let mut line = csv_escape(&url_data.url);
    for (value, on) in csv_optional_values(url_data).into_iter().zip(layout.0) {
        if on {
            line.push(',');
            line.push_str(&value.map(|v| csv_escape(&v)).unwrap_or_default());
        }
    }
    line.push('\n');
    line
}

/// Leading characters that make a spreadsheet evaluate a cell as a formula
/// rather than display it. A tab or carriage return counts because Excel strips
/// it and then looks at the next character.
const FORMULA_TRIGGERS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// Escape a field value for CSV output per RFC 4180.
///
/// A value containing a comma, double-quote, CR or LF is wrapped in
/// double-quotes with any internal double-quotes doubled. CR matters as much as
/// LF: a bare `\r` in a field splits the row in Excel and in most spreadsheet
/// importers, so leaving it unquoted corrupts the table.
///
/// A value *starting* with one of [`FORMULA_TRIGGERS`] is additionally prefixed
/// with an apostrophe. Every field here is archive-controlled — the URL itself,
/// and under `--show-only-param` a parameter name lifted verbatim out of a
/// query string — so `=cmd|'/C calc'!A0` reaches the CSV unaltered and executes
/// when the file is opened. Quoting alone does not stop that; the apostrophe is
/// the standard mitigation for CSV formula injection (CWE-1236).
pub(crate) fn csv_escape(value: &str) -> String {
    let starts_formula = value.starts_with(FORMULA_TRIGGERS);
    let needs_quoting = starts_formula
        || value.contains(',')
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r');

    if !needs_quoting {
        return value.to_string();
    }

    let escaped = value.replace('"', "\"\"");
    if starts_formula {
        format!("\"'{escaped}\"")
    } else {
        format!("\"{escaped}\"")
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
    fn test_csv_escape_neutralises_formula_triggers() {
        // Regression: a field starting with a formula trigger reached the CSV
        // verbatim, so `urx ... --show-only-param -f csv` could emit
        // `=cmd|'/C calc'!A0=1` and a spreadsheet would execute it on open.
        for trigger in ['=', '+', '-', '@', '\t', '\r'] {
            let value = format!("{trigger}cmd|'/C calc'!A0");
            let escaped = csv_escape(&value);
            assert!(
                escaped.starts_with("\"'"),
                "{value:?} must be neutralised, got {escaped:?}"
            );
        }
        assert_eq!(csv_escape("=1+1"), "\"'=1+1\"");
        // A trigger anywhere but the start is harmless and left alone.
        assert_eq!(csv_escape("https://a.com/x=1"), "https://a.com/x=1");
    }

    #[test]
    fn test_csv_escape_quotes_carriage_return() {
        // A bare CR splits the row in Excel and most importers, so it has to be
        // quoted just like LF.
        assert_eq!(csv_escape("https://a.com/a\rb"), "\"https://a.com/a\rb\"");
        assert_eq!(
            csv_escape("https://a.com/a\r\nb"),
            "\"https://a.com/a\r\nb\""
        );
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

    /// A record carrying every archive metadata field.
    fn with_meta(url: &str) -> UrlData {
        let mut data = UrlData::new(url.to_string());
        data.first_seen = Some("20050101000000".to_string());
        data.last_seen = Some("20240101000000".to_string());
        data.mime = Some("text/html".to_string());
        data.archive_status = Some("200".to_string());
        data.digest = Some("ABCDEF".to_string());
        data
    }

    #[test]
    fn test_json_omits_metadata_keys_when_absent() {
        // The contract that keeps existing consumers working: a run that
        // collected no metadata is byte-identical to before the fields existed.
        let formatter = JsonFormatter::new();
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            formatter.format(&url_data, true),
            "{\"url\":\"https://example.com\"}\n"
        );
    }

    #[test]
    fn test_json_emits_every_metadata_field_it_has() {
        let formatter = JsonFormatter::new();
        assert_eq!(
            formatter.format(&with_meta("https://example.com"), true),
            "{\"url\":\"https://example.com\",\
             \"first_seen\":\"20050101000000\",\
             \"last_seen\":\"20240101000000\",\
             \"mime\":\"text/html\",\
             \"archive_status\":\"200\",\
             \"digest\":\"ABCDEF\"}\n"
        );
    }

    #[test]
    fn test_json_emits_only_the_metadata_fields_present() {
        let formatter = JsonLinesFormatter::new();
        let mut url_data = UrlData::new("https://example.com".to_string());
        url_data.first_seen = Some("20050101000000".to_string());
        assert_eq!(
            formatter.format(&url_data, true),
            "{\"url\":\"https://example.com\",\"first_seen\":\"20050101000000\"}\n"
        );
    }

    #[test]
    fn test_csv_layout_adds_a_column_only_when_some_row_uses_it() {
        let rows = vec![UrlData::new("https://example.com/a".to_string()), {
            let mut d = UrlData::new("https://example.com/b".to_string());
            d.mime = Some("text/html".to_string());
            d
        }];
        let layout = CsvLayout::for_rows(&rows);

        assert_eq!(csv_header(&layout), "url,mime\n");
        // The row without a MIME type still emits the column, empty, so every
        // line has the same number of fields.
        assert_eq!(csv_row(&rows[0], &layout), "https://example.com/a,\n");
        assert_eq!(
            csv_row(&rows[1], &layout),
            "https://example.com/b,text/html\n"
        );
    }

    #[test]
    fn test_csv_header_is_url_only_without_any_metadata() {
        let rows = vec![UrlData::new("https://example.com".to_string())];
        assert_eq!(csv_header(&CsvLayout::for_rows(&rows)), "url\n");
    }

    #[test]
    fn test_csv_columns_follow_the_documented_order() {
        let mut row = with_meta("https://example.com");
        row.status = Some("200 OK".to_string());
        row.sources = vec!["wayback".to_string()];
        let rows = vec![row];
        let layout = CsvLayout::for_rows(&rows);

        assert_eq!(
            csv_header(&layout),
            "url,status,sources,first_seen,last_seen,mime,archive_status,digest\n"
        );
        assert_eq!(
            csv_row(&rows[0], &layout),
            "https://example.com,200 OK,wayback,20050101000000,20240101000000,text/html,200,ABCDEF\n"
        );
    }

    #[test]
    fn test_plain_output_is_unchanged_without_metadata() {
        // The pipeline contract: `urx target.com | httpx` must keep seeing one
        // bare URL per line.
        let formatter = PlainFormatter::new();
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(formatter.format(&url_data, true), "https://example.com\n");
    }

    #[test]
    fn test_plain_appends_metadata_when_the_entry_carries_it() {
        let formatter = PlainFormatter::new();
        let out = formatter.format(&with_meta("https://example.com"), true);
        assert!(out.starts_with("https://example.com "));
        assert!(out.contains("first_seen=20050101000000"));
        assert!(out.contains("last_seen=20240101000000"));
        assert!(out.contains("mime=text/html"));
        assert!(out.contains("archive_status=200"));
        assert!(out.contains("digest=ABCDEF"));
        assert!(out.ends_with('\n'));
    }
}
