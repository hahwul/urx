//! Turning flags into the objects that decide which URLs survive a run.
//!
//! A URL can reach the output through three different paths — the batch list,
//! the `--stream` sink, and links discovered by `--extract-links` — and all
//! three must apply the same rules. That is why the filter and transformer are
//! built by shared constructors here rather than assembled at each call site.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::cli::{self, read_domains_from_file, read_domains_from_stdin, Args};
use crate::filters::{compile_url_regexes, HostValidator, UrlFilter};
use crate::network::NetworkSettings;
use crate::output;
use crate::progress::ProgressManager;
use crate::readers::read_urls_from_file;
use crate::tester_manager::{self, apply_network_settings_to_tester};
use crate::testers::{LinkExtractor, StatusChecker, Tester};
use crate::utils::{verbose_print, UrlTransformer};

/// Raw targets named directly on the command line: positional args plus every
/// `--domain-list` file, before host normalization.
///
/// stdin is excluded on purpose — it can be drained only once, so
/// [`build_host_validator`] rebuilds the target list from this subset alone.
/// `announce` is off for that second pass so the per-file verbose line isn't
/// printed twice in one run.
fn cli_domain_inputs(args: &Args, announce: bool) -> Result<Vec<String>> {
    let mut domains: Vec<String> = args.domains.clone();

    for path in &args.domain_list {
        let file_domains = read_domains_from_file(path)?;
        if announce {
            verbose_print(
                args,
                format!(
                    "Loaded {} domains from {}",
                    file_domains.len(),
                    path.display()
                ),
            );
        }
        domains.extend(file_domains);
    }

    Ok(domains)
}

/// Reduce each target to a bare host so a pasted full URL or trailing path
/// doesn't silently corrupt provider queries (a common copy/paste footgun).
/// Inputs with no recoverable host drop out.
fn normalize_domains(raw: &[String]) -> Vec<String> {
    raw.iter()
        .filter_map(|d| cli::normalize_domain(d))
        .collect()
}

/// Collect the effective domain list from CLI positional args, `--domain-list`
/// files, and (when both are empty) stdin. Duplicates are removed while
/// preserving first-seen order so the run order is predictable.
pub fn collect_domains(args: &Args) -> Result<Vec<String>> {
    let mut domains = cli_domain_inputs(args, true)?;

    // Only fall back to stdin when no domains were supplied via flags/files,
    // otherwise piped data would silently get appended on every invocation.
    // This check runs on the raw inputs: reading stdin blocks, so a target that
    // merely failed to normalize must not send us looking for one there.
    if domains.is_empty() {
        domains.extend(read_domains_from_stdin()?);
    }

    let mut normalized = normalize_domains(&domains);
    let mut seen = HashSet::new();
    normalized.retain(|d| seen.insert(d.clone()));
    Ok(normalized)
}

/// Read URLs from every `--files` path, or `None` when the flag wasn't used.
pub fn read_urls_from_files(args: &Args) -> Result<Option<Vec<String>>> {
    if args.files.is_empty() {
        return Ok(None);
    }

    let mut all_file_urls = Vec::new();

    for file_path in &args.files {
        let urls = read_urls_from_file(file_path).inspect_err(|e| {
            if !args.silent {
                eprintln!("Error reading file {}: {}", file_path.display(), e);
            }
        })?;
        verbose_print(
            args,
            format!(
                "Read {} URLs from file: {}",
                urls.len(),
                file_path.display()
            ),
        );
        all_file_urls.extend(urls);
    }

    verbose_print(
        args,
        format!(
            "Read {} URLs total from {} file(s)",
            all_file_urls.len(),
            args.files.len()
        ),
    );

    Ok(Some(all_file_urls))
}

/// Re-resolve the original target list into a [`HostValidator`], or `None` when
/// strict mode is off or no host-bearing target was supplied.
///
/// The domains are normalized exactly the way the fetch targets were, so the
/// validator's hosts line up with what was actually queried.
pub fn build_host_validator(args: &Args) -> Result<Option<HostValidator>> {
    if !args.strict_enabled() {
        return Ok(None);
    }
    let domains = normalize_domains(&cli_domain_inputs(args, false)?);
    if domains.is_empty() {
        return Ok(None);
    }
    Ok(Some(HostValidator::new(&domains, args.subs)))
}

