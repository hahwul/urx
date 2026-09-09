//! Turning flags into the objects that decide which URLs survive a run.
//!
//! A URL can reach the output through three different paths — the batch list,
//! the `--stream` sink, and links discovered by `--extract-links` — and all
//! three must apply the same rules. That is why the filter and transformer are
//! built by shared constructors here rather than assembled at each call site.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::cli::{self, read_domains_from_file, read_domains_from_stdin, Args};
use crate::filters::{
    compile_url_regexes, HostValidator, MetaFilter, MetaFilterStats, ScopeMatcher, UrlFilter,
};
use crate::network::{NetworkScope, NetworkSettings};
use crate::output;
use crate::progress::ProgressManager;
use crate::readers::read_urls_from_file;
use crate::runner::ProviderRunResult;
use crate::tester_manager::{self, apply_network_settings_to_tester};
use crate::testers::{
    ArchiveBodyExtractor, ArchiveBodyStats, ArchiveCapture, JsEndpointExtractor, LinkExtractor,
    SpecExpander, StatusChecker, Tester,
};
use crate::utils::{verbose_print, ParamView, UrlTransformer};

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
        .with_max_length(args.max_length)
        // --- result-filters ---
        // Attached to the shared filter so the batch list, the --stream sink
        // and the extracted-link filter all enforce the same scope.
        .with_scope(ScopeMatcher::from_files(&args.scope_file)?);
    // --- end result-filters ---
    Ok(url_filter)
}

// --- result-filters ---
/// Build the post-collection metadata filter from the `--meta-*` flags.
///
/// Fails on a date or status pattern urx cannot honour: unlike `--from`/`--to`,
/// which warn and carry on because a dropped archive-side filter still yields a
/// usable (if wider) result, these decide the *final* result set, so ignoring
/// one would hand back URLs the user explicitly excluded.
pub fn build_meta_filter(args: &Args) -> Result<MetaFilter> {
    let mut filter = MetaFilter::new();
    filter.with_first_seen(
        args.meta_first_seen_after.as_deref(),
        args.meta_first_seen_before.as_deref(),
    )?;
    filter.with_last_seen(
        args.meta_last_seen_after.as_deref(),
        args.meta_last_seen_before.as_deref(),
    )?;
    filter.with_mime(&args.meta_mime, &args.meta_exclude_mime);
    filter.with_archive_status(&args.meta_status, &args.meta_exclude_status)?;
    Ok(filter)
}

/// Reject a `--scope-file` or `--meta-*` value urx cannot honour, before the
/// scan starts.
///
/// Both are otherwise first built once collection has finished — the scope file
/// inside [`build_url_filter`], the metadata predicates inside
/// [`apply_meta_filters`] — so a typo in a scope file or a date bound would
/// surface only after minutes of fetching. Same reason [`build_stream_sink`] is
/// called before the run rather than at first use.
pub fn validate_result_filters(args: &Args) -> Result<()> {
    ScopeMatcher::from_files(&args.scope_file)?;
    build_meta_filter(args)?;
    Ok(())
}

/// Apply the `--meta-*` filters to the batch result.
///
/// Runs *before* [`apply_url_transformations`]: `--merge-endpoint` and
/// `--show-only-host` rewrite URLs, and a rewritten URL no longer keys into the
/// run result that holds its metadata.
///
/// The report is deliberately louder than a plain count. A URL carries archive
/// metadata only when a CDX provider reported it in *this* run — not from a
/// cache hit, not from `--files`, not from `otx`/`vt`/`urlscan`/`github`/
/// `bevigil`/`robots`/`sitemap` — so the failure mode of these flags is a run
/// that returns nothing and looks exactly like a target with nothing to find.
pub fn apply_meta_filters(
    args: &Args,
    run_result: &ProviderRunResult,
    urls: Vec<String>,
    progress_manager: &ProgressManager,
) -> Result<Vec<String>> {
    let filter = build_meta_filter(args)?;
    if filter.is_empty() {
        return Ok(urls);
    }

    let before = urls.len();
    let (kept, stats) = filter.apply(urls, |url| run_result.urls.get(url).map(|e| &e.meta));
    report_meta_filter_stats(args, before, stats, progress_manager);
    Ok(kept)
}

