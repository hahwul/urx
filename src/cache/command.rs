//! `urx cache …` — the operator-facing view of the URL cache.
//!
//! Before this existed the only way to inspect the cache was to open the SQLite
//! file by hand, and the only way to invalidate one domain was to delete the
//! whole database. Every subcommand here honours the same `--cache-type`,
//! `--cache-path`, `--redis-url` and `--cache-ttl` the scan path uses, so what
//! it reports is what a scan would actually see.
//!
//! Rendering is kept in pure `render_*` functions returning `String`: the
//! numbers are the whole point of the feature, so they are asserted in tests
//! rather than only eyeballed.

use std::collections::BTreeMap;
use std::io::IsTerminal;

use anyhow::{Context, Result};
use serde::Serialize;

use super::admin::{
    domain_matches, is_expired, summarize_domains, summarize_stats, CacheStats, DomainSummary,
};
use crate::cli::Args;

/// Top-level subcommands.
///
/// The field holding this on [`Args`] is an `Option`, so the historical
/// `urx [OPTIONS] [DOMAINS]...` invocation — including `cat domains.txt | urx`
/// — parses exactly as it always did. Only the literal first token `cache`
/// selects a subcommand.
#[derive(clap::Subcommand, Debug, Clone)]
pub enum Command {
    /// Inspect and maintain the URL cache
    Cache {
        #[clap(subcommand)]
        action: CacheAction,
    },
}

/// Domain-pattern syntax, shared by `list --domain` and `drop`.
///
/// Spelled out in `--help` because the exact-by-default rule is the opposite of
/// what a `grep`-shaped expectation would assume, and `drop` is destructive.
const PATTERN_HELP: &str = "Domain pattern. Matching is case-insensitive and exact unless the \
pattern contains `*`, which stands for any run of characters: `*.example.com` matches subdomains \
only, `example.*` matches any TLD, `*example*` matches any domain containing the text.";

#[derive(clap::Subcommand, Debug, Clone)]
pub enum CacheAction {
    /// Summarize the cache: entries, domains, URLs, age span, size, expired count
    Stats,

    /// List cached domains with their URL counts, last scan time and TTL left
    List {
        /// Only list domains matching this pattern
        #[clap(long, value_name = "PAT", long_help = PATTERN_HELP)]
        domain: Option<String>,
    },

    /// Delete entries older than --cache-ttl, leaving everything still fresh
    Prune,

    /// Delete every entry for the given domains
    Drop {
        /// Domains (or `*` patterns) whose entries should be deleted
        #[clap(value_name = "DOMAIN", required = true, long_help = PATTERN_HELP)]
        domains: Vec<String>,
    },

    /// Delete every entry in the cache
    Clear {
        /// Skip the confirmation prompt
        #[clap(long, short = 'y')]
        yes: bool,
    },
}

/// `-f json` / `-f jsonl` switch every subcommand to machine-readable output.
/// `csv` has no sensible shape for a stats blob, so it — like `plain` and
/// anything unrecognised — gets the text report.
fn wants_json(format: &str) -> bool {
    matches!(format.to_lowercase().as_str(), "json" | "jsonl")
}