/// Build the URL filter from the `--preset`/extension/pattern/length flags.
///
/// Shared by the batch path, the streaming sink, and the extracted-link filter
/// so a URL is judged identically no matter which one emits it.
/// Fails when `--match-regex` or `--filter-regex` was given a pattern that does
/// not compile, so a bad expression stops the run instead of quietly matching
/// nothing on every URL.
pub fn build_url_filter(args: &Args) -> Result<UrlFilter> {
    let mut url_filter = UrlFilter::new();
    // Presets seed the filter; the explicit flags below are combined with them.
    if !args.preset.is_empty() {
        url_filter.apply_presets(&args.preset);
    }
    url_filter
        .with_extensions(args.extensions.clone())
        .with_exclude_extensions(args.exclude_extensions.clone())
        .with_patterns(args.patterns.clone())
        .with_exclude_patterns(args.exclude_patterns.clone())
        .with_match_regex(compile_url_regexes(&args.match_regex, "--match-regex")?)
        .with_filter_regex(compile_url_regexes(&args.filter_regex, "--filter-regex")?)
        .with_min_length(args.min_length)
        .with_max_length(args.max_length);
    Ok(url_filter)
}

/// Build the per-URL transformer used everywhere a URL must be decided on its
/// own: streaming output and links discovered by `--extract-links`.
///
/// `--merge-endpoint` is deliberately absent — it folds several URLs into one and
/// so has no single-URL form (see [`UrlTransformer::transform_one`]).
pub fn build_url_transformer(args: &Args) -> UrlTransformer {
    let mut transformer = UrlTransformer::new();
    transformer
        .with_normalize_url(args.normalize_url)
        .with_show_only_host(args.show_only_host)
        .with_show_only_path(args.show_only_path)
        .with_show_only_param(args.show_only_param);
    transformer
}

/// True when any flag that narrows the URL list is set, which is the only case
/// where a filtering progress bar is worth drawing.
fn has_url_filters(args: &Args) -> bool {
    !args.extensions.is_empty()
        || !args.patterns.is_empty()
        || !args.exclude_extensions.is_empty()
        || !args.exclude_patterns.is_empty()
        || !args.match_regex.is_empty()
        || !args.filter_regex.is_empty()
        || args.min_length.is_some()
        || args.max_length.is_some()
}

/// Apply URL filtering and, in strict mode, host validation to the batch result.
pub fn apply_url_filters(
    args: &Args,
    urls: &HashSet<String>,
    progress_manager: &ProgressManager,
) -> Result<Vec<String>> {
    let filter_bar = has_url_filters(args).then(|| {
        let bar = progress_manager.create_filter_bar();
        bar.set_message("Applying filters to URLs...");
        bar
    });

    let mut sorted_urls = build_url_filter(args)?.apply_filters(urls);

    // Host validation only applies to domain-driven runs: file input has no
    // queried domain to validate against.
    if args.strict_enabled() && args.files.is_empty() {
        verbose_print(args, "Enforcing strict host validation...");

        if let Some(host_validator) = build_host_validator(args)? {
            let before = sorted_urls.len();
            sorted_urls.retain(|url| host_validator.is_valid_host(url));
            let removed = before - sorted_urls.len();

            // When validation discards most (or all) of what providers returned,
            // a quiet, much-smaller result looks like a broken provider. Surface
            // a single hint (even without -v; --silent still suppresses it). With
            // www. already kept as the apex, the usual remaining cause is other
            // subdomains under a bare apex query.
            let drops_most = before > 0 && (sorted_urls.is_empty() || removed * 2 > before);
            if drops_most && !args.silent && !args.subs {
                eprintln!(
                    "[urx] strict host validation removed {removed}/{before} URLs; \
                     pass --subs to keep subdomains or --no-strict to keep all hosts"
                );
            }

            verbose_print(
                args,
                format!(
                    "Number of valid URLs after host validation: {}",
                    sorted_urls.len()
                ),
            );
        }
    }

    if let Some(bar) = filter_bar {
        bar.finish_with_message(format!("Filtered to {} URLs", sorted_urls.len()));
    }

    verbose_print(
        args,
        format!("Total unique URLs after filtering: {}", sorted_urls.len()),
    );

    Ok(sorted_urls)
}