/// Say what the metadata filters did, and why, when the answer is surprising.
fn report_meta_filter_stats(
    args: &Args,
    before: usize,
    stats: MetaFilterStats,
    progress_manager: &ProgressManager,
) {
    verbose_print(
        args,
        format!(
            "Metadata filters kept {}/{before} URLs ({} failed a predicate, {} carried no archive metadata to test)",
            stats.kept, stats.rejected, stats.unknown
        ),
    );

    // Worth saying without -v as well: nothing failed a predicate, everything
    // simply had nothing to test, so the empty result is about the run's shape
    // rather than about the target.
    if stats.dropped_everything_for_lack_of_metadata() && !args.silent {
        progress_manager.note(format!(
            "[urx] --meta-* dropped all {} URLs because none carries archive metadata. \
             Cached results, --files input and the non-CDX providers have none; \
             a CDX provider (wayback, cc, arquivo) run with --no-cache does.",
            stats.unknown
        ));
    }
}
// --- end result-filters ---

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
        // --- result-filters ---
        || !args.scope_file.is_empty()
    // --- end result-filters ---
}

// --- result-filters ---
/// True when any `--meta-*` flag is set. A cheap flag-shape test, unlike
/// [`build_meta_filter`] which also validates the values.
fn has_meta_filters(args: &Args) -> bool {
    args.meta_first_seen_after.is_some()
        || args.meta_first_seen_before.is_some()
        || args.meta_last_seen_after.is_some()
        || args.meta_last_seen_before.is_some()
        || !args.meta_mime.is_empty()
        || !args.meta_exclude_mime.is_empty()
        || !args.meta_status.is_empty()
        || !args.meta_exclude_status.is_empty()
}
// --- end result-filters ---

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
        || args.show_only_param
        // --- output-views ---
        || args.params
        || args.params_by_endpoint
        || args.fuzz_placeholder.is_some();
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
        .with_dedup_similar(args.dedup_similar)
        // --- output-views ---
        // Set here and not in build_url_transformer(): an inventory view needs
        // the whole list, so only the batch path can honour it. The streaming
        // sink and the extracted-link filter both work one URL at a time and
        // must never see it half-applied — --stream rejects the flags outright.
        .with_param_view(param_view(args));

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
    if args.extract_js_endpoints {
        out.push((
            "--extract-js-endpoints",
            "it fetches collected scripts after collection finishes",
        ));
    }
    if args.archive_body {
        out.push((
            "--archive-body",
            "it replays collected URLs from the archive after collection finishes",
        ));
    }
    // --- spec-expansion ---
    if args.expand_specs {
        out.push((
            "--expand-specs",
            "it fetches collected specification documents after collection finishes",
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
    if args.show_meta {
        out.push((
            "--show-meta",
            "a URL is printed on first sighting, before later captures can widen its first/last seen range",
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
    // --- output-views ---
    if args.params {
        out.push((
            "--params",
            "it reports the parameter names of the whole target, which needs every URL first",
        ));
    }
    if args.params_by_endpoint {
        out.push((
            "--params-by-endpoint",
            "it unions the parameters seen per endpoint, which needs every URL first",
        ));
    }
    if args.fuzz_placeholder.is_some() {
        out.push((
            "--fuzz-placeholder",
            "it keeps one URL per parameter signature, which needs every URL first",
        ));
    }
    if args.check_title {
        out.push((
            "--check-title",
            "it re-requests each URL after collection finishes",
        ));
    }
    // --- result-filters ---
    // The sink deliberately drops capture metadata: it prints a URL on first
    // sighting, before the providers that would widen its first/last seen range
    // have answered. There is nothing for these to read, so accepting them
    // would silently emit an unfiltered stream — the same reason --show-meta is
    // rejected here.
    if has_meta_filters(args) {
        out.push((
            "--meta-first-seen-* / --meta-last-seen-* / --meta-mime / --meta-status",
            "they read capture metadata, which is only complete once every provider has answered",
        ));
    }
    // --- end result-filters ---
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
        anyhow::bail!(output::streaming_format_error(&args.format));
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

/// The filter applied to links `--extract-links`, `--extract-js-endpoints` and
/// `--archive-body` discover, or `None` when none of the extractors is
/// running.
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
    // --- spec-expansion --- (`|| args.expand_specs`)
    if !args.extract_links && !args.extract_js_endpoints && !args.archive_body && !args.expand_specs
    {
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
    args.check_status
        || !args.include_status.is_empty()
        || !args.exclude_status.is_empty()
        // --- output-views ---
        // --check-title has nothing to attach a title to without the request
        // the status checker already makes, so it turns that pass on.
        || args.check_title
}

// --- output-views ---
/// The inventory view the flags select, or [`ParamView::None`]. The three flags
/// are mutually exclusive (clap enforces it), so the order here is arbitrary.
fn param_view(args: &Args) -> ParamView {
    if args.params {
        ParamView::Names
    } else if args.params_by_endpoint {
        ParamView::ByEndpoint
    } else if let Some(placeholder) = &args.fuzz_placeholder {
        ParamView::Fuzz(placeholder.clone())
    } else {
        ParamView::None
    }
}

/// Whether the free response facts (`Location`, `Content-Length`,
/// `Content-Type`) a `--check-status` request already carries should be kept.
///
/// Mirrors `wants_capture_meta` in `main.rs`: the structured formats always take
/// them (absent keys are omitted, so a run that collected none is byte-identical
/// to before the fields existed), while plain text is a pipeline contract and
/// keeps one bare URL per line unless `--show-meta` asks otherwise.
fn wants_response_meta(args: &Args) -> bool {
    args.show_meta
        || matches!(
            args.format.to_lowercase().as_str(),
            "json" | "jsonl" | "csv"
        )
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

        // --- output-views ---
        status_checker.with_response_meta(wants_response_meta(args));
        status_checker.with_response_title(args.check_title);
        if args.check_title {
            verbose_print(args, "Reading response bodies to record HTML titles");
        }

        testers.push(Box::new(status_checker));
    }

    if args.extract_links {
        verbose_print(args, "Extracting links from HTML content");

        let mut link_extractor = LinkExtractor::new();
        apply_network_settings_to_tester(&mut link_extractor, network_settings);
        testers.push(Box::new(link_extractor));
    }

    if args.extract_js_endpoints {
        verbose_print(args, "Extracting endpoints from JavaScript content");

        let mut js_extractor = JsEndpointExtractor::new();
        apply_network_settings_to_tester(&mut js_extractor, network_settings);
        js_extractor.with_max_files(args.max_js_files);
        // Providers pace themselves with --rate-limit; this tester re-requests
        // a large slice of the result set from the target, so it must too.
        if network_settings.scope != NetworkScope::Providers {
            js_extractor.with_rate_limit(network_settings.rate_limit);
        }
        testers.push(Box::new(js_extractor));
    }

    // --- spec-expansion ---
    if args.expand_specs {
        verbose_print(args, "Expanding endpoints from API specification documents");

        let mut spec_expander = SpecExpander::new();
        apply_network_settings_to_tester(&mut spec_expander, network_settings);
        spec_expander.with_max_files(args.max_spec_files);
        // Same reasoning as the JS extractor: these requests go to the target,
        // not to a provider, so --rate-limit has to reach them.
        if network_settings.scope != NetworkScope::Providers {
            spec_expander.with_rate_limit(network_settings.rate_limit);
        }
        testers.push(Box::new(spec_expander));
    }

    testers
}

/// URL → the capture `--archive-body` should replay, for every URL the run
/// reported with a digest-bearing capture.
///
/// Built from the run result rather than the output list on purpose: the
/// output may have been reshaped by `--show-only-*`, and a URL that no longer
/// matches what the providers reported has no capture to look up.
pub fn archive_captures(run_result: &ProviderRunResult) -> HashMap<String, ArchiveCapture> {
    run_result
        .urls
        .iter()
        .filter_map(|(url, entry)| {
            let (timestamp, digest) = match entry.meta.newest_capture() {
                Some((ts, digest)) => (ts, Some(digest.to_string())),
                // A capture with a timestamp but no digest can still be
                // replayed; it just cannot be deduplicated against others.
                None => (entry.meta.last_seen()?, None),
            };
            Some((
                url.clone(),
                ArchiveCapture {
                    timestamp: timestamp.to_string(),
                    digest,
                },
            ))
        })
        .collect()
}

/// Build the `--archive-body` tester over the run result, or `None` when the
/// flag is off. The returned stats handle stays readable after the tester has
/// been boxed away and cloned per worker.
pub fn build_archive_body_extractor(
    args: &Args,
    network_settings: &NetworkSettings,
    run_result: &ProviderRunResult,
) -> Option<(ArchiveBodyExtractor, Arc<ArchiveBodyStats>)> {
    if !args.archive_body {
        return None;
    }
    verbose_print(args, "Extracting links from archived response bodies");

    let mut extractor =
        ArchiveBodyExtractor::new(archive_captures(run_result), args.archive_body_limit);
    apply_network_settings_to_tester(&mut extractor, network_settings);
    // Every replay request goes to the Wayback Machine, so the rate that
    // applies is the one the user set for it — `--rate-limit-by wayback=N`
    // first, the global `--rate-limit` otherwise. The tester scope rule is
    // honoured the way it is for the other settings.
    if network_settings.scope != crate::network::NetworkScope::Providers {
        let rate = args
            .rate_limit_overrides()
            .get("wayback")
            .copied()
            .or(network_settings.rate_limit);
        extractor.with_rate_limit(rate);
    }
    // --- spec-expansion ---
    // With --expand-specs also on, an archived specification is read as one
    // rather than run through the HTML link extractor.
    extractor.with_expand_specs(args.expand_specs);

    let stats = extractor.stats();
    Some((extractor, stats))
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
            (vec!["--extract-js-endpoints"], "--extract-js-endpoints"),
            (vec!["--archive-body"], "--archive-body"),
            (vec!["--incremental"], "--incremental"),
            (vec!["--show-sources"], "--show-sources"),
            (vec!["--show-meta"], "--show-meta"),
            // --- spec-expansion ---
            (vec!["--expand-specs"], "--expand-specs"),
            // --- output-views ---
            (vec!["--params"], "--params"),
            (vec!["--params-by-endpoint"], "--params-by-endpoint"),
            (vec!["--fuzz-placeholder", "FUZZ"], "--fuzz-placeholder"),
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
    fn test_response_metadata_follows_the_capture_metadata_rule() {
        // Structured formats always take it (absent keys are omitted, so a run
        // that collected none is unchanged); plain text is a pipeline contract
        // and stays one bare URL per line unless --show-meta asks otherwise.
        let mut args = build_test_args();
        args.format = "plain".to_string();
        assert!(!wants_response_meta(&args));

        args.show_meta = true;
        assert!(wants_response_meta(&args));

        args.show_meta = false;
        for format in ["json", "jsonl", "csv", "JSON"] {
            args.format = format.to_string();
            assert!(wants_response_meta(&args), "{format}");
        }

        // A wordlist carries no per-URL fields at all, so there is nothing to
        // populate them for.
        args.format = "wordlist".to_string();
        assert!(!wants_response_meta(&args));
    }

    #[test]
    fn test_check_title_turns_the_status_pass_on() {
        // Without the status checker's request there is no response to read a
        // title out of, so the flag implies the pass rather than silently
        // doing nothing.
        let mut args = build_test_args();
        assert!(!should_check_status(&args));

        args.check_title = true;
        assert!(should_check_status(&args));
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
    fn test_archive_body_links_get_the_same_filter_as_extracted_links() {
        // The archived-body path discovers links after filtering has run over
        // the primary list, exactly like --extract-links, so it must be given
        // the same filter — or `-e js` and strict host validation are silently
        // bypassed for everything it finds.
        let args = Args::parse_from([
            "urx",
            "--archive-body",
            "-e",
            "js",
            "--silent",
            "example.com",
        ]);
        let filter = build_extracted_link_filter(&args)
            .unwrap()
            .expect("--archive-body must build a filter");
        assert!(filter.accept("https://example.com/app.js").is_some());
        assert!(filter.accept("https://example.com/index.html").is_none());
        assert!(filter.accept("https://cdn.other.net/a.js").is_none());
    }

    #[test]
    fn test_archive_captures_pair_each_url_with_its_newest_capture() {
        use crate::providers::CaptureMeta;
        use crate::runner::UrlEntry;

        let mut run_result = ProviderRunResult::default();
        let mut with_two = UrlEntry::default();
        with_two.absorb(
            "wayback",
            &CaptureMeta::capture(Some("20050101000000"), None, None, Some("OLD")),
        );
        with_two.absorb(
            "cc",
            &CaptureMeta::capture(Some("20240101000000"), None, None, Some("NEW")),
        );
        run_result
            .urls
            .insert("https://example.com/a".to_string(), with_two);

        let mut undigested = UrlEntry::default();
        undigested.absorb(
            "arquivo",
            &CaptureMeta::capture(Some("20200101000000"), None, None, None),
        );
        run_result
            .urls
            .insert("https://example.com/b".to_string(), undigested);

        // A URL from a provider without a capture index has nothing to replay.
        run_result
            .urls
            .insert("https://example.com/c".to_string(), UrlEntry::default());

        let captures = archive_captures(&run_result);
        assert_eq!(
            captures["https://example.com/a"],
            ArchiveCapture {
                timestamp: "20240101000000".to_string(),
                digest: Some("NEW".to_string()),
            }
        );
        assert_eq!(
            captures["https://example.com/b"],
            ArchiveCapture {
                timestamp: "20200101000000".to_string(),
                digest: None,
            }
        );
        assert!(!captures.contains_key("https://example.com/c"));
    }

    #[test]
    fn test_build_archive_body_extractor_follows_the_flag_and_the_wayback_rate() {
        let settings = NetworkSettings::default();
        let run_result = ProviderRunResult::default();

        let args = build_test_args();
        assert!(build_archive_body_extractor(&args, &settings, &run_result).is_none());

        let mut args = build_test_args();
        args.archive_body = true;
        args.archive_body_limit = 7;
        let (extractor, stats) =
            build_archive_body_extractor(&args, &settings, &run_result).unwrap();
        assert_eq!(extractor.candidate_count(), 0);
        assert_eq!(stats.fetched(), 0);
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

        // --extract-js-endpoints is a third tester, independent of the others.
        let mut args = build_test_args();
        args.extract_js_endpoints = true;
        assert_eq!(build_testers(&args, &settings).len(), 1);

        let mut args = build_test_args();
        args.check_status = true;
        args.extract_links = true;
        args.extract_js_endpoints = true;
        assert_eq!(build_testers(&args, &settings).len(), 3);
    }

    #[test]
    fn test_extract_js_endpoints_filter_is_wired_up() {
        // Endpoints mined from scripts come into existence after the filters
        // ran over the primary list, exactly like extracted links — so the
        // same filter must be built for them, including strict host
        // validation (on by default).
        let args = Args::parse_from(["urx", "--extract-js-endpoints", "--silent", "example.com"]);
        let filter = build_extracted_link_filter(&args)
            .unwrap()
            .expect("--extract-js-endpoints must build a filter");
        assert_eq!(
            filter.accept("https://example.com/api/v1/users").as_deref(),
            Some("https://example.com/api/v1/users")
        );
        assert!(filter
            .accept("https://api.thirdparty.net/v1/track")
            .is_none());
    }

    // --- spec-expansion ---

    #[test]
    fn test_expand_specs_is_its_own_tester_and_honours_max_spec_files() {
        let settings = NetworkSettings::default();

        let mut args = build_test_args();
        args.expand_specs = true;
        assert_eq!(build_testers(&args, &settings).len(), 1);

        // Independent of the other extractors: all four can run in one pass,
        // and the status checker still comes first.
        let mut args = build_test_args();
        args.check_status = true;
        args.extract_links = true;
        args.extract_js_endpoints = true;
        args.expand_specs = true;
        assert_eq!(build_testers(&args, &settings).len(), 4);

        // The flag alone builds nothing when it is off.
        let args = build_test_args();
        assert!(build_testers(&args, &settings).is_empty());
    }

    #[test]
    fn test_expand_specs_filter_is_wired_up() {
        // Routes read out of a specification come into existence after the
        // filters ran over the primary list, exactly like extracted links — so
        // the same filter must be built for them, including strict host
        // validation (on by default).
        let args = Args::parse_from(["urx", "--expand-specs", "--silent", "example.com"]);
        let filter = build_extracted_link_filter(&args)
            .unwrap()
            .expect("--expand-specs must build a filter");
        assert_eq!(
            filter
                .accept("https://example.com/v3/users/{id}")
                .as_deref(),
            Some("https://example.com/v3/users/{id}")
        );
        assert!(filter
            .accept("https://api.thirdparty.net/v1/track")
            .is_none());
    }

    #[test]
    fn test_archive_body_expands_specs_only_when_both_flags_are_on() {
        let settings = NetworkSettings::default();
        let run_result = ProviderRunResult::default();

        let mut args = build_test_args();
        args.archive_body = true;
        let (extractor, _) = build_archive_body_extractor(&args, &settings, &run_result).unwrap();
        assert!(!extractor.expands_specs());

        args.expand_specs = true;
        let (extractor, _) = build_archive_body_extractor(&args, &settings, &run_result).unwrap();
        assert!(extractor.expands_specs());
    }

    #[test]
    fn test_max_spec_files_defaults_match_the_expander() {
        let args = Args::parse_from(["urx", "example.com"]);
        assert_eq!(args.max_spec_files, SpecExpander::DEFAULT_MAX_FILES);
    }
    // --- result-filters ---
    /// Write `text` to a temp file and return the handle, which must outlive the
    /// use — dropping it deletes the file.
    fn scope_file(text: &str) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{text}").unwrap();
        file.flush().unwrap();
        file
    }

    /// A run result in which each `(url, first_seen, mime, archive_status)` was
    /// reported by a CDX provider, and every other URL was not.
    fn run_result_with_meta(rows: &[(&str, &str, &str, &str)]) -> ProviderRunResult {
        let mut result = ProviderRunResult::default();
        for (url, ts, mime, status) in rows {
            result.urls.insert(
                (*url).to_string(),
                crate::runner::UrlEntry {
                    sources: HashSet::from(["wayback".to_string()]),
                    meta: crate::providers::CaptureMeta::capture(
                        (!ts.is_empty()).then_some(ts),
                        (!mime.is_empty()).then_some(mime),
                        (!status.is_empty()).then_some(status),
                        None,
                    ),
                },
            );
        }
        result
    }

    fn urls(list: &[&str]) -> Vec<String> {
        list.iter().map(|u| u.to_string()).collect()
    }

    #[test]
    fn a_scope_file_reaches_the_shared_url_filter() {
        let file = scope_file("*.example.com\n!admin.example.com\n");
        let mut args = build_test_args();
        args.scope_file = vec![file.path().to_path_buf()];

        let filter = build_url_filter(&args).unwrap();
        assert!(filter.matches("https://api.example.com/v1"));
        assert!(!filter.matches("https://admin.example.com/v1"));
        assert!(!filter.matches("https://example.org/v1"));

        // ...and it composes with the other filters rather than replacing them.
        args.extensions = vec!["js".to_string()];
        let filter = build_url_filter(&args).unwrap();
        assert!(filter.matches("https://api.example.com/app.js"));
        assert!(!filter.matches("https://api.example.com/index.html"));
    }

    #[test]
    fn a_scope_file_survives_the_stream_and_extracted_link_paths() {
        // The three emission paths share one filter precisely so a scope cannot
        // apply to some of them and not others.
        let file = scope_file("api.example.com\n");
        let args = Args::parse_from([
            "urx",
            "--extract-links",
            "--no-strict",
            "--silent",
            "--scope-file",
            file.path().to_str().unwrap(),
            "example.com",
        ]);

        let link_filter = build_extracted_link_filter(&args)
            .unwrap()
            .expect("--extract-links must build a filter");
        assert_eq!(
            link_filter.accept("https://api.example.com/v1").as_deref(),
            Some("https://api.example.com/v1")
        );
        assert!(link_filter.accept("https://cdn.example.com/x.js").is_none());

        // Streaming is not one of the combinations a scope file conflicts with.
        let args = Args::parse_from([
            "urx",
            "--stream",
            "--scope-file",
            file.path().to_str().unwrap(),
            "example.com",
        ]);
        assert!(streaming_conflicts(&args).is_empty());
    }

    #[test]
    fn a_scope_file_that_cannot_be_parsed_stops_the_run() {
        // Silently dropping the entry would run against a wider scope than the
        // file describes, which for a bug bounty is the expensive direction.
        let file = scope_file("*.example.com\nhttps://example.com/only/this/path\n");
        let mut args = build_test_args();
        args.scope_file = vec![file.path().to_path_buf()];

        match build_url_filter(&args) {
            Ok(_) => panic!("an unhonourable scope line must be fatal"),
            Err(err) => {
                let rendered = format!("{err:#}");
                assert!(rendered.contains("path-scoped"), "{rendered}");
                assert!(rendered.contains(":2"), "{rendered}");
            }
        }
    }

    #[test]
    fn a_scope_file_is_anded_with_strict_host_validation() {
        // The two gates are independent, and strict mode runs first: a wildcard
        // scope over a bare-apex query still needs --subs.
        let file = scope_file("*.example.com\n");
        let with_subs = Args::parse_from([
            "urx",
            "--subs",
            "--silent",
            "--scope-file",
            file.path().to_str().unwrap(),
            "example.com",
        ]);
        let strict_only = Args::parse_from([
            "urx",
            "--silent",
            "--scope-file",
            file.path().to_str().unwrap(),
            "example.com",
        ]);

        let discovered: HashSet<String> = urls(&[
            "https://example.com/a",
            "https://api.example.com/a",
            "https://example.org/a",
        ])
        .into_iter()
        .collect();

        let kept = apply_url_filters(&with_subs, &discovered, &ProgressManager::new(true)).unwrap();
        assert_eq!(
            kept,
            urls(&["https://api.example.com/a", "https://example.com/a"])
        );

        // Without --subs, host validation removes the subdomain before the
        // scope file ever sees it — the scope does not widen the query.
        let kept =
            apply_url_filters(&strict_only, &discovered, &ProgressManager::new(true)).unwrap();
        assert_eq!(kept, urls(&["https://example.com/a"]));
    }

    #[test]
    fn a_scope_file_makes_the_filter_progress_bar_worth_drawing() {
        let file = scope_file("*.example.com\n");
        let mut args = build_test_args();
        assert!(!has_url_filters(&args));
        args.scope_file = vec![file.path().to_path_buf()];
        assert!(has_url_filters(&args));
    }

    #[test]
    fn meta_filters_narrow_the_batch_result_uniformly_across_providers() {
        // The case the archive-side filters cannot serve: a positive multi-value
        // status list, applied to URLs from a mix of providers.
        let run_result = run_result_with_meta(&[
            (
                "https://example.com/ok",
                "20200101000000",
                "text/html",
                "200",
            ),
            (
                "https://example.com/moved",
                "20200101000000",
                "text/html",
                "301",
            ),
            (
                "https://example.com/gone",
                "20200101000000",
                "text/html",
                "404",
            ),
        ]);

        let mut args = build_test_args();
        args.meta_status = vec!["200".to_string(), "301".to_string()];

        let kept = apply_meta_filters(
            &args,
            &run_result,
            urls(&[
                "https://example.com/ok",
                "https://example.com/moved",
                "https://example.com/gone",
            ]),
            &ProgressManager::new(true),
        )
        .unwrap();

        assert_eq!(
            kept,
            urls(&["https://example.com/ok", "https://example.com/moved"])
        );
    }

    #[test]
    fn meta_date_filters_read_the_merged_capture_range() {
        let run_result = run_result_with_meta(&[
            ("https://example.com/old", "20050101000000", "", ""),
            ("https://example.com/new", "20240101000000", "", ""),
        ]);

        let mut args = build_test_args();
        args.meta_last_seen_before = Some("2010".to_string());

        let kept = apply_meta_filters(
            &args,
            &run_result,
            urls(&["https://example.com/old", "https://example.com/new"]),
            &ProgressManager::new(true),
        )
        .unwrap();
        assert_eq!(kept, urls(&["https://example.com/old"]));
    }

    #[test]
    fn urls_with_no_metadata_survive_an_exclusion_but_not_a_positive_filter() {
        // The documented policy for cache hits, --files input and the providers
        // with no capture index.
        let run_result = ProviderRunResult::default();
        let list = urls(&["https://example.com/a", "https://example.com/b"]);

        let mut positive = build_test_args();
        positive.meta_mime = vec!["text/html".to_string()];
        assert!(apply_meta_filters(
            &positive,
            &run_result,
            list.clone(),
            &ProgressManager::new(true)
        )
        .unwrap()
        .is_empty());

        let mut negative = build_test_args();
        negative.meta_exclude_mime = vec!["image/*".to_string()];
        assert_eq!(
            apply_meta_filters(
                &negative,
                &run_result,
                list.clone(),
                &ProgressManager::new(true)
            )
            .unwrap(),
            list
        );
    }

    #[test]
    fn no_meta_flag_leaves_the_list_untouched() {
        let run_result = ProviderRunResult::default();
        let list = urls(&["https://example.com/a", "https://example.com/b"]);
        assert_eq!(
            apply_meta_filters(
                &build_test_args(),
                &run_result,
                list.clone(),
                &ProgressManager::new(true)
            )
            .unwrap(),
            list
        );
    }

    #[test]
    fn a_result_set_emptied_only_by_missing_metadata_says_so_without_verbose() {
        let (progress, notes) = ProgressManager::capturing();
        let mut args = build_test_args();
        args.silent = false;
        args.meta_status = vec!["200".to_string()];

        let kept = apply_meta_filters(
            &args,
            &ProviderRunResult::default(),
            urls(&["https://example.com/a"]),
            &progress,
        )
        .unwrap();

        assert!(kept.is_empty());
        let notes = notes.lock().unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("--no-cache"), "{}", notes[0]);
    }

    #[test]
    fn a_partially_filtered_result_set_stays_quiet() {
        // Something did fail a predicate, so the result is about the data, not
        // about the run's shape — no note.
        let (progress, notes) = ProgressManager::capturing();
        let mut args = build_test_args();
        args.silent = false;
        args.meta_status = vec!["200".to_string()];

        let run_result =
            run_result_with_meta(&[("https://example.com/gone", "20200101000000", "", "404")]);
        let kept = apply_meta_filters(
            &args,
            &run_result,
            urls(&["https://example.com/gone", "https://example.com/bare"]),
            &progress,
        )
        .unwrap();

        assert!(kept.is_empty());
        assert!(notes.lock().unwrap().is_empty());
    }

    #[test]
    fn an_unhonourable_meta_value_stops_the_run() {
        let mut bad_date = build_test_args();
        bad_date.meta_first_seen_after = Some("last tuesday".to_string());
        assert!(build_meta_filter(&bad_date).is_err());

        let mut bad_status = build_test_args();
        bad_status.meta_exclude_status = vec!["4o4".to_string()];
        let err = build_meta_filter(&bad_status).unwrap_err();
        assert!(err.to_string().contains("20x"), "{err}");
    }

    #[test]
    fn streaming_rejects_every_meta_filter() {
        for flag in [
            "--meta-first-seen-after",
            "--meta-first-seen-before",
            "--meta-last-seen-after",
            "--meta-last-seen-before",
        ] {
            let args = Args::parse_from(["urx", "--stream", flag, "2020", "example.com"]);
            assert!(
                !streaming_conflicts(&args).is_empty(),
                "{flag} must not stream"
            );
            assert!(build_stream_sink(&args).is_err(), "{flag}");
        }
        for (flag, value) in [
            ("--meta-mime", "text/html"),
            ("--meta-exclude-mime", "image/*"),
            ("--meta-status", "200"),
            ("--meta-exclude-status", "404"),
        ] {
            let args = Args::parse_from(["urx", "--stream", flag, value, "example.com"]);
            assert!(
                !streaming_conflicts(&args).is_empty(),
                "{flag} must not stream"
            );
        }

        // The reason is the one the user needs: the sink has no metadata to read.
        let args = Args::parse_from(["urx", "--stream", "--meta-status", "200", "example.com"]);
        match build_stream_sink(&args) {
            Ok(_) => panic!("--meta-status must not stream"),
            Err(err) => assert!(err.to_string().contains("capture metadata"), "{err}"),
        }
    }
    #[test]
    fn unhonourable_filter_values_are_rejected_before_the_scan_starts() {
        // The point of the check: these are otherwise first built after
        // collection, so a typo would cost the user the whole fetch.
        assert!(validate_result_filters(&build_test_args()).is_ok());

        let good = scope_file("*.example.com\n");
        let mut args = build_test_args();
        args.scope_file = vec![good.path().to_path_buf()];
        args.meta_status = vec!["20x".to_string()];
        args.meta_last_seen_after = Some("2020".to_string());
        assert!(validate_result_filters(&args).is_ok());

        let bad = scope_file("example.com:8080\n");
        let mut args = build_test_args();
        args.scope_file = vec![bad.path().to_path_buf()];
        assert!(validate_result_filters(&args).is_err());

        let mut args = build_test_args();
        args.meta_first_seen_before = Some("nope".to_string());
        assert!(validate_result_filters(&args).is_err());
    }
    // --- end result-filters ---
}