/// Run one `urx cache` subcommand and print its report to stdout.
pub async fn run(args: &Args, action: &CacheAction) -> Result<()> {
    let admin = super::open_admin(args).await?;
    let json = wants_json(&args.format);
    let ttl = args.cache_ttl;

    match action {
        CacheAction::Stats => {
            let entries = admin.entries().await?;
            let stats = summarize_stats(
                &entries,
                ttl,
                admin.backend_name(),
                admin.location(),
                admin.size_bytes().await?,
            );
            if json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                print!("{}", render_stats(&stats));
            }
        }

        CacheAction::List { domain } => {
            let entries = admin.entries().await?;
            let rows = summarize_domains(&entries, ttl, domain.as_deref());
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                print!("{}", render_list(&rows, domain.as_deref()));
            }
        }

        CacheAction::Prune => {
            let before = admin.entries().await?;
            let expected = before
                .iter()
                .filter(|e| is_expired(e.timestamp, ttl))
                .count();
            let pruned = admin.delete_expired(ttl).await?;
            let report = PruneReport {
                pruned,
                remaining: before.len().saturating_sub(pruned),
                ttl_seconds: ttl,
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render_prune(&report, expected));
            }
        }

        CacheAction::Drop { domains } => {
            let entries = admin.entries().await?;

            // Resolved before the delete so the report can name the patterns
            // that matched nothing — a typo'd domain otherwise looks exactly
            // like a successful no-op.
            let mut hits: BTreeMap<String, usize> = BTreeMap::new();
            let mut unmatched = Vec::new();
            for pattern in domains {
                let mut matched = false;
                for entry in &entries {
                    if domain_matches(pattern, &entry.domain) {
                        *hits.entry(entry.domain.clone()).or_default() += 1;
                        matched = true;
                    }
                }
                if !matched {
                    unmatched.push(pattern.clone());
                }
            }

            let dropped = admin.delete_domains(domains).await?;
            let report = DropReport {
                dropped,
                domains: hits
                    .into_iter()
                    .map(|(domain, entries)| DroppedDomain { domain, entries })
                    .collect(),
                unmatched,
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render_drop(&report));
            }
        }

        CacheAction::Clear { yes } => {
            let entries = admin.entries().await?;
            if !yes && !confirm_clear(entries.len(), &admin.location())? {
                // Not an error: the user was asked and said no.
                println!("Aborted; nothing was deleted.");
                return Ok(());
            }
            let cleared = admin.clear().await?;
            let report = ClearReport { cleared };
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render_clear(&report));
            }
        }
    }

    Ok(())
}

/// Ask before deleting the whole cache.
///
/// A non-interactive stdin is refused rather than assumed either way: assuming
/// "yes" would make `urx cache clear` in a script destroy a cache the author
/// never confirmed, and assuming "no" would hang or silently do nothing. The
/// scripted spelling is `--yes`, and the error says so.
fn confirm_clear(entries: usize, location: &str) -> Result<bool> {
    use std::io::Write;

    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "`urx cache clear` deletes every cached entry and stdin is not a terminal, so it cannot ask for confirmation. Re-run with --yes to confirm."
        );
    }

    eprint!(
        "Delete all {entries} cached {} from {location}? [y/N] ",
        entries_noun(entries)
    );
    std::io::stderr().flush().ok();

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("Failed to read confirmation from stdin")?;

    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