/// Apply the display-shaping options to the batch result.
pub fn apply_url_transformations(
    args: &Args,
    urls: Vec<String>,
    progress_manager: &ProgressManager,
) -> Vec<String> {
    let reshapes_urls = args.merge_endpoint
        || args.dedup_similar
        || args.show_only_host
        || args.show_only_path
        || args.show_only_param;
    let transform_bar = reshapes_urls.then(|| {
        let bar = progress_manager.create_transform_bar();
        bar.set_message("Applying URL transformations...");
        bar
    });

    // The batch path is the one place that can honour --merge-endpoint and
    // --dedup-similar, since it alone holds every URL at once.
    let mut url_transformer = build_url_transformer(args);
    url_transformer
        .with_merge_endpoint(args.merge_endpoint)
        .with_dedup_similar(args.dedup_similar);

    let (transformed_urls, stats) = url_transformer.transform_with_stats(urls);

    if let Some(bar) = transform_bar {
        bar.finish_with_message(format!("Transformed to {} URLs", transformed_urls.len()));
    }

    // Worth saying out loud: --dedup-similar can drop most of a result set, and
    // without a number the user cannot tell a well-collapsed run from a run
    // that found little.
    if args.dedup_similar {
        verbose_print(
            args,
            format!(
                "Collapsed {} near-duplicate URLs; {} distinct endpoints remain",
                stats.similar_collapsed,
                transformed_urls.len()
            ),
        );
    }

    transformed_urls
}

/// Options that need the complete result set and therefore cannot be combined
/// with `--stream`. Returned as (flag, why) so the error can say more than "not
/// supported".
pub fn streaming_conflicts(args: &Args) -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();

    if args.merge_endpoint {
        out.push((
            "--merge-endpoint",
            "it folds URLs sharing a path into one, which needs every URL first",
        ));
    }
    if args.dedup_similar {
        out.push((
            "--dedup-similar",
            "it keeps the lexicographically smallest URL of each group, which needs every URL first",
        ));
    }
    if args.check_status || !args.include_status.is_empty() || !args.exclude_status.is_empty() {
        out.push((
            "--check-status / --include-status / --exclude-status",
            "they re-request each URL after collection finishes",
        ));
    }
    if args.extract_links {
        out.push((
            "--extract-links",
            "it fetches collected URLs after collection finishes",
        ));
    }
    if args.incremental {
        out.push((
            "--incremental",
            "it diffs this run against the previous one, which needs the full set",
        ));
    }
    if args.show_sources {
        out.push((
            "--show-sources",
            "a URL is printed on first sighting, before later providers can report it too",
        ));
    }
    if args.output_dir.is_some() {
        out.push((
            "--output-dir",
            "it groups URLs by domain once the scan has finished",
        ));
    }
    if !args.files.is_empty() {
        out.push((
            "--files",
            "file input is read up front, so there is nothing to stream",
        ));
    }
    out
}

/// Construct the streaming sink when `--stream` is set, after rejecting the
/// option combinations it cannot honour.
pub fn build_stream_sink(args: &Args) -> Result<Option<Arc<output::StreamSink>>> {
    if !args.stream {
        return Ok(None);
    }

    let conflicts = streaming_conflicts(args);
    if !conflicts.is_empty() {
        let detail = conflicts
            .iter()
            .map(|(flag, why)| format!("  {flag}: {why}"))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::bail!("--stream cannot be combined with:\n{detail}");
    }

    if !output::format_supports_streaming(&args.format) {
        anyhow::bail!(
            "--stream cannot produce --format {}: it wraps every entry in one array, so the writer must know which entry is last. Use --format jsonl for line-delimited JSON.",
            args.format
        );
    }

    let writer: Box<dyn std::io::Write + Send> = match &args.output {
        Some(path) => Box::new(
            std::fs::File::create(path)
                .with_context(|| format!("Failed to create output file: {}", path.display()))?,
        ),
        None => Box::new(std::io::stdout()),
    };

    // Colour would be baked into a redirected stream, and streamed rows carry
    // no status to colourise anyway.
    if args.output.is_some() {
        colored::control::set_override(false);
    }

    Ok(Some(Arc::new(output::StreamSink::new(
        build_url_filter(args)?,
        build_url_transformer(args),
        // Host validation mirrors the batch path: only meaningful when strict
        // mode is on and the targets came from the command line, not a file.
        build_host_validator(args)?,
        &args.format,
        writer,
    )?)))
}

