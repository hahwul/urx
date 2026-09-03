use anyhow::Result;
use std::sync::Arc;

mod app;
mod cache;
mod cli;
mod config;
mod filters;
mod network;
mod output;
mod progress;
mod providers;
mod readers;
mod runner;
mod tester_manager;
mod testers;
mod utils;

#[cfg(test)]
mod test_support;

use app::caching::{create_cache_manager, process_domains_with_cache};
use app::catalog::print_provider_list;
use app::keys::seed_api_keys_from_env;
use app::pipeline::{
    apply_url_filters, apply_url_transformations, build_extracted_link_filter, build_stream_sink,
    build_testers, collect_domains, read_urls_from_files, should_check_status,
};
use app::report::{configure_colors, print_provider_stats, render_header, write_per_domain_output};
use app::selection::{initialize_providers, validate_selection_flags};
use cli::{Args, CliProvided};
use config::Config;
use network::NetworkSettings;
use output::create_outputter;
use progress::ProgressManager;
use runner::{process_domains, ProviderRunResult, ProviderStats};
use tester_manager::process_urls_with_testers;
use utils::verbose_print;

/// Load the config layers over `args`, honouring the documented precedence:
/// `CLI/env` > provider-config file > main config file.
fn apply_config_layers(args: &mut Args, provided: &CliProvided) -> Result<()> {
    // Must run before either config layer: afterwards there is no way to tell a
    // key the user supplied from one a config file filled in.
    let direct = seed_api_keys_from_env(args);

    Config::load(args)?.apply_to_args(args, provided);

    // The provider-config file is separate from the main config and overrides
    // it, but still loses to anything supplied on the CLI or in the environment.
    config::ProviderKeysConfig::load(args)?.apply_to_args(
        args,
        direct.vt,
        direct.urlscan,
        direct.zoomeye,
        direct.github,
    );

    Ok(())
}

/// Fetch URLs, either from `--files` or by running the selected providers.
///
/// `header` is returned rather than dropped locally: it is a transient line in
/// the live progress region and must outlive this call so it is cleared
/// together with the bars.
async fn collect_urls(
    args: &Args,
    network_settings: &NetworkSettings,
    progress_manager: &ProgressManager,
    stream_sink: Option<&Arc<output::StreamSink>>,
    header: &mut Option<indicatif::ProgressBar>,
) -> Result<ProviderRunResult> {
    // Ahead of the `--files` short-circuit below, which never reaches
    // `initialize_providers`. `--preset` is applied to file input just like
    // provider output, so a misspelled preset used to be dropped in silence
    // there and emit an unfiltered run that looked like the filter had matched
    // everything — the exact failure this validation exists to prevent.
    validate_selection_flags(args)?;

    // File input skips provider processing entirely. Every URL is attributed to
    // "file" so downstream `--show-sources` stays consistent.
    let read_started = std::time::Instant::now();
    if let Some(urls) = read_urls_from_files(args)? {
        let elapsed = read_started.elapsed();
        // Counted before deduplication, matching what a provider row reports.
        let url_count = urls.len();
        let mut url_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for url in urls {
            url_map.entry(url).or_default().insert("file".to_string());
        }
        return Ok(ProviderRunResult {
            urls: url_map,
            // `--stats` must not fall silent just because the URLs came from a
            // file rather than a provider: an explicit flag printing nothing at
            // all reads as "the run collected nothing". Labelled "file", the
            // same source name `--show-sources` gives these URLs. `error_count`
            // is honestly 0 — `read_urls_from_files` propagates the first
            // failure, so reaching here means every file was read.
            stats: vec![ProviderStats {
                name: "file".to_string(),
                url_count,
                error_count: 0,
                partial_count: 0,
                elapsed,
                // Reading local files has no deadline to be cut off by.
                aborted: false,
            }],
        });
    }

    let domains = collect_domains(args)?;
    if domains.is_empty() {
        // A usage error, not a successful empty run: reported through the
        // `Result` so the process exits non-zero. Printing it and returning
        // `Ok` made `urx | wc -l` in a script look like a target that simply
        // has no archived URLs.
        return Err(anyhow::anyhow!(
            "No domains provided. Pass DOMAINS positionally, use --domain-list FILE, or pipe them through stdin."
        ));
    }

    let (providers, provider_names) = initialize_providers(args, network_settings)?;

    *header = Some(
        progress_manager.create_header_line(render_header(domains.len(), provider_names.len())),
    );

    // Streaming writes as it goes and bypasses the cache entirely (the cache
    // both reads whole domains and writes whole result sets).
    if let Some(sink) = stream_sink {
        return Ok(process_domains(
            domains,
            args,
            progress_manager,
            &providers,
            &provider_names,
            Some(Arc::clone(sink)),
        )
        .await);
    }

    let cache_manager = create_cache_manager(args).await?;
    process_domains_with_cache(
        domains,
        args,
        progress_manager,
        &providers,
        &provider_names,
        cache_manager.as_ref(),
    )
    .await
}