#[derive(Debug, Clone, Serialize)]
struct PruneReport {
    pruned: usize,
    remaining: usize,
    ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
struct DroppedDomain {
    domain: String,
    entries: usize,
}

#[derive(Debug, Clone, Serialize)]
struct DropReport {
    dropped: usize,
    domains: Vec<DroppedDomain>,
    /// Patterns that matched no cached domain — almost always a typo.
    unmatched: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ClearReport {
    cleared: usize,
}

fn render_stats(s: &CacheStats) -> String {
    let mut out = String::new();
    out.push_str(&format!("Cache:    {}\n", s.backend));
    out.push_str(&format!("Location: {}\n", s.location));
    out.push_str(&format!(
        "Size:     {}{}\n",
        s.size_bytes.map_or("unknown".to_string(), format_bytes),
        match s.backend.as_str() {
            "sqlite" => " (database file)",
            "redis" => " (stored values)",
            _ => "",
        }
    ));

    if s.entries == 0 {
        out.push_str("\nThe cache is empty.\n");
        return out;
    }

    out.push('\n');
    out.push_str(&format!("Entries:  {}\n", thousands(s.entries)));
    out.push_str(&format!("Domains:  {}\n", thousands(s.domains)));
    out.push_str(&format!("URLs:     {}\n", thousands(s.urls)));
    out.push_str(&format!(
        "Expired:  {}  (--cache-ttl {}s = {})\n",
        thousands(s.expired_entries),
        s.ttl_seconds,
        format_secs(s.ttl_seconds.min(i64::MAX as u64) as i64),
    ));

    out.push('\n');
    if let Some(oldest) = s.oldest {
        out.push_str(&format!(
            "Oldest:   {}  ({} ago)\n",
            stamp(oldest),
            format_secs(age_secs(oldest))
        ));
    }
    if let Some(newest) = s.newest {
        out.push_str(&format!(
            "Newest:   {}  ({} ago)\n",
            stamp(newest),
            format_secs(age_secs(newest))
        ));
    }
    out
}

fn render_list(rows: &[DomainSummary], pattern: Option<&str>) -> String {
    if rows.is_empty() {
        return match pattern {
            Some(pat) => format!("No cached domain matches '{pat}'.\n"),
            None => "The cache is empty.\n".to_string(),
        };
    }

    let width = rows
        .iter()
        .map(|r| r.domain.chars().count())
        .max()
        .unwrap_or(6)
        .max(6);

    let mut out = format!(
        "{:<width$}  {:>7}  {:>7}  {:>9}  {:<20}  {}\n",
        "DOMAIN", "ENTRIES", "EXPIRED", "URLS", "LAST SCAN", "TTL LEFT"
    );
    out.push_str(&format!(
        "{:<width$}  {:>7}  {:>7}  {:>9}  {:<20}  {}\n",
        "-".repeat(width),
        "-------",
        "-------",
        "---------",
        "-".repeat(20),
        "--------"
    ));

    for r in rows {
        out.push_str(&format!(
            "{:<width$}  {:>7}  {:>7}  {:>9}  {:<20}  {}\n",
            r.domain,
            r.entries,
            r.expired_entries,
            thousands(r.urls),
            stamp(r.last_scan),
            r.ttl_remaining.map_or("expired".to_string(), format_secs),
        ));
    }
    out
}

fn render_prune(report: &PruneReport, expected: usize) -> String {
    if report.pruned == 0 {
        return format!(
            "Nothing to prune: no entry is older than --cache-ttl {}s.\n",
            report.ttl_seconds
        );
    }

    let mut out = format!(
        "Pruned {} expired {} (--cache-ttl {}s); {} remain.\n",
        thousands(report.pruned),
        entries_noun(report.pruned),
        report.ttl_seconds,
        thousands(report.remaining),
    );
    // The two counts are taken either side of the delete, so a disagreement
    // means something else wrote to the cache in between. Worth saying rather
    // than quietly reporting a number that doesn't match what was listed.
    if expected != report.pruned {
        out.push_str(&format!(
            "Note: {} entries looked expired before the sweep; the cache changed while it ran.\n",
            thousands(expected)
        ));
    }
    out
}

fn render_drop(report: &DropReport) -> String {
    let mut out = String::new();

    if report.dropped == 0 {
        out.push_str("No cached entry matched; nothing was deleted.\n");
    } else {
        let dword = if report.domains.len() == 1 {
            "domain"
        } else {
            "domains"
        };
        out.push_str(&format!(
            "Dropped {} {} across {} {dword}:\n",
            thousands(report.dropped),
            entries_noun(report.dropped),
            report.domains.len()
        ));
        let width = report
            .domains
            .iter()
            .map(|d| d.domain.chars().count())
            .max()
            .unwrap_or(0);
        for d in &report.domains {
            out.push_str(&format!(
                "  {:<width$}  {} {}\n",
                d.domain,
                d.entries,
                entries_noun(d.entries)
            ));
        }
    }

    if !report.unmatched.is_empty() {
        out.push_str(&format!(
            "No cached entry matched: {}\n",
            report.unmatched.join(", ")
        ));
    }
    out
}

fn render_clear(report: &ClearReport) -> String {
    if report.cleared == 0 {
        return "The cache was already empty.\n".to_string();
    }
    format!(
        "Cleared {} cached {}.\n",
        thousands(report.cleared),
        entries_noun(report.cleared)
    )
}

/// The counted-noun form these reports use throughout: "1 entry", "3 entries".
fn entries_noun(n: usize) -> &'static str {
    if n == 1 {
        "entry"
    } else {
        "entries"
    }
}

/// Seconds since `ts`, floored at zero so an entry dated in the future reads as
/// "0s ago" rather than a negative age.
fn age_secs(ts: chrono::DateTime<chrono::Utc>) -> i64 {
    chrono::Utc::now()
        .signed_duration_since(ts)
        .num_seconds()
        .max(0)
}

fn stamp(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Two significant units, which is as much precision as a cache age needs.
fn format_secs(secs: i64) -> String {
    if secs < 0 {
        return "0s".to_string();
    }
    let (d, h, m, s) = (
        secs / 86_400,
        (secs % 86_400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Group digits so a six-figure URL count is readable at a glance.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::admin::EntryMeta;
    use crate::test_support::plain;
    use chrono::Utc;

    fn entry(domain: &str, urls: usize, age_secs: i64) -> EntryMeta {
        EntryMeta {
            domain: domain.to_string(),
            url_count: urls,
            timestamp: Utc::now() - chrono::Duration::seconds(age_secs),
        }
    }

    /// The compatibility contract for introducing a subcommand at all: every
    /// pre-existing way of naming a target has to keep parsing identically.
    #[test]
    fn the_historical_invocation_is_unchanged() {
        use clap::Parser;

        let args = Args::parse_from(["urx", "example.com", "test.org"]);
        assert!(args.command.is_none());
        assert_eq!(args.domains, ["example.com", "test.org"]);

        // The `cat domains.txt | urx` shape: no argv at all, targets read from
        // stdin later. A required subcommand would have made this an error.
        let args = Args::parse_from(["urx"]);
        assert!(args.command.is_none());
        assert!(args.domains.is_empty());

        // Options on either side of the positionals still bind to them.
        let args = Args::parse_from(["urx", "--silent", "example.com", "-f", "json"]);
        assert!(args.command.is_none());
        assert_eq!(args.domains, ["example.com"]);
        assert_eq!(args.format, "json");

        // --files input names no domain and must not look like a subcommand.
        let args = Args::parse_from(["urx", "--files", "urls.txt"]);
        assert!(args.command.is_none());
    }

    /// A host literally named `cache` is the one collision introducing a
    /// subcommand creates. It is not silently unreachable: `--` still gets it
    /// to the scanner, as do stdin and `--domain-list`.
    #[test]
    fn a_host_named_cache_is_still_reachable() {
        use clap::Parser;

        let args = Args::parse_from(["urx", "--", "cache"]);
        assert!(args.command.is_none());
        assert_eq!(args.domains, ["cache"]);

        // Only the bare word collides; anything longer is untouched.
        let args = Args::parse_from(["urx", "cache.com"]);
        assert!(args.command.is_none());
        assert_eq!(args.domains, ["cache.com"]);

        // ...and only in first position.
        let args = Args::parse_from(["urx", "example.com", "cache"]);
        assert!(args.command.is_none());
        assert_eq!(args.domains, ["example.com", "cache"]);
    }

    #[test]
    fn every_cache_subcommand_parses() {
        use clap::Parser;

        let action = |argv: &[&str]| -> CacheAction {
            let mut full = vec!["urx"];
            full.extend_from_slice(argv);
            match Args::parse_from(full).command {
                Some(Command::Cache { action }) => action,
                other => panic!("{argv:?} did not select a cache subcommand: {other:?}"),
            }
        };

        assert!(matches!(action(&["cache", "stats"]), CacheAction::Stats));
        assert!(matches!(
            action(&["cache", "list"]),
            CacheAction::List { domain: None }
        ));
        assert!(matches!(
            action(&["cache", "list", "--domain", "*.example.com"]),
            CacheAction::List { domain: Some(d) } if d == "*.example.com"
        ));
        assert!(matches!(action(&["cache", "prune"]), CacheAction::Prune));
        assert!(matches!(
            action(&["cache", "drop", "a.test", "b.test"]),
            CacheAction::Drop { domains } if domains == ["a.test", "b.test"]
        ));
        assert!(matches!(
            action(&["cache", "clear"]),
            CacheAction::Clear { yes: false }
        ));
        assert!(matches!(
            action(&["cache", "clear", "--yes"]),
            CacheAction::Clear { yes: true }
        ));
        assert!(matches!(
            action(&["cache", "clear", "-y"]),
            CacheAction::Clear { yes: true }
        ));

        // `drop` without a target is a usage error, not a silent no-op.
        assert!(Args::try_parse_from(["urx", "cache", "drop"]).is_err());
        assert!(Args::try_parse_from(["urx", "cache", "bogus"]).is_err());
    }

    /// The cache flags are `global`, so they read the same on either side of
    /// the subcommand — and still register as explicitly supplied, which is
    /// what keeps `CLI > config file` precedence working for them.
    #[test]
    fn cache_options_bind_on_either_side_of_the_subcommand() {
        use clap::Parser;

        for argv in [
            vec![
                "urx",
                "--cache-path",
                "/tmp/x.db",
                "--cache-ttl",
                "60",
                "cache",
                "stats",
            ],
            vec![
                "urx",
                "cache",
                "stats",
                "--cache-path",
                "/tmp/x.db",
                "--cache-ttl",
                "60",
            ],
        ] {
            let args = Args::parse_from(argv.clone());
            assert_eq!(
                args.cache_path.as_deref(),
                Some(std::path::Path::new("/tmp/x.db")),
                "{argv:?}"
            );
            assert_eq!(args.cache_ttl, 60, "{argv:?}");

            let (_, provided) = crate::cli::parse_args_from(argv.clone());
            assert!(provided.has("cache_ttl"), "{argv:?}");
            assert!(provided.has("cache_path"), "{argv:?}");
        }
    }

    #[test]
    fn json_is_opt_in_via_the_existing_format_flag() {
        assert!(wants_json("json"));
        assert!(wants_json("jsonl"));
        assert!(wants_json("JSON"));
        // csv has no shape for a stats blob; plain text is the honest fallback.
        assert!(!wants_json("csv"));
        assert!(!wants_json("plain"));
        assert!(!wants_json("nonsense"));
    }

    #[test]
    fn stats_report_names_every_number_the_operator_asked_for() {
        let entries = vec![
            entry("a.test", 10, 7200),
            entry("a.test", 20, 60),
            entry("b.test", 5, 30),
        ];
        let stats = summarize_stats(
            &entries,
            3600,
            "sqlite",
            "/tmp/urx/cache.db".to_string(),
            Some(2_500_000),
        );

        let out = plain(&render_stats(&stats));
        assert!(out.contains("Cache:    sqlite"), "{out}");
        assert!(out.contains("/tmp/urx/cache.db"), "{out}");
        assert!(out.contains("2.4 MiB (database file)"), "{out}");
        assert!(out.contains("Entries:  3"), "{out}");
        assert!(out.contains("Domains:  2"), "{out}");
        assert!(out.contains("URLs:     35"), "{out}");
        assert!(out.contains("Expired:  1"), "{out}");
        assert!(out.contains("--cache-ttl 3600s = 1h 0m"), "{out}");
        assert!(out.contains("Oldest:"), "{out}");
        assert!(out.contains("Newest:"), "{out}");
    }

    #[test]
    fn an_empty_cache_says_so_instead_of_printing_a_wall_of_zeros() {
        let stats = summarize_stats(&[], 3600, "sqlite", "/tmp/cache.db".to_string(), Some(0));
        let out = render_stats(&stats);
        assert!(out.contains("The cache is empty."), "{out}");
        assert!(!out.contains("Entries:"), "{out}");
    }

    #[test]
    fn list_renders_one_row_per_domain_with_ttl_left() {
        let entries = vec![entry("fresh.test", 1234, 60), entry("stale.test", 7, 7200)];
        let rows = summarize_domains(&entries, 3600, None);
        let out = plain(&render_list(&rows, None));

        assert!(out.contains("DOMAIN"), "{out}");
        assert!(out.contains("TTL LEFT"), "{out}");
        // Newest first, and big counts get separators.
        let fresh = out.find("fresh.test").expect("fresh row");
        let stale = out.find("stale.test").expect("stale row");
        assert!(fresh < stale, "{out}");
        assert!(out.contains("1,234"), "{out}");
        assert!(out.contains("expired"), "{out}");
    }

    #[test]
    fn list_distinguishes_an_empty_cache_from_a_pattern_that_matched_nothing() {
        assert!(render_list(&[], None).contains("The cache is empty."));
        let out = render_list(&[], Some("*.example.com"));
        assert!(
            out.contains("No cached domain matches '*.example.com'."),
            "{out}"
        );
    }

    #[test]
    fn prune_reports_zero_as_nothing_to_do() {
        let out = render_prune(
            &PruneReport {
                pruned: 0,
                remaining: 4,
                ttl_seconds: 3600,
            },
            0,
        );
        assert!(out.contains("Nothing to prune"), "{out}");
    }

    #[test]
    fn prune_reports_counts_and_flags_a_concurrent_write() {
        let report = PruneReport {
            pruned: 3,
            remaining: 9,
            ttl_seconds: 86400,
        };
        let out = render_prune(&report, 3);
        assert!(out.contains("Pruned 3 expired entries"), "{out}");
        assert!(out.contains("9 remain"), "{out}");
        assert!(!out.contains("Note:"), "{out}");

        // The pre-count and the delete disagreeing means someone else wrote.
        let out = render_prune(&report, 5);
        assert!(out.contains("Note: 5 entries looked expired"), "{out}");
    }

    #[test]
    fn drop_names_the_patterns_that_matched_nothing() {
        // A typo'd domain otherwise looks exactly like a successful no-op.
        let out = render_drop(&DropReport {
            dropped: 2,
            domains: vec![DroppedDomain {
                domain: "example.com".to_string(),
                entries: 2,
            }],
            unmatched: vec!["exmaple.org".to_string()],
        });
        assert!(out.contains("Dropped 2 entries across 1 domain:"), "{out}");
        assert!(out.contains("example.com  2 entries"), "{out}");
        assert!(
            out.contains("No cached entry matched: exmaple.org"),
            "{out}"
        );

        let out = render_drop(&DropReport {
            dropped: 0,
            domains: vec![],
            unmatched: vec!["nope.test".to_string()],
        });
        assert!(out.contains("nothing was deleted"), "{out}");
    }

    #[test]
    fn clear_report_reads_correctly_when_there_was_nothing_to_clear() {
        assert!(render_clear(&ClearReport { cleared: 0 }).contains("already empty"));
        assert!(render_clear(&ClearReport { cleared: 1 }).contains("Cleared 1 cached entry."));
        assert!(render_clear(&ClearReport { cleared: 12 }).contains("Cleared 12 cached entries."));
    }

    #[test]
    fn durations_read_in_two_units() {
        assert_eq!(format_secs(0), "0s");
        assert_eq!(format_secs(45), "45s");
        assert_eq!(format_secs(90), "1m 30s");
        assert_eq!(format_secs(3600), "1h 0m");
        assert_eq!(format_secs(86_400), "1d 0h");
        assert_eq!(format_secs(90_000), "1d 1h");
        // A saturated TTL must render, not panic.
        assert!(format_secs(i64::MAX).ends_with('h'));
    }

    #[test]
    fn byte_sizes_step_through_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(2_500_000), "2.4 MiB");
    }

    #[test]
    fn digit_grouping() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
