use crate::utils::url::wordlist_terms;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use super::{Format, UrlData};

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

impl Format {
    /// Render the whole result set. Both destinations go through this, so the
    /// bytes a pipe sees and the bytes a file gets can never drift apart.
    ///
    /// CSV decides its column layout once for the whole run, so the header and
    /// every row emit exactly the same columns (otherwise rows could carry a
    /// trailing/extra comma the header doesn't, breaking strict CSV parsers).
    /// A wordlist is a set: `/admin/users` and `/admin/roles` contribute
    /// `admin` once, and a `BTreeSet` gives the dedup and the sort in one pass.
    fn render(self, urls: &[UrlData], out: &mut dyn Write) -> std::io::Result<()> {
        match self {
            Format::Csv => {
                let layout = super::formatter::CsvLayout::for_rows(urls);
                out.write_all(super::formatter::csv_header(&layout).as_bytes())?;
                for url_data in urls {
                    out.write_all(super::formatter::csv_row(url_data, &layout).as_bytes())?;
                }
            }
            Format::Wordlist => {
                let terms: BTreeSet<String> =
                    urls.iter().flat_map(|u| wordlist_terms(&u.url)).collect();
                for term in &terms {
                    out.write_all(term.as_bytes())?;
                    out.write_all(b"\n")?;
                }
            }
            _ => {
                if self == Format::Json {
                    out.write_all(b"[")?;
                }
                for (i, url_data) in urls.iter().enumerate() {
                    out.write_all(self.format(url_data, i == urls.len() - 1).as_bytes())?;
                }
                if self == Format::Json {
                    out.write_all(b"]")?;
                }
            }
        }
        Ok(())
    }