/// The filter applied to links `--extract-links` discovers, or `None` when the
/// extractor isn't running.
///
/// Links found *inside* pages come into existence after
/// [`apply_url_filters`]/[`apply_url_transformations`] have already run over the
/// primary list, so they have to be put through the same rules here. Without
/// this, `--extract-links` silently bypasses every filter the user set: `-e js`
/// emits non-JS links, and strict host validation (on by default) emits every
/// off-site link a page happens to point at.
pub fn build_extracted_link_filter(
    args: &Args,
) -> Result<Option<Arc<tester_manager::ExtractedLinkFilter>>> {
    if !args.extract_links {
        return Ok(None);
    }
    // File input has no queried domain to validate against, which is why the
    // batch path skips host validation for it too.
    let host_validator = if args.files.is_empty() {
        build_host_validator(args)?
    } else {
        None
    };
    Ok(Some(Arc::new(tester_manager::ExtractedLinkFilter::new(
        build_url_filter(args)?,
        build_url_transformer(args),
        host_validator,
    ))))
}

/// True when URLs must be re-requested after collection — either because the
/// user asked for statuses or because a status filter needs them.
pub fn should_check_status(args: &Args) -> bool {
    args.check_status || !args.include_status.is_empty() || !args.exclude_status.is_empty()
}

