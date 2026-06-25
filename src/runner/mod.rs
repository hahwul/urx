use futures::future::join_all;
use futures::stream::{self, StreamExt};
use indicatif::ProgressBar;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::task;

use crate::cli::Args;
use crate::network::{NetworkScope, NetworkSettings};
use crate::progress::{
    provider_error_style, provider_partial_style, provider_running_style, provider_success_style,
    ProgressManager, ProgressReporter,
};
use crate::providers::Provider;
use crate::utils::verbose_print;

/// Format an integer with thousands separators (e.g. `12345` → `12,345`) so
/// large URL counts stay legible in the progress summary.
fn fmt_count(n: usize) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Update a provider line that is fetching several domains concurrently with an
/// aggregate "done/total · URLs" counter — one line can't show every in-flight
/// domain, so we summarise. Ticks so the spinner keeps moving between
/// completions.
fn tick_aggregate(
    bar: &ProgressBar,
    done: usize,
    total: usize,
    urls: usize,
    no_progress: bool,
    silent: bool,
) {
    bar.set_message(format!("{done}/{total} domains · {} URLs", fmt_count(urls)));
    if !no_progress && !silent {
        bar.tick();
    }
}

/// Render an error as a single short line for a progress label, truncating on
/// a char boundary so a verbose chain doesn't blow out the terminal width.
fn short_error(e: &anyhow::Error) -> String {
    let msg = e.to_string();
    let one_line = msg.split('\n').next().unwrap_or(&msg);
    let truncated: String = one_line.chars().take(80).collect();
    if truncated.chars().count() < one_line.chars().count() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

/// Lock a mutex, recovering the inner data if another task panicked while
/// holding it. One failed provider task must not poison the shared state and
/// take down the rest of the run or lose already-collected URLs.
fn lock_ignore_poison<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Shared state for tracking domain completion across provider tasks.
struct DomainCompletionCtx {
    total_providers: usize,
    total_domains: usize,
    domain_completion: Arc<Mutex<HashMap<String, usize>>>,
    processed_domains: Arc<Mutex<usize>>,
    overall_bar: ProgressBar,
    verbose: bool,
    silent: bool,
}

impl DomainCompletionCtx {
    /// Mark one provider as finished for `domain` and update progress bars.
    ///
    /// Returns `true` if the domain is now fully complete (all providers finished).
    fn track(&self, domain: &str) -> bool {
        let mut is_domain_complete = false;
        {
            let mut completion_map = lock_ignore_poison(&self.domain_completion);
            if let Some(count) = completion_map.get_mut(domain) {
                *count += 1;
                is_domain_complete = *count >= self.total_providers;
            }
        }

        if is_domain_complete {
            let mut count = lock_ignore_poison(&self.processed_domains);
            *count += 1;
            self.overall_bar.set_position(*count as u64);
            self.overall_bar.set_message(format!(
                "Completed {}/{} domains",
                *count, self.total_domains
            ));

            if self.verbose && !self.silent {
                println!(
                    "Domain completed: {} ({}/{})",
                    domain, *count, self.total_domains
                );
            }
        }

        is_domain_complete
    }
}

/// Helper function to apply network settings to a provider
pub fn apply_network_settings_to_provider(provider: &mut dyn Provider, settings: &NetworkSettings) {
    // Skip applying settings if network scope doesn't include providers
    if settings.scope == NetworkScope::Testers {
        return;
    }

    provider.with_subdomains(settings.include_subdomains);
    provider.with_timeout(settings.timeout);
    provider.with_retries(settings.retries);
    provider.with_random_agent(settings.random_agent);
    provider.with_insecure(settings.insecure);

    if let Some(proxy) = &settings.proxy {
        provider.with_proxy(Some(proxy.clone()));

        if let Some(auth) = &settings.proxy_auth {
            provider.with_proxy_auth(Some(auth.clone()));
        }
    }

    if let Some(rate) = settings.rate_limit {
        provider.with_rate_limit(Some(rate));
    }
}

pub fn add_provider<T: Provider + 'static>(
    args: &Args,
    network_settings: &NetworkSettings,
    providers: &mut Vec<Box<dyn Provider>>,
    provider_names: &mut Vec<String>,
    provider_id: &str,
    provider_name: String,
    provider_builder: impl FnOnce() -> T,
) {
    // Apply a per-provider rate limit override when --rate-limit-by lists this
    // provider id. Cloning lets us thread the override into the existing
    // apply_network_settings_to_provider helper without changing its API.
    let per_provider_rate = args.rate_limit_overrides().get(provider_id).copied();
    let mut effective_settings = network_settings.clone();
    if per_provider_rate.is_some() {
        effective_settings.rate_limit = per_provider_rate;
    }

    if args.verbose && !args.silent {
        let mut config_info = vec![
            format!("Adding {provider_name} provider"),
            format!("  Timeout: {} seconds", effective_settings.timeout),
            format!("  Retries: {}", effective_settings.retries),
            format!("  Parallel requests: {}", effective_settings.parallel),
        ];

        if effective_settings.include_subdomains {
            config_info.push("  Subdomain inclusion: enabled".to_string());
        }

        if let Some(proxy) = &effective_settings.proxy {
            config_info.push(format!("  Proxy: {}", proxy));
        }

        if effective_settings.random_agent {
            config_info.push("  Random User-Agent: enabled".to_string());
        }

        if let Some(rate) = effective_settings.rate_limit {
            let label = if per_provider_rate.is_some() {
                " (per-provider override)"
            } else {
                ""
            };
            config_info.push(format!("  Rate limit: {rate} requests/second{label}"));
        }

        println!("{}", config_info.join("\n"));
    }

    let mut provider = provider_builder();
    apply_network_settings_to_provider(&mut provider, &effective_settings);
    providers.push(Box::new(provider));
    provider_names.push(provider_name);
}