    /// Write `urls` to `output_path`, or to stdout unless `silent`.
    pub fn output(
        self,
        urls: &[UrlData],
        output_path: Option<PathBuf>,
        silent: bool,
    ) -> Result<()> {
        match output_path {
            Some(path) => {
                // Writing to a file: suppress ANSI colour. `console` decides on
                // colour globally from stdout's TTY status, so without
                // this a run in an interactive terminal would bake escape codes
                // into the file. Capture the current effective decision and
                // restore *that* afterward (not blanket auto-detection), so a
                // later stdout write keeps its colour — and a forced --no-color /
                // NO_COLOR run stays colourless instead of being re-enabled.
                let prev_colorize = console::colors_enabled();
                console::set_colors_enabled(false);
                let result = File::create(&path)
                    .context("Failed to create output file")
                    .and_then(|mut file| {
                        self.render(urls, &mut file)
                            .context("Failed to write to output file")
                    });
                console::set_colors_enabled(prev_colorize);
                result
            }
            None if silent => Ok(()),
            // JSON on stdout gets a trailing newline so an interactive run
            // ends on its own line.
            None => write_stdout(|out| {
                self.render(urls, out)?;
                if self == Format::Json {
                    out.write_all(b"\n")?;
                }
                Ok(())
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::NamedTempFile;

    #[test]
    fn test_plain_outputter_format() {
        let outputter = Format::Plain;
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
        let outputter = Format::Json;
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
        let outputter = Format::Csv;
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
        let outputter = Format::Csv;
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
        let outputter = Format::Plain;
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
        let outputter = Format::Json;
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
        let outputter = Format::Csv;
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
        let outputter = Format::Csv;
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
        let outputter = Format::Csv;
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
        let outputter = Format::Plain;
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
        // Unlike Format::Json, no entry depends on being last — that is what
        // lets the same formatter serve the streaming path.
        let outputter = Format::Jsonl;
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
        let outputter = Format::Jsonl;
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
            rendered(|out| Format::Plain.render(&urls, out)),
            "https://example.com/page1\nhttps://example.com/page2 [200 OK]\n"
        );
        assert_eq!(
            rendered(|out| Format::Json.render(&urls, out)),
            "[{\"url\":\"https://example.com/page1\"},\
             {\"url\":\"https://example.com/page2\",\"status\":\"200 OK\"}\n]"
        );
        assert_eq!(
            rendered(|out| Format::Jsonl.render(&urls, out)),
            "{\"url\":\"https://example.com/page1\"}\n\
             {\"url\":\"https://example.com/page2\",\"status\":\"200 OK\"}\n"
        );
        assert_eq!(
            rendered(|out| Format::Csv.render(&urls, out)),
            "url,status\nhttps://example.com/page1,\nhttps://example.com/page2,200 OK\n"
        );
    }

    #[test]
    fn test_render_of_an_empty_result_set() {
        let none: Vec<UrlData> = Vec::new();
        // json stays a valid (empty) array; csv still declares its columns.
        assert_eq!(rendered(|out| Format::Json.render(&none, out)), "[]");
        assert_eq!(rendered(|out| Format::Csv.render(&none, out)), "url\n");
        assert_eq!(rendered(|out| Format::Plain.render(&none, out)), "");
        assert_eq!(rendered(|out| Format::Jsonl.render(&none, out)), "");
    }

    #[test]
    fn test_render_with_sources() {
        let urls = vec![UrlData::new("https://example.com/a".to_string())
            .with_sources(vec!["wayback".into(), "cc".into()])];

        assert_eq!(
            rendered(|out| Format::Csv.render(&urls, out)),
            "url,sources\nhttps://example.com/a,cc|wayback\n"
        );
        assert_eq!(
            rendered(|out| Format::Jsonl.render(&urls, out)),
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
            Format::parse("plain"),
            Format::parse("json"),
            Format::parse("jsonl"),
            Format::parse("csv"),
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
        let outputter = Format::Jsonl;
        let urls = vec![UrlData::new("https://example.com".to_string())];
        // Silent + stdout must be a no-op rather than an error.
        outputter.output(&urls, None, true)?;
        Ok(())
    }

    fn wordlist_of(urls: &[&str]) -> Result<String> {
        let outputter = Format::parse("wordlist");
        let entries: Vec<UrlData> = urls
            .iter()
            .map(|u| UrlData::new((*u).to_string()))
            .collect();

        let temp_file = NamedTempFile::new()?;
        let temp_path = temp_file.path().to_path_buf();
        outputter.output(&entries, Some(temp_path.clone()), false)?;

        let mut content = String::new();
        File::open(&temp_path)?.read_to_string(&mut content)?;
        Ok(content)
    }

    #[test]
    fn test_wordlist_outputter_unions_terms_across_the_whole_run() -> Result<()> {
        // `admin` is contributed by both URLs and appears once: a wordlist is a
        // set, which is what the per-entry formatter cannot decide on its own.
        assert_eq!(
            wordlist_of(&[
                "https://example.com/admin/users?id=1",
                "https://example.com/admin/roles?sort=asc",
            ])?,
            "admin\nid\nroles\nsort\nusers\n"
        );
        Ok(())
    }

    #[test]
    fn test_wordlist_outputter_drops_data_and_keeps_route_names() -> Result<()> {
        assert_eq!(
            wordlist_of(&[
                "https://example.com/api/v2/users/550e8400-e29b-41d4-a716-446655440000",
                "https://example.com/post/4711/comments",
                "https://example.com/2024-01-02/report",
            ])?,
            "api\ncomments\npost\nreport\nusers\nv2\n"
        );
        Ok(())
    }

    #[test]
    fn test_wordlist_outputter_preserves_case() -> Result<()> {
        // Path segments are case-sensitive on most origins, so both spellings
        // are real words worth trying.
        assert_eq!(
            wordlist_of(&["https://example.com/Admin", "https://example.com/admin"])?,
            "Admin\nadmin\n"
        );
        Ok(())
    }

    #[test]
    fn test_wordlist_outputter_writes_nothing_for_bare_hosts() -> Result<()> {
        assert_eq!(wordlist_of(&["https://example.com/"])?, "");
        Ok(())
    }

    #[test]
    fn test_wordlist_outputter_silent_writes_nothing() -> Result<()> {
        let outputter = Format::Wordlist;
        let urls = vec![UrlData::new("https://example.com/admin".to_string())];
        outputter.output(&urls, None, true)?;
        Ok(())
    }
}
