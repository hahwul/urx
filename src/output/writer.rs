use crate::output::Formatter;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

// Outputter implementations for different formats
use super::{Outputter, UrlData};

/// Write to stdout, treating a closed pipe as a normal end of output.
///
/// `print!`/`println!` *panic* when stdout is gone, so `urx example.com | head -1`
/// used to end in `thread 'main' panicked ... failed printing to stdout: Broken
/// pipe` instead of the silent stop every other CLI gives. Taking the lock once
/// also avoids re-locking stdout for every URL.
fn write_stdout(f: impl FnOnce(&mut dyn Write) -> std::io::Result<()>) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    finish_stdout_write(f(&mut handle).and_then(|()| handle.flush()))
}

/// Decide what a finished stdout write means.
///
/// A closed pipe is not a failure: the reader (`| head`, `| grep -q`, a shut
/// terminal) already has what it asked for, so the run stops delivering output
/// and reports success. Any other I/O error is real and is surfaced.
fn finish_stdout_write(result: std::io::Result<()>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(anyhow::Error::new(e).context("Failed to write to stdout")),
    }
}

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

impl PlainOutputter {
    /// Render the whole result set. Both destinations go through this, so the
    /// bytes a pipe sees and the bytes a file gets can never drift apart.
    fn render(&self, urls: &[UrlData], out: &mut dyn Write) -> std::io::Result<()> {
        for (i, url_data) in urls.iter().enumerate() {
            let formatted = self.format(url_data, i == urls.len() - 1);
            out.write_all(formatted.as_bytes())?;
        }
        Ok(())
    }
}

impl Outputter for PlainOutputter {
    fn format(&self, url_data: &UrlData, is_last: bool) -> String {
        self.formatter.format(url_data, is_last)
    }

    fn output(&self, urls: &[UrlData], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                // Writing to a file: suppress ANSI colour. The `colored` crate
                // decides on color globally from stdout's TTY status, so without
                // this a run in an interactive terminal would bake escape codes
                // into the file. Capture the current effective decision and
                // restore *that* afterward (not blanket auto-detection), so a
                // later stdout write keeps its colour — and a forced --no-color /
                // NO_COLOR run stays colourless instead of being re-enabled.
                let prev_colorize = colored::control::SHOULD_COLORIZE.should_colorize();
                colored::control::set_override(false);
                let result = File::create(&path)
                    .context("Failed to create output file")
                    .and_then(|mut file| {
                        self.render(urls, &mut file)
                            .context("Failed to write to output file")
                    });
                colored::control::set_override(prev_colorize);
                result
            }
            None => {
                if silent {
                    return Ok(());
                };

                write_stdout(|out| self.render(urls, out))
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

impl JsonOutputter {
    /// Render the array. Shared by both destinations; stdout adds a trailing
    /// newline afterwards so an interactive run ends on its own line.
    fn render(&self, urls: &[UrlData], out: &mut dyn Write) -> std::io::Result<()> {
        out.write_all(b"[")?;
        for (i, url_data) in urls.iter().enumerate() {
            let formatted = self.format(url_data, i == urls.len() - 1);
            out.write_all(formatted.as_bytes())?;
        }
        out.write_all(b"]")
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
                self.render(urls, &mut file)
                    .context("Failed to write to output file")
            }
            None => {
                if silent {
                    return Ok(());
                };

                write_stdout(|out| {
                    self.render(urls, out)?;
                    out.write_all(b"\n")
                })
            }
        }
    }
}

/// Writes one JSON object per line, with no array wrapper. Every line stands
/// alone, so the file remains parseable while it is still being written.
#[derive(Debug, Clone)]
pub struct JsonLinesOutputter {
    formatter: Box<dyn Formatter>,
}

impl JsonLinesOutputter {
    pub fn new() -> Self {
        JsonLinesOutputter {
            formatter: Box::new(super::JsonLinesFormatter::new()),
        }
    }
}

impl Default for JsonLinesOutputter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonLinesOutputter {
    /// Render one standalone JSON object per line, for either destination.
    fn render(&self, urls: &[UrlData], out: &mut dyn Write) -> std::io::Result<()> {
        for url_data in urls {
            out.write_all(self.format(url_data, false).as_bytes())?;
        }
        Ok(())
    }
}

impl Outputter for JsonLinesOutputter {
    fn format(&self, url_data: &UrlData, is_last: bool) -> String {
        self.formatter.format(url_data, is_last)
    }