/// Re-request the surviving URLs when `--check-status` or `--extract-links`
/// asked for it; otherwise wrap them unchanged.
async fn run_testers(
    args: &Args,
    network_settings: &NetworkSettings,
    progress_manager: &ProgressManager,
    urls: Vec<String>,
) -> Result<Vec<output::UrlData>> {
    let testers = build_testers(args, network_settings);
    if testers.is_empty() {
        return Ok(urls.into_iter().map(output::UrlData::new).collect());
    }

    Ok(process_urls_with_testers(
        urls,
        args,
        progress_manager,
        testers,
        should_check_status(args),
        build_extracted_link_filter(args)?,
    )
    .await)
}

/// Attach provider attribution to each surviving record when `--show-sources`
/// was set. URLs introduced by the link extractor aren't in the run result and
/// keep an empty `sources` list.
fn attach_sources(final_urls: &mut [output::UrlData], run_result: &ProviderRunResult) {
    for entry in final_urls.iter_mut() {
        if let Some(providers) = run_result.urls.get(&entry.url) {
            let mut sources: Vec<String> = providers.iter().cloned().collect();
            sources.sort();
            sources.dedup();
            entry.sources = sources;
        }
    }
}

/// Write the result set to stdout or `--output`, and to `--output-dir` when set.
fn write_output(args: &Args, final_urls: &[output::UrlData]) {
    let outputter = create_outputter(&args.format);
    match outputter.output(final_urls, args.output.clone(), args.silent) {
        Ok(()) => {
            if let Some(path) = &args.output {
                verbose_print(args, format!("Results written to: {}", path.display()));
            }
        }
        Err(e) => {
            if !args.silent {
                eprintln!("Error writing output: {e}");
            }
        }
    }

    let Some(dir) = &args.output_dir else {
        return;
    };
    match write_per_domain_output(final_urls, dir, &args.format, args.silent) {
        Ok(()) => verbose_print(
            args,
            format!("Per-domain results written under: {}", dir.display()),
        ),
        Err(e) => {
            if !args.silent {
                eprintln!("Error writing per-domain output to {}: {e}", dir.display());
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (mut args, provided) = cli::parse_args();

    // Short-circuits: emit the requested artifact and exit without touching the
    // config layers, the network, or the domain list — these flags are useful
    // on their own, with no target named.
    if let Some(shell) = args.completions {
        return app::shell::print_completions(shell);
    }
    if args.manpage {
        return app::shell::print_man_page();
    }
    if args.list_providers {
        print_provider_list();
        return Ok(());
    }

    apply_config_layers(&mut args, &provided)?;

    // Honor --no-color / NO_COLOR before any styled output is produced.
    configure_colors(&args);

    let network_settings = NetworkSettings::from_args(&args);
    let progress_manager = ProgressManager::new(args.no_progress || args.silent);

    // Built before the scan so a rejected option combination fails immediately
    // rather than after minutes of fetching.
    let stream_sink = build_stream_sink(&args)?;

    let mut header = None;
    let run_result = collect_urls(
        &args,
        &network_settings,
        &progress_manager,
        stream_sink.as_ref(),
        &mut header,
    )
    .await?;

    if let Some(sink) = &stream_sink {
        // Everything has already been written by the sink; printing the
        // (deliberately empty) batch result again would duplicate nothing but
        // would still emit a stray JSON array / CSV header.
        progress_manager.clear();
        verbose_print(&args, format!("Streamed {} URLs", sink.emitted()));
        if args.stats && !args.silent {
            print_provider_stats(&run_result.stats);
        }
        return Ok(());
    }

    // URL-only view for filters (they don't care about sources).
    let all_urls: std::collections::HashSet<String> = run_result.urls.keys().cloned().collect();
    let sorted_urls = apply_url_filters(&args, &all_urls, &progress_manager)?;
    let transformed_urls = apply_url_transformations(&args, sorted_urls, &progress_manager);

    let mut final_urls = run_testers(
        &args,
        &network_settings,
        &progress_manager,
        transformed_urls,
    )
    .await?;

    if args.show_sources {
        attach_sources(&mut final_urls, &run_result);
    }

    // Progress is transient: tear down the live region (header + all bars) now
    // that scanning is done, so the only thing left on screen is the result —
    // the URL list printed below.
    progress_manager.clear();

    write_output(&args, &final_urls);

    if args.stats && !args.silent {
        print_provider_stats(&run_result.stats);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;

    /// A run with nothing to scan must fail, not report success.
    ///
    /// `--domain-list` is used rather than an empty argv because
    /// [`collect_domains`] falls back to reading stdin when no target was named
    /// at all, which would block the test. A file holding only unusable entries
    /// exercises the same "zero effective targets" path without it — and is
    /// itself a case that used to exit 0 while scanning nothing.
    #[tokio::test]
    async fn no_effective_targets_is_a_usage_error() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Neither line yields a host: one is scheme-only, one is a bare slash.
        writeln!(file, "https://\n/").unwrap();

        let args = Args::parse_from([
            "urx",
            "--silent",
            "--no-progress",
            "--domain-list",
            file.path().to_str().unwrap(),
        ]);

        let err = collect_urls(
            &args,
            &NetworkSettings::default(),
            &ProgressManager::new(true),
            None,
            &mut None,
        )
        .await
        .expect_err("a run with no resolvable target must not succeed");

        assert!(err.to_string().contains("No domains provided"), "{err}");
    }

    /// Build an `Args` for a `--files` run over a temp file of `urls`.
    fn files_args(file: &tempfile::NamedTempFile, extra: &[&str]) -> Args {
        let mut argv = vec![
            "urx",
            "--no-progress",
            "--files",
            file.path().to_str().unwrap(),
        ];
        argv.extend_from_slice(extra);
        Args::parse_from(argv)
    }

    fn temp_urls() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "https://example.com/a.js\nhttps://example.com/b.php\nhttps://example.com/c.png"
        )
        .unwrap();
        file
    }

    async fn run_files(args: &Args) -> Result<ProviderRunResult> {
        collect_urls(
            args,
            &NetworkSettings::default(),
            &ProgressManager::new(true),
            None,
            &mut None,
        )
        .await
    }

    /// Regression: `--files` short-circuits before `initialize_providers`, so
    /// none of the selection flags were validated on that path. A misspelled
    /// `--preset` was the damaging case — presets *are* applied to file input,
    /// so `--preset only-jss` silently emitted every URL, which reads as a
    /// filter that matched everything.
    #[tokio::test]
    async fn selection_flags_are_validated_for_file_input_too() {
        let file = temp_urls();

        for (extra, expected) in [
            (
                vec!["--preset", "only-jss"],
                "Unknown preset(s) in --preset",
            ),
            (
                vec!["--providers", "bogus"],
                "Unknown provider id(s) in --providers",
            ),
            (
                vec!["--exclude-providers", "bogus"],
                "Unknown provider id(s) in --exclude-providers",
            ),
            (
                vec!["--rate-limit-by", "wayback=fast"],
                "Malformed entry in --rate-limit-by",
            ),
        ] {
            let args = files_args(&file, &extra);
            let err = run_files(&args)
                .await
                .expect_err("a bad selection flag must fail even with --files");
            assert!(err.to_string().contains(expected), "{extra:?}: {err}");
        }

        // ...and a valid invocation still runs.
        assert!(run_files(&files_args(&file, &["--preset", "only-js"]))
            .await
            .is_ok());
    }

    /// Regression: file input reported `stats: Vec::new()`, and
    /// `print_provider_stats` returns early on an empty slice — so an explicit
    /// `--stats` printed absolutely nothing, which reads as "the run collected
    /// nothing" rather than "these URLs did not come from a provider".
    #[tokio::test]
    async fn file_input_reports_a_stats_row() {
        let file = temp_urls();
        let result = run_files(&files_args(&file, &["--stats"])).await.unwrap();

        assert_eq!(result.stats.len(), 1, "{:?}", result.stats);
        let row = &result.stats[0];
        assert_eq!(row.name, "file");
        // Counted before deduplication, exactly as a provider row is.
        assert_eq!(row.url_count, 3);
        assert_eq!(row.error_count, 0);
        assert_eq!(row.partial_count, 0);
        // Every URL is still attributed to the "file" source.
        assert_eq!(result.urls.len(), 3);
        assert!(result.urls.values().all(|sources| sources.contains("file")));
    }
}
