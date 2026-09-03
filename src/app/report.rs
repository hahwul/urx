//! Everything urx draws for the operator rather than for a downstream pipe.
//!
//! The split matters: URL results go to stdout so they can be piped, while the
//! run header and the stats table go to stderr (or to the transient progress
//! region) so they never contaminate that stream.

use std::collections::BTreeMap;
use std::path::Path;

use crate::cli::Args;
use crate::output::{self, UrlData};
use crate::runner::ProviderStats;

/// Force-disable colour when `--no-color` or the `NO_COLOR` env var is set, for
/// both the progress UI (`console`, used by indicatif) and the URL output
/// (`colored`). With neither set, both keep their own TTY auto-detection.
/// `NO_COLOR` disables on mere presence (any value, including empty), matching
/// how `console` itself detects it (`env::var("NO_COLOR").is_ok()`), so both
/// surfaces stay consistent.
pub fn configure_colors(args: &Args) {
    if args.no_color || std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
}

/// Build the standalone run header drawn above the live progress region: a bold
/// teal `urx` wordmark riding the bars' 2-space gutter, the scan context in the
/// section-label tone, then a dimmed teal rule trailing out to a fixed width. No
/// box corners — it reads as a rule, never an unclosed frame (the header is
/// transient and cleared when the scan ends). Padding is measured from the plain
/// text so colour codes never enter the width math; `colored` strips the hues
/// automatically when colour is off.
pub fn render_header(n_domains: usize, n_providers: usize) -> String {
    use colored::Colorize;
    const RAIL_W: usize = 58;
    let dword = if n_domains == 1 { "domain" } else { "domains" };
    let pword = if n_providers == 1 {
        "provider"
    } else {
        "providers"
    };
    let rest = format!(" · scanning {n_domains} {dword} · {n_providers} {pword} ");
    // Visible cells before the rule (plain): "  "(2) + "urx"(3) + rest.
    let used = 2 + 3 + rest.chars().count();
    let pad = RAIL_W.saturating_sub(used).max(3);
    format!(
        "{}{}{}{}",
        "  ",
        "urx".truecolor(0x5a, 0xd1, 0xcd).bold(),
        rest.truecolor(0xa7, 0xb6, 0xc2),
        "─".repeat(pad).truecolor(0x5a, 0xd1, 0xcd).dimmed(),
    )
}

/// Render the per-provider summary table to stderr (so it doesn't pollute
/// stdout when callers pipe URL results into other tools).
pub fn print_provider_stats(stats: &[ProviderStats]) {
    if stats.is_empty() {
        return;
    }
    eprintln!("{}", render_provider_stats(stats));
}

/// The per-provider summary table, as text.
fn render_provider_stats(stats: &[ProviderStats]) -> String {
    // The name column grows to the longest label rather than staying fixed:
    // "Robots.txt (archived)" overflowed the old 18-column slot and pushed
    // its numbers out of line with every other row.
    let width = stats
        .iter()
        .map(|s| s.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(18);
    let mut out = String::from("\nProvider stats:\n");
    out.push_str(&format!(
        "  {:<width$}  {:>8}  {:>8}  {:>7}  {:>10}\n",
        "provider", "urls", "partial", "errors", "elapsed"
    ));
    out.push_str(&format!(
        "  {:<width$}  {:>8}  {:>8}  {:>7}  {:>10}\n",
        "-".repeat(width),
        "--------",
        "--------",
        "-------",
        "----------"
    ));
    for s in stats {
        // A provider cut off by --max-time / Ctrl-C is called out: every count
        // on its row is a floor, not a total, and without the marker a run that
        // was stopped mid-fetch reads exactly like one that finished.
        out.push_str(&format!(
            "  {:<width$}  {:>8}  {:>8}  {:>7}  {:>10}{}\n",
            s.name,
            s.url_count,
            s.partial_count,
            s.error_count,
            format_elapsed(s.elapsed),
            if s.aborted { "  (aborted)" } else { "" }
        ));
    }
    out
}

/// Sub-second durations read better in milliseconds; anything longer in
/// seconds.
fn format_elapsed(elapsed: std::time::Duration) -> String {
    let ms = elapsed.as_millis();
    if ms >= 1000 {
        format!("{:.2}s", elapsed.as_secs_f64())
    } else {
        format!("{ms}ms")
    }
}

/// Best-effort filename extension matching `--format`. Anything other than
/// json/jsonl/csv falls back to `.txt`, mirroring how `create_outputter` treats
/// unknown formats as plain text.
pub fn output_dir_extension(format: &str) -> &'static str {
    match format.to_lowercase().as_str() {
        "json" => "json",
        "jsonl" => "jsonl",
        "csv" => "csv",
        _ => "txt",
    }
}