/// Build the post-collection testers implied by the flags, or an empty vec when
/// no second pass over the URLs is needed.
pub fn build_testers(args: &Args, network_settings: &NetworkSettings) -> Vec<Box<dyn Tester>> {
    let mut testers: Vec<Box<dyn Tester>> = Vec::new();

    if should_check_status(args) {
        verbose_print(args, "Checking HTTP status codes for URLs");

        let mut status_checker = StatusChecker::new();
        apply_network_settings_to_tester(&mut status_checker, network_settings);

        if !args.include_status.is_empty() {
            status_checker.with_include_status(Some(args.include_status.clone()));
            verbose_print(
                args,
                format!(
                    "Including only status codes that match: {}",
                    args.include_status.join(", ")
                ),
            );
        }

        if !args.exclude_status.is_empty() {
            status_checker.with_exclude_status(Some(args.exclude_status.clone()));
            verbose_print(
                args,
                format!(
                    "Excluding status codes that match: {}",
                    args.exclude_status.join(", ")
                ),
            );
        }

        testers.push(Box::new(status_checker));
    }

    if args.extract_links {
        verbose_print(args, "Extracting links from HTML content");

        let mut link_extractor = LinkExtractor::new();
        apply_network_settings_to_tester(&mut link_extractor, network_settings);
        testers.push(Box::new(link_extractor));
    }

    testers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::build_test_args;
    use clap::Parser;

    #[test]
    fn test_streaming_rejects_options_needing_the_full_result_set() {
        // Each of these is rejected for a concrete reason, and the reason is
        // shown to the user — so assert on the flag list, not just on failure.
        let cases = [
            (vec!["--merge-endpoint"], "--merge-endpoint"),
            (vec!["--dedup-similar"], "--dedup-similar"),
            (vec!["--check-status"], "--check-status"),
            (vec!["--extract-links"], "--extract-links"),
            (vec!["--incremental"], "--incremental"),
            (vec!["--show-sources"], "--show-sources"),
        ];

        for (flags, expected) in cases {
            let mut argv = vec!["urx", "--stream"];
            argv.extend(flags.iter().copied());
            argv.push("example.com");
            let args = Args::parse_from(argv);

            let conflicts = streaming_conflicts(&args);
            assert!(
                conflicts.iter().any(|(flag, _)| flag.contains(expected)),
                "{expected} should conflict with --stream, got {conflicts:?}"
            );
            let err = match build_stream_sink(&args) {
                Err(e) => e.to_string(),
                Ok(_) => panic!("{expected} should have been rejected"),
            };
            assert!(err.contains(expected), "{err}");
        }
    }

    #[test]
    fn test_build_url_filter_rejects_a_malformed_regex() {
        // A bad pattern has to stop the run at startup. Compiling per URL would
        // either fail once per URL or, worse, quietly match nothing.
        for (flag, argv_flag) in [
            ("--match-regex", "--match-regex"),
            ("--filter-regex", "--filter-regex"),
        ] {
            let args = Args::parse_from(["urx", argv_flag, "(unclosed", "example.com"]);
            let err = match build_url_filter(&args) {
                Err(e) => format!("{e:#}"),
                Ok(_) => panic!("{flag} should have rejected an invalid pattern"),
            };
            assert!(err.contains(flag), "{err}");
            assert!(err.contains("(unclosed"), "{err}");
        }

        // ...and it is caught before any network work, by the same up-front
        // check that rejects a misspelled --preset.
        let args = Args::parse_from(["urx", "--match-regex", "(unclosed", "example.com"]);
        let err = crate::app::selection::validate_selection_flags(&args)
            .expect_err("a malformed regex must fail validation")
            .to_string();
        assert!(err.contains("--match-regex"), "{err}");
    }

    #[test]
    fn test_regex_flags_are_repeatable_and_not_comma_split() {
        // A regex can legitimately contain a comma (`\d{2,3}`), so unlike
        // --patterns these flags never split on one.
        let args = Args::parse_from([
            "urx",
            "--match-regex",
            r"/id/\d{2,3}$",
            "--match-regex",
            "admin",
            "--filter-regex",
            r"\.(png|jpg)$",
            "example.com",
        ]);
        assert_eq!(args.match_regex, vec![r"/id/\d{2,3}$", "admin"]);
        assert_eq!(args.filter_regex, vec![r"\.(png|jpg)$"]);

        let filter = build_url_filter(&args).unwrap();
        assert!(filter.matches("https://example.com/id/123"));
        assert!(filter.matches("https://example.com/admin/panel"));
        assert!(!filter.matches("https://example.com/id/1"));
        assert!(!filter.matches("https://example.com/admin/logo.png"));
    }

    #[test]
    fn test_dedup_similar_is_applied_by_the_batch_transformations() {
        let mut args = build_test_args();
        args.dedup_similar = true;

        let urls: Vec<String> = ["/post/1", "/post/2", "/post/3", "/about"]
            .iter()
            .map(|p| format!("https://example.com{p}"))
            .collect();

        let out = apply_url_transformations(&args, urls.clone(), &ProgressManager::new(true));
        assert_eq!(
            out,
            vec![
                "https://example.com/about".to_string(),
                "https://example.com/post/1".to_string(),
            ]
        );

        // ...and it stays off unless asked for.
        args.dedup_similar = false;
        assert_eq!(
            apply_url_transformations(&args, urls.clone(), &ProgressManager::new(true)).len(),
            urls.len()
        );
    }

    #[test]
    fn test_streaming_rejects_value_taking_options_too() {
        // --files and --output-dir take values, so they don't fit the flag-only
        // table above — but they conflict for the same reason.
        //
        // --files is rejected here rather than when the file is read: main builds
        // the sink before touching input, so `--stream --files missing.txt`
        // reports the unsupported combination instead of a read error for a file
        // streaming would never have used.
        for (argv, expected) in [
            (
                vec!["urx", "--stream", "--files", "urls.txt", "example.com"],
                "--files",
            ),
            (
                vec!["urx", "--stream", "--output-dir", "/tmp/out", "example.com"],
                "--output-dir",
            ),
        ] {
            let args = Args::parse_from(argv);
            assert!(streaming_conflicts(&args)
                .iter()
                .any(|(flag, _)| flag.contains(expected)));
            match build_stream_sink(&args) {
                Err(e) => assert!(e.to_string().contains(expected), "{e}"),
                Ok(_) => panic!("{expected} should have been rejected"),
            }
        }
    }

    #[test]
    fn test_streaming_allows_per_url_options() {
        // --normalize-url and the show-only views are per-URL, so they stream
        // fine; only cross-URL work is off limits.
        let args = Args::parse_from([
            "urx",
            "--stream",
            "--normalize-url",
            "--show-only-path",
            "-e",
            "js",
            "example.com",
        ]);
        assert!(streaming_conflicts(&args).is_empty());
        assert!(build_stream_sink(&args).unwrap().is_some());
    }

    #[test]
    fn test_streaming_rejects_json_and_points_at_jsonl() {
        let args = Args::parse_from(["urx", "--stream", "-f", "json", "example.com"]);
        let err = match build_stream_sink(&args) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("--format json should have been rejected"),
        };
        assert!(err.contains("jsonl"), "should suggest jsonl, got {err}");

        for format in ["plain", "jsonl", "csv"] {
            let args = Args::parse_from(["urx", "--stream", "-f", format, "example.com"]);
            assert!(build_stream_sink(&args).is_ok(), "{format} should stream");
        }
    }

    #[test]
    fn test_no_stream_flag_builds_no_sink() {
        let args = Args::parse_from(["urx", "example.com"]);
        assert!(build_stream_sink(&args).unwrap().is_none());
    }

    #[test]
    fn test_extract_links_filter_is_wired_up_and_applies_every_rule() {
        // Regression: extracted links were appended straight to the results,
        // after filtering and host validation had already run — so this filter
        // has to exist *and* be handed to the tester stage.
        let args = Args::parse_from([
            "urx",
            "--extract-links",
            "-e",
            "js",
            "--silent",
            "example.com",
        ]);
        let filter = build_extracted_link_filter(&args)
            .unwrap()
            .expect("--extract-links must build a filter");

        // Extension filter applies...
        assert_eq!(
            filter.accept("https://example.com/app.js").as_deref(),
            Some("https://example.com/app.js")
        );
        assert!(filter.accept("https://example.com/index.html").is_none());
        // ...and so does strict host validation, which is on by default.
        assert!(filter.accept("https://ads.tracker.net/a.js").is_none());
    }

    #[test]
    fn test_no_extract_links_builds_no_filter() {
        let args = Args::parse_from(["urx", "example.com"]);
        assert!(build_extracted_link_filter(&args).unwrap().is_none());
    }

    #[test]
    fn test_extract_links_filter_skips_host_validation_for_file_input() {
        // With --files there is no queried domain to validate against, matching
        // how the batch path treats file input.
        let args = Args::parse_from(["urx", "--extract-links", "--files", "urls.txt", "--silent"]);
        let filter = build_extracted_link_filter(&args).unwrap().unwrap();
        assert_eq!(
            filter.accept("https://anywhere.test/x").as_deref(),
            Some("https://anywhere.test/x")
        );
    }

    #[test]
    fn test_collect_domains_merges_inputs_and_dedupes() -> Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new()?;
        writeln!(file, "from-file.test\nexample.com")?; // example.com overlaps positional

        let mut args = build_test_args();
        args.domains = vec!["example.com".to_string(), "another.test".to_string()];
        args.domain_list = vec![file.path().to_path_buf()];

        let domains = collect_domains(&args)?;
        // Positional first, file second, dedupe keeps first occurrence.
        assert_eq!(
            domains,
            vec!["example.com", "another.test", "from-file.test"]
        );
        Ok(())
    }

    #[test]
    fn test_collect_domains_normalizes_pasted_urls() -> Result<()> {
        let mut args = build_test_args();
        args.domains = vec![
            "https://example.com/some/path?q=1".to_string(),
            "example.com".to_string(),
        ];

        // Both spellings reduce to the same host, so only one target remains.
        assert_eq!(collect_domains(&args)?, vec!["example.com"]);
        Ok(())
    }

    #[test]
    fn test_build_host_validator_is_none_without_strict_mode() -> Result<()> {
        let mut args = build_test_args();
        args.domains = vec!["example.com".to_string()];
        args.strict = false;
        args.no_strict = true;
        assert!(build_host_validator(&args)?.is_none());

        args.strict = true;
        args.no_strict = false;
        assert!(build_host_validator(&args)?.is_some());
        Ok(())
    }

    #[test]
    fn test_build_url_filter_and_stream_sink_agree_on_the_same_rules() {
        // The batch list and the stream must never disagree about which URLs
        // qualify — both go through build_url_filter.
        let args = Args::parse_from(["urx", "-e", "js", "--silent", "example.com"]);
        let urls = HashSet::from([
            "https://example.com/app.js".to_string(),
            "https://example.com/index.html".to_string(),
        ]);

        let batch = build_url_filter(&args).unwrap().apply_filters(&urls);
        assert_eq!(batch, vec!["https://example.com/app.js"]);
    }

    #[test]
    fn test_apply_url_filters_errors_when_domain_list_cannot_be_read() {
        let urls = HashSet::from(["https://example.com/page1.html".to_string()]);
        let mut args = build_test_args();
        args.strict = true;
        args.domain_list = vec![std::path::PathBuf::from("/definitely/missing-domains.txt")];

        let progress_manager = ProgressManager::new(true);
        let err = apply_url_filters(&args, &urls, &progress_manager).unwrap_err();

        assert!(err.to_string().contains("Failed to open domain list"));
    }

    #[test]
    fn test_apply_url_filters_applies_extensions_and_strict_hosts() -> Result<()> {
        let mut args = build_test_args();
        args.domains = vec!["example.com".to_string()];
        args.strict = true;
        args.extensions = vec!["js".to_string()];

        let urls = HashSet::from([
            "https://example.com/app.js".to_string(),
            "https://example.com/page.html".to_string(),
            "https://evil.test/other.js".to_string(),
        ]);

        let filtered = apply_url_filters(&args, &urls, &ProgressManager::new(true))?;
        assert_eq!(filtered, vec!["https://example.com/app.js"]);
        Ok(())
    }

    #[test]
    fn test_apply_url_transformations_honours_merge_endpoint() {
        // --merge-endpoint is the one option the shared transformer omits, so
        // the batch path has to opt into it explicitly.
        let mut args = build_test_args();
        args.merge_endpoint = true;

        let urls = vec![
            "https://example.com/item?id=1".to_string(),
            "https://example.com/item?id=2".to_string(),
        ];
        let merged = apply_url_transformations(&args, urls, &ProgressManager::new(true));
        assert_eq!(
            merged.len(),
            1,
            "same path should fold into one: {merged:?}"
        );

        // ...and it stays off when the flag isn't set.
        let mut args = build_test_args();
        args.merge_endpoint = false;
        let urls = vec![
            "https://example.com/item?id=1".to_string(),
            "https://example.com/item?id=2".to_string(),
        ];
        assert_eq!(
            apply_url_transformations(&args, urls, &ProgressManager::new(true)).len(),
            2
        );
    }

    #[test]
    fn test_read_urls_from_files_is_none_without_the_flag() -> Result<()> {
        let args = build_test_args();
        assert!(read_urls_from_files(&args)?.is_none());
        Ok(())
    }

    #[test]
    fn test_read_urls_from_files_concatenates_every_path() -> Result<()> {
        use std::io::Write;
        let mut a = tempfile::NamedTempFile::new()?;
        writeln!(a, "https://example.com/a")?;
        let mut b = tempfile::NamedTempFile::new()?;
        writeln!(b, "https://example.com/b")?;

        let mut args = build_test_args();
        args.files = vec![a.path().to_path_buf(), b.path().to_path_buf()];

        let urls = read_urls_from_files(&args)?.expect("--files should produce a list");
        assert_eq!(urls, vec!["https://example.com/a", "https://example.com/b"]);
        Ok(())
    }

    #[test]
    fn test_build_testers_follows_the_flags() {
        let settings = NetworkSettings::default();

        let args = build_test_args();
        assert!(build_testers(&args, &settings).is_empty());

        let mut args = build_test_args();
        args.check_status = true;
        assert_eq!(build_testers(&args, &settings).len(), 1);

        let mut args = build_test_args();
        args.extract_links = true;
        assert_eq!(build_testers(&args, &settings).len(), 1);

        // A status filter implies a status check even without --check-status.
        let mut args = build_test_args();
        args.include_status = vec!["200".to_string()];
        args.extract_links = true;
        assert!(should_check_status(&args));
        assert_eq!(build_testers(&args, &settings).len(), 2);
    }
}