    fn output(&self, urls: &[UrlData], output_path: Option<PathBuf>, silent: bool) -> Result<()> {
        match output_path {
            Some(path) => {
                let mut file = File::create(&path).context("Failed to create output file")?;
                self.render(urls, &mut file)
                    .context("Failed to write to output file")
            }
            None => {
                if silent {
                    return Ok(());
                };
                write_stdout(|out| self.render(urls, out))
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

impl CsvOutputter {
    /// Render header plus rows for either destination.
    ///
    /// The column layout is decided once for the whole run so the header and
    /// every row emit exactly the same columns (otherwise rows could carry a
    /// trailing/extra comma the header doesn't, breaking strict CSV parsers) —
    /// which is also why both destinations must share this one function.
    fn render(&self, urls: &[UrlData], out: &mut dyn Write) -> std::io::Result<()> {
        let has_status = urls.iter().any(|url| url.status.is_some());
        let has_sources = urls.iter().any(|url| !url.sources.is_empty());

        out.write_all(super::formatter::csv_header(has_status, has_sources).as_bytes())?;
        for url_data in urls {
            let formatted = super::formatter::csv_row(url_data, has_status, has_sources);
            out.write_all(formatted.as_bytes())?;
        }
        Ok(())
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
                self.render(urls, &mut file)
                    .context("Failed to write to output file")
            }
            None => {
                if silent {
                    return Ok(());
                };

                write_stdout(|out| self.render(urls, out))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::create_outputter;
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
        assert_eq!(outputter.format(&url_data, false), "https://example.com\n");

        let url_data_status =
            UrlData::with_status("https://example.com".to_string(), "200 OK".to_string());
        assert_eq!(
            outputter.format(&url_data_status, true),
            "https://example.com,200 OK\n"
        );
    }

    #[test]
    fn test_csv_outputter_no_status_no_sources_single_column() -> Result<()> {
        // Regression: a url-only run must produce a single `url` column for both
        // header and every row (no dangling trailing comma).
        let outputter = CsvOutputter::new();
        let urls = vec![
            UrlData::new("https://example.com/a".to_string()),
            UrlData::new("https://example.com/b".to_string()),
        ];
        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();
        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        File::open(&temp_path)?.read_to_string(&mut content)?;
        assert_eq!(
            content,
            "url\nhttps://example.com/a\nhttps://example.com/b\n"
        );
        Ok(())
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
    fn test_csv_outputter_with_sources_header() -> Result<()> {
        let outputter = CsvOutputter::new();
        let urls = vec![
            UrlData::new("https://example.com/a".to_string()).with_sources(vec!["wayback".into()]),
            UrlData::with_status("https://example.com/b".to_string(), "200 OK".to_string())
                .with_sources(vec!["cc".into(), "otx".into()]),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();
        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        let mut file = File::open(&temp_path)?;
        file.read_to_string(&mut content)?;

        assert_eq!(
            content,
            "url,status,sources\nhttps://example.com/a,,wayback\nhttps://example.com/b,200 OK,cc|otx\n"
        );
        Ok(())
    }

    #[test]
    fn test_csv_output_neutralises_a_formula_field_end_to_end() -> Result<()> {
        // `urx ... --show-only-param -f csv` writes a raw query-parameter name
        // into the url column; one starting with `=` is a live DDE formula in
        // Excel. Reproduced from real output: `=cmd|'/C calc'!A0=1&normal=2`.
        let outputter = CsvOutputter::new();
        let urls = vec![
            UrlData::new("=cmd|'/C calc'!A0=1".to_string()),
            UrlData::new("https://example.com/ok".to_string()),
        ];

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();
        outputter.output(&urls, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        File::open(&temp_path)?.read_to_string(&mut content)?;
        assert_eq!(
            content,
            "url\n\"'=cmd|'/C calc'!A0=1\"\nhttps://example.com/ok\n"
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

    #[test]
    fn test_jsonl_outputter_format_is_position_independent() {
        // Unlike JsonOutputter, no entry depends on being last — that is what
        // lets the same formatter serve the streaming path.
        let outputter = JsonLinesOutputter::new();
        let url_data = UrlData::new("https://example.com".to_string());
        assert_eq!(
            outputter.format(&url_data, false),
            "{\"url\":\"https://example.com\"}\n"
        );
        assert_eq!(
            outputter.format(&url_data, true),
            outputter.format(&url_data, false)
        );
    }

    #[test]
    fn test_jsonl_outputter_file_output() -> Result<()> {
        let outputter = JsonLinesOutputter::new();
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

        // No array wrapper and no separating commas: every line parses alone.
        assert_eq!(
            content,
            "{\"url\":\"https://example.com/page1\"}\n\
             {\"url\":\"https://example.com/page2\",\"status\":\"200 OK\"}\n"
        );
        for line in content.lines() {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }

        Ok(())
    }

    fn rendered(f: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
        let mut buf = Vec::new();
        f(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn sample() -> Vec<UrlData> {
        vec![
            UrlData::new("https://example.com/page1".to_string()),
            UrlData::with_status(
                "https://example.com/page2".to_string(),
                "200 OK".to_string(),
            ),
        ]
    }

    #[test]
    fn test_render_produces_the_same_bytes_for_every_destination() {
        // The stdout and file branches used to be twin copies of the same loop,
        // so a fix to one could silently miss the other. They now share
        // `render`; these assert what both destinations therefore emit.
        let urls = sample();

        assert_eq!(
            rendered(|out| PlainOutputter::new().render(&urls, out)),
            "https://example.com/page1\nhttps://example.com/page2 [200 OK]\n"
        );
        assert_eq!(
            rendered(|out| JsonOutputter::new().render(&urls, out)),
            "[{\"url\":\"https://example.com/page1\"},\
             {\"url\":\"https://example.com/page2\",\"status\":\"200 OK\"}\n]"
        );
        assert_eq!(
            rendered(|out| JsonLinesOutputter::new().render(&urls, out)),
            "{\"url\":\"https://example.com/page1\"}\n\
             {\"url\":\"https://example.com/page2\",\"status\":\"200 OK\"}\n"
        );
        assert_eq!(
            rendered(|out| CsvOutputter::new().render(&urls, out)),
            "url,status\nhttps://example.com/page1,\nhttps://example.com/page2,200 OK\n"
        );
    }

    #[test]
    fn test_render_of_an_empty_result_set() {
        let none: Vec<UrlData> = Vec::new();
        // json stays a valid (empty) array; csv still declares its columns.
        assert_eq!(
            rendered(|out| JsonOutputter::new().render(&none, out)),
            "[]"
        );
        assert_eq!(
            rendered(|out| CsvOutputter::new().render(&none, out)),
            "url\n"
        );
        assert_eq!(rendered(|out| PlainOutputter::new().render(&none, out)), "");
        assert_eq!(
            rendered(|out| JsonLinesOutputter::new().render(&none, out)),
            ""
        );
    }

    #[test]
    fn test_render_with_sources() {
        let urls = vec![UrlData::new("https://example.com/a".to_string())
            .with_sources(vec!["wayback".into(), "cc".into()])];

        assert_eq!(
            rendered(|out| CsvOutputter::new().render(&urls, out)),
            "url,sources\nhttps://example.com/a,cc|wayback\n"
        );
        assert_eq!(
            rendered(|out| JsonLinesOutputter::new().render(&urls, out)),
            "{\"url\":\"https://example.com/a\",\"sources\":[\"cc\",\"wayback\"]}\n"
        );
    }

    #[test]
    fn test_stdout_destination_writes_every_format() {
        // Exercises the stdout branch of all four outputters end to end — the
        // path that used to panic on a closed pipe. The bytes are pinned by the
        // render tests above; what matters here is that the real destination is
        // driven without panicking and reports success. One URL only: these
        // writes go straight to fd 1 and so escape the harness's per-test
        // capture, and this keeps the stray lines to a minimum.
        let urls = vec![UrlData::new("https://example.com/a".to_string())];
        for outputter in [
            create_outputter("plain"),
            create_outputter("json"),
            create_outputter("jsonl"),
            create_outputter("csv"),
        ] {
            outputter.output(&urls, None, false).unwrap();
            // --silent short-circuits before touching stdout at all.
            outputter.output(&urls, None, true).unwrap();
        }
    }

    #[test]
    fn test_broken_pipe_on_stdout_is_a_clean_stop_not_a_failure() {
        // Regression: every stdout path used `print!`, which *panics* when the
        // reader is gone, so `urx example.com | head -1` ended in
        // "thread 'main' panicked ... failed printing to stdout: Broken pipe".
        // The outputters now funnel through this policy instead.
        let closed = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Broken pipe");
        assert!(finish_stdout_write(Err(closed)).is_ok());

        // A genuine write failure is still reported.
        let disk_full = std::io::Error::new(std::io::ErrorKind::StorageFull, "No space left");
        let err = finish_stdout_write(Err(disk_full)).unwrap_err();
        assert!(
            err.to_string().contains("Failed to write to stdout"),
            "{err}"
        );

        assert!(finish_stdout_write(Ok(())).is_ok());
    }

    #[test]
    fn test_jsonl_outputter_silent_writes_nothing() -> Result<()> {
        let outputter = JsonLinesOutputter::new();
        let urls = vec![UrlData::new("https://example.com".to_string())];
        // Silent + stdout must be a no-op rather than an error.
        outputter.output(&urls, None, true)?;
        Ok(())
    }
}