/// Group URLs by their host and write one file per domain into `dir`.
/// URLs that fail to parse a host (rare after filtering) land in
/// `_unknown.<ext>` so nothing is silently dropped.
pub fn write_per_domain_output(
    urls: &[UrlData],
    dir: &Path,
    format: &str,
    silent: bool,
) -> anyhow::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }

    let mut grouped: BTreeMap<String, Vec<UrlData>> = BTreeMap::new();
    for entry in urls {
        let host = url::Url::parse(&entry.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "_unknown".to_string());
        grouped.entry(host).or_default().push(entry.clone());
    }

    let outputter = output::create_outputter(format);
    let ext = output_dir_extension(format);

    for (host, entries) in &grouped {
        outputter.output(entries, Some(dir.join(format!("{host}.{ext}"))), silent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::plain;
    use std::time::Duration;

    #[test]
    fn test_render_header_line() {
        let p = plain(&render_header(3, 5));
        // Standalone rule header: 2-space gutter, bold `urx` wordmark, scan
        // context, then a trailing rule out to a fixed 58 columns. No box.
        assert!(p.starts_with("  urx · scanning 3 domains · 5 providers "));
        assert!(p.ends_with('─'));
        assert!(!p.starts_with('╭') && !p.ends_with('╮'));
        assert_eq!(p.chars().count(), 58);
        // Singular forms.
        let one = plain(&render_header(1, 1));
        assert!(one.contains("scanning 1 domain · 1 provider "));
    }

    #[test]
    fn test_render_header_keeps_a_minimum_rule_when_context_is_long() {
        // A huge count would otherwise push the rule to zero width and make the
        // header read as truncated.
        let p = plain(&render_header(usize::MAX, usize::MAX));
        assert!(p.ends_with("───"), "{p}");
    }

    #[test]
    fn test_format_elapsed_switches_unit_at_one_second() {
        assert_eq!(format_elapsed(Duration::from_millis(0)), "0ms");
        assert_eq!(format_elapsed(Duration::from_millis(999)), "999ms");
        assert_eq!(format_elapsed(Duration::from_millis(1000)), "1.00s");
        assert_eq!(format_elapsed(Duration::from_millis(1500)), "1.50s");
    }

    #[test]
    fn test_stats_table_flags_a_provider_that_was_cut_off() {
        // Regression: a provider aborted by --max-time was rendered exactly
        // like one that finished — same columns, no marker — so a truncated run
        // read as a complete one.
        let rows = [
            ProviderStats {
                name: "Wayback Machine".to_string(),
                url_count: 1_200,
                partial_count: 1,
                error_count: 0,
                elapsed: Duration::from_secs(90),
                aborted: true,
            },
            ProviderStats {
                name: "OTX".to_string(),
                url_count: 3,
                partial_count: 0,
                error_count: 0,
                elapsed: Duration::from_millis(120),
                aborted: false,
            },
        ];

        let table = render_provider_stats(&rows);
        let wayback = table
            .lines()
            .find(|l| l.contains("Wayback Machine"))
            .unwrap();
        assert!(wayback.contains("90.00s"), "{wayback}");
        assert!(wayback.ends_with("(aborted)"), "{wayback}");

        // A provider that ran to completion carries no marker.
        let otx = table.lines().find(|l| l.contains("OTX")).unwrap();
        assert!(!otx.contains("aborted"), "{otx}");
    }

    #[test]
    fn test_output_dir_extension() {
        assert_eq!(output_dir_extension("json"), "json");
        assert_eq!(output_dir_extension("JSON"), "json");
        assert_eq!(output_dir_extension("jsonl"), "jsonl");
        assert_eq!(output_dir_extension("csv"), "csv");
        assert_eq!(output_dir_extension("plain"), "txt");
        assert_eq!(output_dir_extension("anything-else"), "txt");
    }

    #[test]
    fn test_write_per_domain_output_groups_by_host() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let urls = vec![
            UrlData::new("https://example.com/a".to_string()),
            UrlData::new("https://example.com/b".to_string()),
            UrlData::new("https://other.test/x".to_string()),
            UrlData::new("not-a-url".to_string()),
        ];

        write_per_domain_output(&urls, dir.path(), "plain", true)?;

        let example = std::fs::read_to_string(dir.path().join("example.com.txt"))?;
        assert!(example.contains("https://example.com/a"));
        assert!(example.contains("https://example.com/b"));

        let other = std::fs::read_to_string(dir.path().join("other.test.txt"))?;
        assert!(other.contains("https://other.test/x"));

        // Unparseable URLs land in _unknown.txt instead of being dropped.
        let unknown = std::fs::read_to_string(dir.path().join("_unknown.txt"))?;
        assert!(unknown.contains("not-a-url"));
        Ok(())
    }

    #[test]
    fn test_write_per_domain_output_creates_missing_dir() -> anyhow::Result<()> {
        let base = tempfile::tempdir()?;
        let nested = base.path().join("nested/output/dir");
        let urls = vec![UrlData::new("https://example.com/a".to_string())];

        write_per_domain_output(&urls, &nested, "json", true)?;

        assert!(nested.is_dir());
        let example = std::fs::read_to_string(nested.join("example.com.json"))?;
        assert!(example.starts_with('['));
        assert!(example.contains("https://example.com/a"));
        Ok(())
    }
}