/// Per-provider tally for end-of-run summaries (`--stats`).
#[derive(Debug, Clone, Default)]
pub struct ProviderStats {
    /// Provider name (e.g. "Wayback Machine").
    pub name: String,
    /// Cumulative URLs returned across all domains.
    pub url_count: usize,
    /// Number of domain fetches that failed.
    pub error_count: usize,
    /// Number of domain fetches that returned incomplete (partial) results.
    pub partial_count: usize,
    /// Total wall-clock time spent in fetch_urls across domains.
    pub elapsed: std::time::Duration,
}

/// Result of a provider run: URLs mapped to the providers that reported them,
/// plus per-provider stats indexed in the same order as `provider_names`.
#[derive(Debug, Default)]
pub struct ProviderRunResult {
    pub urls: HashMap<String, HashSet<String>>,
    pub stats: Vec<ProviderStats>,
}

/// Process domains using a provider-based concurrency pattern.
///
/// Returns each discovered URL along with the set of providers that reported
/// it. Order within each source set is preserved by the caller via sort+dedup.
pub async fn process_domains(
    domains: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    providers: &[Box<dyn Provider>],
    provider_names: &[String],
) -> ProviderRunResult {
    // Map URL -> set of provider names that reported it.
    let all_urls: Arc<Mutex<HashMap<String, HashSet<String>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let total_domains = domains.len();
    let total_providers = providers.len();

    // Per-provider stats, indexed identically to `provider_names`.
    let stats: Arc<Mutex<Vec<ProviderStats>>> = Arc::new(Mutex::new(
        provider_names
            .iter()
            .map(|n| ProviderStats {
                name: n.clone(),
                ..Default::default()
            })
            .collect(),
    ));

    // Create a progress bar for overall progress
    let overall_bar = progress_manager.create_domain_bar(total_domains);
    overall_bar.set_message("Processing domains");

    // Create a shared counter for processed domains
    let processed_domains = Arc::new(Mutex::new(0usize));

    // Create provider bars - one bar per provider
    let provider_bars = progress_manager.create_provider_bars(provider_names);

    // Create a tracking set for each domain to know when it's fully processed
    let domain_completion = Arc::new(Mutex::new(
        domains
            .iter()
            .map(|d| (d.clone(), 0))
            .collect::<HashMap<String, usize>>(),
    ));

    verbose_print(
        args,
        format!("Using provider-based concurrency with {total_providers} providers"),
    );

    // Clone provider data for use in async tasks
    let provider_data: Vec<_> = providers
        .iter()
        .enumerate()
        .map(|(idx, provider)| (provider.clone_box(), provider_names[idx].clone(), idx))
        .collect();

    // Create a future for each provider
    let mut provider_futures = Vec::new();

    // Extract the values we need from Args to avoid lifetime issues
    let verbose = args.verbose;
    let silent = args.silent;
    let no_progress = args.no_progress;

    // --parallel bounds how many of a provider's domains are fetched at once.
    // The shared per-provider rate limiter (stored in the provider and cloned
    // per domain) keeps --rate-limit honest across these concurrent fetches.
    let parallel = args.parallel.unwrap_or(5).max(1) as usize;

    for (provider_clone, provider_name, original_idx) in provider_data.into_iter() {
        let all_urls = Arc::clone(&all_urls);
        let stats = Arc::clone(&stats);
        let provider_bar = provider_bars[original_idx].clone();
        let domains = domains.clone();

        // Shared so each concurrent domain future can mark domain completion
        // against the run-wide progress without contending on a &mut.
        let completion_ctx = Arc::new(DomainCompletionCtx {
            total_providers,
            total_domains,
            domain_completion: Arc::clone(&domain_completion),
            processed_domains: Arc::clone(&processed_domains),
            overall_bar: overall_bar.clone(),
            verbose,
            silent,
        });

        // With one domain in flight the single provider line can show rich
        // per-domain detail (live page counts). With several concurrent, that
        // line can't represent them all, so fall back to an aggregate counter.
        let effective_parallel = parallel.min(domains.len().max(1));
        let rich = effective_parallel <= 1;

        // Spawn a task for this provider
        let provider_future = task::spawn(async move {
            let provider = Arc::new(provider_clone);
            // Running totals are atomics so the concurrent domain futures below
            // can update them; read back for an honest end-of-run summary.
            let url_total = Arc::new(AtomicUsize::new(0));
            let err_total = Arc::new(AtomicUsize::new(0));
            let partial_total = Arc::new(AtomicUsize::new(0));
            let done = Arc::new(AtomicUsize::new(0));
            let total = domains.len();

            // Handles retained for the summary after the stream consumes the
            // per-domain clones.
            let summary_bar = provider_bar.clone();
            let summary_name = provider_name.clone();
            let summary_urls = Arc::clone(&url_total);
            let summary_errs = Arc::clone(&err_total);
            let summary_partials = Arc::clone(&partial_total);

            // Prime the line. In aggregate mode the elapsed timer measures the
            // whole provider run; rich mode resets it per domain below.
            provider_bar.set_style(provider_running_style());
            provider_bar.set_prefix(format!("{provider_name:<16}"));
            provider_bar.reset_elapsed();
            if !rich {
                provider_bar.set_message(format!("0/{total} domains"));
            }
            if !no_progress && !silent {
                provider_bar.tick();
            }

            stream::iter(domains)
                .map(move |domain| {
                    let provider = Arc::clone(&provider);
                    let provider_bar = provider_bar.clone();
                    let provider_name = provider_name.clone();
                    let all_urls = Arc::clone(&all_urls);
                    let stats = Arc::clone(&stats);
                    let completion_ctx = Arc::clone(&completion_ctx);
                    let url_total = Arc::clone(&url_total);
                    let err_total = Arc::clone(&err_total);
                    let partial_total = Arc::clone(&partial_total);
                    let done = Arc::clone(&done);

                    async move {
                        let prefix = format!("{domain} · ");

                        // Rich mode: the reporter drives the visible line with
                        // live page-by-page detail and re-arms the spinner.
                        // Aggregate mode: it only carries the partial-result
                        // flag (a hidden bar) so concurrent domains don't fight
                        // over the single line; --silent suppresses it entirely.
                        let reporter = if silent {
                            None
                        } else if rich {
                            provider_bar.set_style(provider_running_style());
                            provider_bar.set_prefix(format!("{provider_name:<16}"));
                            provider_bar.reset_elapsed();
                            provider_bar.set_message(format!("{prefix}fetching…"));
                            if !no_progress {
                                provider_bar.tick();
                            }
                            Some(ProgressReporter::new(provider_bar.clone(), prefix.clone()))
                        } else {
                            Some(ProgressReporter::new(ProgressBar::hidden(), prefix.clone()))
                        };

                        // Fetch URLs for this domain using this provider.
                        let fetch_start = std::time::Instant::now();
                        let fetch_result = provider
                            .fetch_urls_with_progress(&domain, reporter.clone())
                            .await;
                        let fetch_elapsed = fetch_start.elapsed();
                        match fetch_result {
                            Ok(urls) => {
                                let url_count = urls.len();
                                url_total.fetch_add(url_count, Ordering::Relaxed);

                                // A *partial* result (e.g. a page failed
                                // mid-pagination) is surfaced as a distinct,
                                // warned state so a truncated crawl is never
                                // mistaken for a clean success.
                                let partial =
                                    reporter.as_ref().is_some_and(|r| r.is_partial());
                                if partial {
                                    partial_total.fetch_add(1, Ordering::Relaxed);
                                }

                                // Add URLs to the shared map (URL -> providers).
                                {
                                    let mut url_map = lock_ignore_poison(&all_urls);
                                    for url in urls {
                                        url_map
                                            .entry(url)
                                            .or_default()
                                            .insert(provider_name.clone());
                                    }
                                }

                                // Update per-provider stats.
                                {
                                    let mut s = lock_ignore_poison(&stats);
                                    s[original_idx].url_count += url_count;
                                    if partial {
                                        s[original_idx].partial_count += 1;
                                    }
                                    s[original_idx].elapsed += fetch_elapsed;
                                }

                                let done_n = done.fetch_add(1, Ordering::Relaxed) + 1;
                                if rich {
                                    if partial {
                                        provider_bar.set_style(provider_partial_style());
                                        provider_bar
                                            .set_prefix(format!("◐ {provider_name:<16}"));
                                        provider_bar.set_message(format!(
                                            "{domain} · {} URLs (partial)",
                                            fmt_count(url_count)
                                        ));
                                    } else {
                                        provider_bar.set_style(provider_success_style());
                                        provider_bar
                                            .set_prefix(format!("✓ {provider_name:<16}"));
                                        provider_bar.set_message(format!(
                                            "{domain} · {} URLs",
                                            fmt_count(url_count)
                                        ));
                                    }
                                    provider_bar.tick();
                                    if partial && verbose && !silent {
                                        eprintln!(
                                            "Warning: partial results for {domain} from {provider_name}: a request failed mid-fetch; returning {url_count} URL(s) collected so far"
                                        );
                                    }
                                } else {
                                    tick_aggregate(
                                        &provider_bar,
                                        done_n,
                                        total,
                                        url_total.load(Ordering::Relaxed),
                                        no_progress,
                                        silent,
                                    );
                                }

                                completion_ctx.track(&domain);

                                if verbose && !silent {
                                    println!(
                                        "  - {provider_name}: Found {url_count} URLs for {domain}"
                                    );
                                }
                            }
                            Err(e) => {
                                err_total.fetch_add(1, Ordering::Relaxed);

                                {
                                    let mut s = lock_ignore_poison(&stats);
                                    s[original_idx].error_count += 1;
                                    s[original_idx].elapsed += fetch_elapsed;
                                }

                                let done_n = done.fetch_add(1, Ordering::Relaxed) + 1;
                                if rich {
                                    provider_bar.set_style(provider_error_style());
                                    provider_bar.set_prefix(format!("✗ {provider_name:<16}"));
                                    provider_bar
                                        .set_message(format!("{domain} · {}", short_error(&e)));
                                    provider_bar.tick();
                                } else {
                                    tick_aggregate(
                                        &provider_bar,
                                        done_n,
                                        total,
                                        url_total.load(Ordering::Relaxed),
                                        no_progress,
                                        silent,
                                    );
                                }

                                completion_ctx.track(&domain);

                                if verbose && !silent {
                                    eprintln!(
                                        "Error fetching URLs for {domain} from {provider_name}: {e}"
                                    );
                                }
                            }
                        }
                    }
                })
                .buffer_unordered(effective_parallel)
                .collect::<Vec<()>>()
                .await;

            // Freeze this provider's line on a one-line summary that reflects
            // what actually happened across all of its domains.
            let provider_bar = summary_bar;
            let provider_name = summary_name;
            let provider_url_total = summary_urls.load(Ordering::Relaxed);
            let provider_err_total = summary_errs.load(Ordering::Relaxed);
            let provider_partial_total = summary_partials.load(Ordering::Relaxed);
            if provider_url_total == 0 && provider_err_total > 0 {
                provider_bar.set_style(provider_error_style());
                provider_bar.set_prefix(format!("✗ {provider_name:<16}"));
                provider_bar
                    .finish_with_message(format!("all {provider_err_total} fetch(es) failed"));
            } else {
                // A partial anywhere keeps the line amber so the run doesn't
                // read as a clean, complete success at a glance.
                let glyph = if provider_partial_total > 0 {
                    "◐"
                } else {
                    "✓"
                };
                provider_bar.set_style(if provider_partial_total > 0 {
                    provider_partial_style()
                } else {
                    provider_success_style()
                });
                provider_bar.set_prefix(format!("{glyph} {provider_name:<16}"));
                let mut summary = format!("{} URLs", fmt_count(provider_url_total));
                if provider_partial_total > 0 {
                    summary.push_str(&format!(" · {provider_partial_total} partial"));
                }
                if provider_err_total > 0 {
                    summary.push_str(&format!(" · {provider_err_total} error(s)"));
                }
                provider_bar.finish_with_message(summary);
            }

            if verbose && !silent {
                println!("Provider {provider_name} has completed processing all domains");
            }
        });

        provider_futures.push(provider_future);
    }

    // Wait for all provider tasks to finish, honouring both --max-time and a
    // Ctrl-C interrupt. Abort handles are grabbed up front so either trigger can
    // cancel in-flight tasks while we keep whatever URLs they have already
    // pushed into the shared map — an interrupted run still produces output and
    // a summary instead of dying with nothing.
    let abort_handles: Vec<_> = provider_futures.iter().map(|h| h.abort_handle()).collect();
    let join_future = join_all(provider_futures);
    let deadline = (args.max_time > 0).then(|| std::time::Duration::from_secs(args.max_time));

    enum RunEnd {
        Completed,
        TimedOut,
        Interrupted,
    }

    let run_end = {
        tokio::pin!(join_future);
        // A deadline that simply never fires when --max-time isn't set.
        let timeout = async {
            match deadline {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(timeout);
        tokio::select! {
            _ = &mut join_future => RunEnd::Completed,
            _ = &mut timeout => RunEnd::TimedOut,
            // First Ctrl-C becomes a graceful stop. If signal registration
            // fails we fall back to never firing, so the run isn't spuriously
            // marked interrupted.
            _ = async {
                if tokio::signal::ctrl_c().await.is_err() {
                    std::future::pending::<()>().await;
                }
            } => RunEnd::Interrupted,
        }
    };

    match &run_end {
        RunEnd::Completed => {}
        RunEnd::TimedOut => {
            for h in &abort_handles {
                h.abort();
            }
            if !args.silent {
                progress_manager.note(format!(
                    "[urx] --max-time {}s elapsed; aborting in-flight provider fetches and returning partial results",
                    deadline.map(|d| d.as_secs()).unwrap_or(0)
                ));
            }
        }
        RunEnd::Interrupted => {
            for h in &abort_handles {
                h.abort();
            }
            if !args.silent {
                progress_manager.note(
                    "[urx] interrupted (Ctrl-C); returning URLs collected so far — press Ctrl-C again to force quit",
                );
            }
            // The rest of the pipeline (output, optional testing) can still take
            // a while, so a second Ctrl-C force-quits.
            tokio::spawn(async {
                if tokio::signal::ctrl_c().await.is_ok() {
                    std::process::exit(130);
                }
            });
        }
    }

    // A timeout/interrupt leaves the provider(s) that were mid-fetch on a
    // spinning "fetching…" line; freeze them so the final display is honest.
    if !matches!(run_end, RunEnd::Completed) {
        let label = if matches!(run_end, RunEnd::TimedOut) {
            "timed out"
        } else {
            "interrupted"
        };
        for (i, bar) in provider_bars.iter().enumerate() {
            if !bar.is_finished() {
                bar.set_style(provider_partial_style());
                if let Some(name) = provider_names.get(i) {
                    bar.set_prefix(format!("◐ {name:<16}"));
                }
                bar.finish_with_message(label.to_string());
            }
        }
    }

    match run_end {
        RunEnd::Completed => overall_bar.finish_with_message("All domains processed"),
        RunEnd::TimedOut => overall_bar.finish_with_message("Stopped by --max-time deadline"),
        RunEnd::Interrupted => overall_bar.finish_with_message("Interrupted by Ctrl-C"),
    }

    // Reclaim the shared state. If tasks were aborted the inner Arc may still
    // have outstanding strong counts for a brief moment; drain via clone in
    // that case rather than panicking.
    let urls = match Arc::try_unwrap(all_urls) {
        Ok(m) => m
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        Err(arc) => lock_ignore_poison(&arc).clone(),
    };
    let stats = match Arc::try_unwrap(stats) {
        Ok(s) => s
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        Err(arc) => lock_ignore_poison(&arc).clone(),
    };
    ProviderRunResult { urls, stats }
}
