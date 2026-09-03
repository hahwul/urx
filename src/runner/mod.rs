use futures::future::join_all;
use futures::stream::{self, StreamExt};
use indicatif::ProgressBar;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::task;

use crate::cli::Args;
use crate::network::{NetworkScope, NetworkSettings};
use crate::output::StreamSink;
use crate::progress::{
    provider_error_style, provider_partial_style, provider_running_style, provider_success_style,
    Notifier, ProgressManager, ProgressReporter, StopSignal,
};
#[cfg(test)]
use crate::providers::UrlRecord;
use crate::providers::{CaptureMeta, Provider};
use crate::utils::verbose_print;

/// How long a fetch gets, after the run has asked it to stop, to come back
/// with the URLs it has already collected before it is hard-cancelled.
///
/// Cancelling drops a paginating provider's whole in-memory result set, so this
/// window is what makes `--max-time` keep its promise to "proceed with whatever
/// URLs have been collected so far". Kept short: it is time spent past the
/// deadline the user asked for, and a provider only has to notice the flag
/// between two requests.
const STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

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
    notifier: Notifier,
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
                self.notifier.note(format!(
                    "Domain completed: {} ({}/{})",
                    domain, *count, self.total_domains
                ));
            }
        }

        is_domain_complete
    }
}

/// Helper function to apply network settings to a provider
pub fn apply_network_settings_to_provider(provider: &mut dyn Provider, settings: &NetworkSettings) {
    // `--subs` is a *search scope* option, not a network setting: it decides
    // which hosts the archive query asks for. `--network-scope testers` narrows
    // where proxy/timeout/TLS/rate-limit apply, and used to take subdomain
    // inclusion down with it — so `urx example.com --subs --network-scope
    // testers` silently queried the apex only.
    provider.with_subdomains(settings.include_subdomains);

    // Skip applying settings if network scope doesn't include providers
    if settings.scope == NetworkScope::Testers {
        return;
    }

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

        // stderr: stdout is the URL list. This runs before any progress bar is
        // drawn (providers are built first), so a plain write is safe here.
        eprintln!("{}", config_info.join("\n"));
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
    /// Number of domain fetches that returned incomplete (partial) results,
    /// including the ones cut off by `--max-time` or Ctrl-C.
    pub partial_count: usize,
    /// Total wall-clock time spent in fetch_urls across domains. For an
    /// [`aborted`] provider this is at least the time the run actually spent
    /// waiting on it, not the (zero) time its unfinished fetches recorded.
    ///
    /// [`aborted`]: ProviderStats::aborted
    pub elapsed: std::time::Duration,
    /// The provider was still fetching when `--max-time` or Ctrl-C ended the
    /// run, so every other number in this row is a floor, not a total.
    pub aborted: bool,
}

/// Everything the run learned about one URL: which providers reported it, and
/// the archive metadata they carried, folded together across every provider.
#[derive(Debug, Default, Clone)]
pub struct UrlEntry {
    /// Provider names that reported this URL.
    pub sources: HashSet<String>,
    /// Capture metadata merged over every provider that reported it. Empty for
    /// providers that have no capture index, and for cache hits (the cache
    /// stores URLs only).
    pub meta: CaptureMeta,
}

impl UrlEntry {
    /// Fold one provider's view of this URL in: record the provider, and merge
    /// whatever capture metadata it carried (nothing, for a provider with no
    /// capture index).
    pub fn absorb(&mut self, source: &str, meta: &CaptureMeta) {
        self.sources.insert(source.to_string());
        self.meta.merge(meta);
    }
}

/// Result of a provider run: URLs mapped to what the run learned about them,
/// plus per-provider stats indexed in the same order as `provider_names`.
#[derive(Debug, Default)]
pub struct ProviderRunResult {
    pub urls: HashMap<String, UrlEntry>,
    pub stats: Vec<ProviderStats>,
}

/// Process domains using a provider-based concurrency pattern.
///
/// Returns each discovered URL along with the set of providers that reported
/// it. Order within each source set is preserved by the caller via sort+dedup.
/// `stream`, when present, receives each provider's URLs the moment they land
/// and writes the ones that survive filtering. The shared `all_urls` map is
/// then only needed for the batch output path, so streaming runs skip filling
/// it and avoid holding the entire crawl in memory.
pub async fn process_domains(
    domains: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    providers: &[Box<dyn Provider>],
    provider_names: &[String],
    stream: Option<Arc<StreamSink>>,
) -> ProviderRunResult {
    // Map URL -> the providers that reported it plus their capture metadata.
    let all_urls: Arc<Mutex<HashMap<String, UrlEntry>>> = Arc::new(Mutex::new(HashMap::new()));
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

    // Per-provider bookkeeping the *outer* task needs after an abort: how many
    // domains each provider actually got through, and whether it ran to
    // completion at all. Both live outside the spawned task because a task that
    // `--max-time` cancels never comes back to report them.
    let done_counters: Vec<Arc<AtomicUsize>> = (0..total_providers)
        .map(|_| Arc::new(AtomicUsize::new(0)))
        .collect();
    let finished_flags: Vec<Arc<AtomicBool>> = (0..total_providers)
        .map(|_| Arc::new(AtomicBool::new(false)))
        .collect();
    let run_start = std::time::Instant::now();
    let notifier = progress_manager.notifier();
    // Raised when --max-time or Ctrl-C ends the run, so a provider mid-cursor
    // can return what it has instead of losing it to a cancelled future.
    let stop_signal = StopSignal::default();

    for (provider_clone, provider_name, original_idx) in provider_data.into_iter() {
        let all_urls = Arc::clone(&all_urls);
        let stream = stream.clone();
        let stats = Arc::clone(&stats);
        let provider_bar = provider_bars[original_idx].clone();
        let domains = domains.clone();
        let notifier = notifier.clone();
        let stop_signal = stop_signal.clone();
        let done = Arc::clone(&done_counters[original_idx]);
        let finished = Arc::clone(&finished_flags[original_idx]);

        // Shared so each concurrent domain future can mark domain completion
        // against the run-wide progress without contending on a &mut.
        let completion_ctx = Arc::new(DomainCompletionCtx {
            total_providers,
            total_domains,
            domain_completion: Arc::clone(&domain_completion),
            processed_domains: Arc::clone(&processed_domains),
            overall_bar: overall_bar.clone(),
            notifier: notifier.clone(),
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
            let total = domains.len();

            // Handles retained for the summary after the stream consumes the
            // per-domain clones.
            let summary_bar = provider_bar.clone();
            let summary_name = provider_name.clone();
            let summary_notifier = notifier.clone();
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
                    let stream = stream.clone();
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
                    let notifier = notifier.clone();
                    let stop_signal = stop_signal.clone();

                    async move {
                        let prefix = format!("{domain} · ");

                        // Rich mode: the reporter drives the visible line with
                        // live page-by-page detail and re-arms the spinner.
                        // Aggregate mode: it only carries the partial-result
                        // flag (a hidden bar) so concurrent domains don't fight
                        // over the single line; --silent suppresses it entirely.
                        // A reporter is handed in even under --silent: besides
                        // the (then hidden) line it carries the partial flag and
                        // the run-wide stop signal, and a provider that cannot
                        // see the stop signal loses everything it has paged in
                        // when --max-time cancels it.
                        let reporter = if rich && !silent {
                            provider_bar.set_style(provider_running_style());
                            provider_bar.set_prefix(format!("{provider_name:<16}"));
                            provider_bar.reset_elapsed();
                            provider_bar.set_message(format!("{prefix}fetching…"));
                            if !no_progress {
                                provider_bar.tick();
                            }
                            ProgressReporter::new(provider_bar.clone(), prefix.clone())
                        } else {
                            ProgressReporter::new(ProgressBar::hidden(), prefix.clone())
                        };
                        let reporter = Some(reporter.with_stop_signal(stop_signal.clone()));

                        // Fetch URLs for this domain using this provider.
                        let fetch_start = std::time::Instant::now();
                        let fetch_result = provider
                            .fetch_urls_with_progress(&domain, reporter.clone())
                            .await;
                        let fetch_elapsed = fetch_start.elapsed();
                        match fetch_result {
                            Ok(records) => {
                                let url_count = records.len();
                                url_total.fetch_add(url_count, Ordering::Relaxed);

                                // A *partial* result (e.g. a page failed
                                // mid-pagination) is surfaced as a distinct,
                                // warned state so a truncated crawl is never
                                // mistaken for a clean success. A result that
                                // lands after the run asked everyone to stop
                                // counts too: the provider may have cut its
                                // cursor walk short to hand back what it had,
                                // so it cannot be reported as complete.
                                let partial = reporter
                                    .as_ref()
                                    .is_some_and(|r| r.is_partial() || r.stop_requested());
                                if partial {
                                    partial_total.fetch_add(1, Ordering::Relaxed);
                                }

                                // Hand this batch to the streaming sink, or —
                                // in batch mode — accumulate it (URL -> the
                                // providers that reported it).
                                match &stream {
                                    Some(sink) => {
                                        if let Err(e) = sink.emit(&records) {
                                            if !silent {
                                                notifier.note(format!(
                                                    "Error writing streamed output: {e}"
                                                ));
                                            }
                                        }
                                    }
                                    None => {
                                        let mut url_map = lock_ignore_poison(&all_urls);
                                        for record in records {
                                            url_map
                                                .entry(record.url)
                                                .or_default()
                                                .absorb(&provider_name, &record.meta);
                                        }
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
                                        notifier.note(format!(
                                            "Warning: partial results for {domain} from {provider_name}: the fetch stopped early; returning {url_count} URL(s) collected so far"
                                        ));
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
                                    notifier.note(format!(
                                        "  - {provider_name}: Found {url_count} URLs for {domain}"
                                    ));
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
                                    notifier.note(format!(
                                        "Error fetching URLs for {domain} from {provider_name}: {e}"
                                    ));
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
                summary_notifier.note(format!(
                    "Provider {provider_name} has completed processing all domains"
                ));
            }

            // Last thing the task does: whoever is watching the deadline uses
            // this to tell "ran to completion" from "cancelled mid-flight".
            finished.store(true, Ordering::Relaxed);
        });

        provider_futures.push(provider_future);
    }

    // Wait for all provider tasks to finish, honouring both --max-time and a
    // Ctrl-C interrupt. Abort handles are grabbed up front so either trigger can
    // cancel in-flight tasks while we keep whatever URLs they have already
    // pushed into the shared map — an interrupted run still produces output and
    // a summary instead of dying with nothing.
    let abort_handles: Vec<_> = provider_futures.iter().map(|h| h.abort_handle()).collect();
    let deadline = (args.max_time > 0).then(|| std::time::Duration::from_secs(args.max_time));

    enum RunEnd {
        Completed,
        TimedOut,
        Interrupted,
    }

    // Pinned in the enclosing scope, not inside the `select!` block: after the
    // deadline fires we still need the same join future to await the graceful
    // wind-down below.
    let join_future = join_all(provider_futures);
    tokio::pin!(join_future);

    let run_end = {
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

    if !matches!(run_end, RunEnd::Completed) {
        // Say why the run is wrapping up before waiting on it, so the grace
        // window below never looks like a hang.
        if !args.silent {
            match run_end {
                RunEnd::TimedOut => progress_manager.note(format!(
                    "[urx] --max-time {}s elapsed; stopping in-flight provider fetches and returning partial results",
                    deadline.map(|d| d.as_secs()).unwrap_or(0)
                )),
                _ => progress_manager.note(
                    "[urx] interrupted (Ctrl-C); returning URLs collected so far — press Ctrl-C again to force quit",
                ),
            }
        }

        // The rest of the pipeline (output, optional testing) can still take a
        // while, so a second Ctrl-C force-quits. Armed before the grace window
        // so it also covers the wait itself.
        if matches!(run_end, RunEnd::Interrupted) {
            tokio::spawn(async {
                if tokio::signal::ctrl_c().await.is_ok() {
                    std::process::exit(130);
                }
            });
        }

        // Cancelling a provider task drops everything it has buffered but not
        // yet returned, and a paginating provider buffers the whole crawl until
        // its last page — so a hard abort here threw away exactly the results
        // --max-time promises to keep. Raise the cooperative stop signal and
        // give in-flight fetches a brief window to return what they already
        // have; only what is still running when it closes gets cancelled.
        stop_signal.request_stop();
        let _ = tokio::time::timeout(STOP_GRACE, &mut join_future).await;
        for h in &abort_handles {
            h.abort();
        }

        // A provider cancelled mid-fetch never reached the stats update that
        // runs after a fetch resolves, so its row stayed at 0 urls / 0 partial
        // / 0 errors / 0ms — a run that spent 90 seconds on it read as an
        // instant, clean pass. Charge it the wall time the run actually spent
        // and flag it, counting each domain it never finished as partial.
        let wall = run_start.elapsed();
        {
            let mut s = lock_ignore_poison(&stats);
            for (idx, entry) in s.iter_mut().enumerate() {
                if finished_flags[idx].load(Ordering::Relaxed) {
                    continue;
                }
                entry.aborted = true;
                entry.elapsed = entry.elapsed.max(wall);
                let completed = done_counters[idx].load(Ordering::Relaxed);
                entry.partial_count += total_domains.saturating_sub(completed);
            }
        }

        // A timeout/interrupt leaves the provider(s) that were mid-fetch on a
        // spinning "fetching…" line; freeze them so the final display is honest.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::UrlFilter;
    use crate::output;
    use crate::test_support::{build_test_args, MockProvider};
    use crate::utils::UrlTransformer;
    use std::future::Future;
    use std::pin::Pin;

    /// A provider that paginates: it collects one URL every `step`, and honours
    /// the run-wide stop signal the way a real cursor-walking provider is meant
    /// to — flag the result partial and return what it already has, rather than
    /// let a cancelled future take the whole buffer with it.
    #[derive(Clone)]
    struct PaginatingProvider {
        step: std::time::Duration,
        pages: usize,
    }

    impl Provider for PaginatingProvider {
        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        fn fetch_urls<'a>(
            &'a self,
            domain: &'a str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<UrlRecord>>> + Send + 'a>> {
            self.fetch_urls_with_progress(domain, None)
        }

        fn fetch_urls_with_progress<'a>(
            &'a self,
            domain: &'a str,
            reporter: Option<ProgressReporter>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<UrlRecord>>> + Send + 'a>> {
            let step = self.step;
            let pages = self.pages;
            let domain = domain.to_string();
            Box::pin(async move {
                let mut urls = Vec::new();
                for page in 0..pages {
                    tokio::time::sleep(step).await;
                    urls.push(UrlRecord::bare(format!("https://{domain}/page{page}")));
                    if let Some(r) = &reporter {
                        if r.stop_requested() {
                            r.mark_partial();
                            break;
                        }
                    }
                }
                Ok(urls)
            })
        }

        fn with_subdomains(&mut self, _include: bool) {}
        fn with_proxy(&mut self, _proxy: Option<String>) {}
        fn with_proxy_auth(&mut self, _auth: Option<String>) {}
        fn with_timeout(&mut self, _seconds: u64) {}
        fn with_retries(&mut self, _count: u32) {}
        fn with_random_agent(&mut self, _enabled: bool) {}
        fn with_insecure(&mut self, _enabled: bool) {}
        fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
    }

    /// A provider that records what `with_subdomains` was told.
    #[derive(Clone, Default)]
    struct SubdomainRecordingProvider {
        include_subdomains: bool,
    }

    impl Provider for SubdomainRecordingProvider {
        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        fn fetch_urls<'a>(
            &'a self,
            _domain: &'a str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<UrlRecord>>> + Send + 'a>> {
            Box::pin(async move { Ok(vec![]) })
        }

        fn with_subdomains(&mut self, include: bool) {
            self.include_subdomains = include;
        }
        fn with_proxy(&mut self, _proxy: Option<String>) {}
        fn with_proxy_auth(&mut self, _auth: Option<String>) {}
        fn with_timeout(&mut self, _seconds: u64) {}
        fn with_retries(&mut self, _count: u32) {}
        fn with_random_agent(&mut self, _enabled: bool) {}
        fn with_insecure(&mut self, _enabled: bool) {}
        fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
    }

    /// A provider returning canned records, so a test can hand two providers
    /// different capture metadata for the same URL.
    #[derive(Clone)]
    struct RecordingProvider {
        records: Vec<UrlRecord>,
    }

    impl Provider for RecordingProvider {
        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        fn fetch_urls<'a>(
            &'a self,
            _domain: &'a str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<UrlRecord>>> + Send + 'a>> {
            let records = self.records.clone();
            Box::pin(async move { Ok(records) })
        }

        fn with_subdomains(&mut self, _include: bool) {}
        fn with_proxy(&mut self, _proxy: Option<String>) {}
        fn with_proxy_auth(&mut self, _auth: Option<String>) {}
        fn with_timeout(&mut self, _seconds: u64) {}
        fn with_retries(&mut self, _count: u32) {}
        fn with_random_agent(&mut self, _enabled: bool) {}
        fn with_insecure(&mut self, _enabled: bool) {}
        fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
    }

    /// Two providers reporting the same URL must produce one entry whose
    /// metadata spans both — the archive with the oldest capture supplies
    /// `first_seen`, the one with the newest supplies `last_seen` and the
    /// single-valued fields.
    #[tokio::test]
    async fn test_metadata_merges_across_providers() {
        let url = "https://example.com/page";
        let old = RecordingProvider {
            records: vec![UrlRecord::new(
                url.to_string(),
                CaptureMeta::capture(
                    Some("19990101000000"),
                    Some("text/plain"),
                    Some("404"),
                    Some("OLD"),
                ),
            )],
        };
        let new = RecordingProvider {
            records: vec![UrlRecord::new(
                url.to_string(),
                CaptureMeta::capture(
                    Some("20240101000000"),
                    Some("text/html"),
                    Some("200"),
                    Some("NEW"),
                ),
            )],
        };

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(old), Box::new(new)];
        let provider_names = vec!["arquivo".to_string(), "wayback".to_string()];

        let result = process_domains(
            vec!["example.com".to_string()],
            &build_test_args(),
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;

        assert_eq!(result.urls.len(), 1);
        let entry = &result.urls[url];
        assert_eq!(
            entry
                .sources
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            ["arquivo".to_string(), "wayback".to_string()].into()
        );
        assert_eq!(entry.meta.first_seen(), Some("19990101000000"));
        assert_eq!(entry.meta.last_seen(), Some("20240101000000"));
        assert_eq!(entry.meta.mime(), Some("text/html"));
        assert_eq!(entry.meta.archive_status(), Some("200"));
    }

    /// A provider with no capture index must not leave metadata behind: the
    /// entry has sources but nothing to report about captures.
    #[tokio::test]
    async fn test_providers_without_metadata_produce_empty_entries() {
        let provider = MockProvider::new(vec!["https://example.com/a".to_string()], false);
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];

        let result = process_domains(
            vec!["example.com".to_string()],
            &build_test_args(),
            &ProgressManager::new(true),
            &providers,
            &["otx".to_string()],
            None,
        )
        .await;

        let entry = &result.urls["https://example.com/a"];
        assert!(entry.sources.contains("otx"));
        assert!(entry.meta.is_empty());
    }

    #[tokio::test]
    async fn test_process_domains() {
        let provider = MockProvider::new(
            vec![
                "https://example.com/page1".to_string(),
                "https://example.com/page2".to_string(),
            ],
            false,
        );
        let calls = provider.calls.clone();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];
        let provider_names = vec!["MockProvider".to_string()];

        let args = build_test_args();
        let result = process_domains(
            vec!["example.com".to_string()],
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;

        // The provider was asked about exactly the requested domain.
        let calls = calls.lock().unwrap();
        assert_eq!(calls.as_slice(), ["example.com"]);

        // URLs come back attributed to the provider that reported them.
        assert_eq!(result.urls.len(), 2);
        assert!(result.urls.contains_key("https://example.com/page1"));
        assert!(result.urls.contains_key("https://example.com/page2"));
        assert!(result.urls["https://example.com/page1"]
            .sources
            .contains("MockProvider"));

        assert_eq!(result.stats.len(), 1);
        assert_eq!(result.stats[0].name, "MockProvider");
        assert_eq!(result.stats[0].url_count, 2);
        assert_eq!(result.stats[0].error_count, 0);
    }

    #[tokio::test]
    async fn test_provider_failure_is_counted_not_fatal() {
        let providers: Vec<Box<dyn Provider>> = vec![
            Box::new(MockProvider::new(vec![], true)),
            Box::new(MockProvider::new(
                vec!["https://example.com/ok".to_string()],
                false,
            )),
        ];
        let provider_names = vec!["Broken".to_string(), "Working".to_string()];

        let result = process_domains(
            vec!["example.com".to_string()],
            &build_test_args(),
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;

        // One provider failing must not lose the other's results.
        assert!(result.urls.contains_key("https://example.com/ok"));
        let broken = result.stats.iter().find(|s| s.name == "Broken").unwrap();
        assert_eq!(broken.error_count, 1);
        assert_eq!(broken.url_count, 0);
    }

    #[tokio::test]
    async fn test_parallel_processes_provider_domains_concurrently() {
        // One provider, five domains, each fetch sleeps 200ms. With --parallel 5
        // the provider's domains must be fetched concurrently — finishing in
        // ~200ms rather than the ~1s a sequential per-provider drain would take.
        // This guards the #270 fix from regressing back to single-flight.
        let provider =
            MockProvider::new(vec!["https://example.com/a".to_string()], false).with_delay_ms(200);
        let calls = provider.calls.clone();

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];
        let provider_names = vec!["MockProvider".to_string()];
        let domains: Vec<String> = ["a.com", "b.com", "c.com", "d.com", "e.com"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let mut args = build_test_args();
        args.parallel = Some(5);

        let start = std::time::Instant::now();
        let _ = process_domains(
            domains,
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;
        let elapsed = start.elapsed();

        // All five domains were fetched...
        assert_eq!(calls.lock().unwrap().len(), 5);
        // ...and concurrently: well under the ~1s a sequential drain would need.
        assert!(
            elapsed < std::time::Duration::from_millis(800),
            "expected concurrent per-provider fetches (~200ms), took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn test_parallel_one_processes_sequentially() {
        // With --parallel 1 the same five 200ms fetches must run one at a time,
        // taking ~1s. This pins the sequential (rich-UI) path so the
        // concurrency knob is honored in both directions.
        let provider =
            MockProvider::new(vec!["https://example.com/a".to_string()], false).with_delay_ms(200);
        let calls = provider.calls.clone();

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];
        let provider_names = vec!["MockProvider".to_string()];
        let domains: Vec<String> = ["a.com", "b.com", "c.com", "d.com", "e.com"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let mut args = build_test_args();
        args.parallel = Some(1);

        let start = std::time::Instant::now();
        let _ = process_domains(
            domains,
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;
        let elapsed = start.elapsed();

        assert_eq!(calls.lock().unwrap().len(), 5);
        assert!(
            elapsed >= std::time::Duration::from_millis(900),
            "expected sequential fetches (~1s) with --parallel 1, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn test_stream_emits_before_slow_provider_finishes() {
        // The whole point of --stream: a fast provider's URLs must reach the
        // consumer while a slow one is still fetching, instead of everyone
        // waiting for the slowest.
        use std::io::Write;

        #[derive(Clone, Default)]
        struct SharedBuf(Arc<Mutex<Vec<u8>>>);
        impl Write for SharedBuf {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let fast = MockProvider::new(vec!["https://example.com/fast".to_string()], false);
        let slow = MockProvider::new(vec!["https://example.com/slow".to_string()], false)
            .with_delay_ms(2_000);

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(fast), Box::new(slow)];
        let provider_names = vec!["Fast".to_string(), "Slow".to_string()];

        let buf = SharedBuf::default();
        let sink = Arc::new(
            output::StreamSink::new(
                UrlFilter::new(),
                UrlTransformer::new(),
                None,
                "plain",
                Box::new(buf.clone()),
            )
            .unwrap(),
        );

        let args = build_test_args();
        let run = tokio::spawn({
            let sink = Arc::clone(&sink);
            let providers: Vec<Box<dyn Provider>> =
                providers.iter().map(|p| p.clone_box()).collect();
            let provider_names = provider_names.clone();
            async move {
                process_domains(
                    vec!["example.com".to_string()],
                    &args,
                    &ProgressManager::new(true),
                    &providers,
                    &provider_names,
                    Some(sink),
                )
                .await
            }
        });

        // Half a second in — long before the slow provider's 2s delay elapses —
        // the fast provider's URL must already be written.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let mid_run = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(
            mid_run.contains("https://example.com/fast"),
            "fast provider's URL should be streamed while the slow one is still running, got {mid_run:?}"
        );
        assert!(
            !mid_run.contains("https://example.com/slow"),
            "slow provider should not have reported yet, got {mid_run:?}"
        );

        run.await.unwrap();
        let final_out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(
            final_out.contains("https://example.com/slow"),
            "{final_out}"
        );
        assert_eq!(sink.emitted(), 2);
    }

    #[tokio::test]
    async fn test_stream_mode_skips_the_batch_url_map() {
        // Streaming runs must not also accumulate every URL in memory — that
        // map exists only to feed the batch output path.
        use std::io::Write;

        struct Sink;
        impl Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(MockProvider::new(
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ],
            false,
        ))];
        let provider_names = vec!["Mock".to_string()];

        let sink = Arc::new(
            output::StreamSink::new(
                UrlFilter::new(),
                UrlTransformer::new(),
                None,
                "plain",
                Box::new(Sink),
            )
            .unwrap(),
        );

        let result = process_domains(
            vec!["example.com".to_string()],
            &build_test_args(),
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            Some(Arc::clone(&sink)),
        )
        .await;

        assert!(
            result.urls.is_empty(),
            "batch map should stay empty when streaming, got {:?}",
            result.urls
        );
        assert_eq!(sink.emitted(), 2);
        // Stats are still tallied so --stats keeps working.
        assert_eq!(result.stats[0].url_count, 2);
    }

    #[tokio::test]
    async fn test_max_time_aborts_slow_provider() {
        // A provider that sleeps for 5s should be cut off when max_time=1.
        let slow = MockProvider::new(vec!["https://example.com/never".to_string()], false)
            .with_delay_ms(5_000);

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(slow)];
        let provider_names = vec!["SlowProvider".to_string()];

        let mut args = build_test_args();
        args.max_time = 1;

        let started = std::time::Instant::now();
        let result = process_domains(
            vec!["example.com".to_string()],
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;
        let elapsed = started.elapsed();

        // Should bail out well before the provider's 5s sleep finishes.
        assert!(
            elapsed.as_secs() < 4,
            "expected --max-time to abort within ~1s, got {elapsed:?}"
        );
        // No URLs were produced because the provider was cut off mid-await.
        assert!(
            result.urls.is_empty(),
            "expected no URLs, got {:?}",
            result.urls
        );
    }

    #[tokio::test]
    async fn test_max_time_charges_the_aborted_provider_real_elapsed_time() {
        // Regression: a provider cancelled by --max-time never reached the
        // stats update at the end of its fetch, so `--stats` reported it as
        // "0 urls · 0 partial · 0 errors · 0ms" — a run that spent the whole
        // deadline on it read as an instant, clean pass.
        let slow = MockProvider::new(vec!["https://example.com/never".to_string()], false)
            .with_delay_ms(30_000);

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(slow)];
        let provider_names = vec!["SlowProvider".to_string()];

        let mut args = build_test_args();
        args.max_time = 1;

        let result = process_domains(
            vec!["example.com".to_string()],
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;

        let stats = &result.stats[0];
        assert!(stats.aborted, "aborted provider must be flagged: {stats:?}");
        assert!(
            stats.elapsed >= std::time::Duration::from_millis(900),
            "elapsed must be real wall time, got {:?}",
            stats.elapsed
        );
        // The one domain it never finished is reported as an incomplete result
        // rather than a silent zero.
        assert_eq!(stats.partial_count, 1, "{stats:?}");
    }

    #[tokio::test]
    async fn test_max_time_keeps_the_urls_a_provider_already_collected() {
        // Regression: --max-time is documented as "in-flight provider fetches
        // are aborted and urx proceeds with whatever URLs have been collected
        // so far", but cancelling the task dropped the provider's in-memory
        // buffer — a paginating provider accumulates the whole crawl until it
        // returns, so a timed-out run yielded *nothing*. The runner now asks
        // fetches to stop and gives them a moment to hand back what they have.
        let provider = PaginatingProvider {
            step: std::time::Duration::from_millis(100),
            pages: 10_000,
        };
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];
        let provider_names = vec!["Paginating".to_string()];

        let mut args = build_test_args();
        args.max_time = 1;

        let result = process_domains(
            vec!["example.com".to_string()],
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;

        assert!(
            !result.urls.is_empty(),
            "URLs collected before the deadline must survive it"
        );
        let stats = &result.stats[0];
        assert_eq!(stats.url_count, result.urls.len(), "{stats:?}");
        // The provider wrapped up on its own, so it isn't flagged as cancelled —
        // but the truncated result is still reported as partial, never clean.
        assert!(!stats.aborted, "{stats:?}");
        assert_eq!(stats.partial_count, 1, "{stats:?}");
        assert!(
            stats.elapsed >= std::time::Duration::from_millis(900),
            "{stats:?}"
        );
    }

    #[test]
    fn test_subdomain_inclusion_is_not_a_network_setting() {
        // Regression: --network-scope testers made
        // apply_network_settings_to_provider bail before it applied --subs, so
        // `urx example.com --subs --network-scope testers` silently queried the
        // apex only. Subdomain inclusion decides *what* is searched, not how
        // the request is made.
        let settings = NetworkSettings::new().with_subdomains(true);

        for scope in [
            NetworkScope::All,
            NetworkScope::Providers,
            NetworkScope::Testers,
        ] {
            let mut settings = settings.clone();
            settings.scope = scope.clone();
            let mut provider = SubdomainRecordingProvider::default();
            apply_network_settings_to_provider(&mut provider, &settings);
            assert!(
                provider.include_subdomains,
                "--subs must survive --network-scope {scope:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_verbose_progress_lines_never_touch_stdout() {
        // Regression: the per-domain verbose lines went out through `println!`,
        // so `urx -v > urls.txt` interleaved "Domain completed: …" into the URL
        // list a caller was piping — and, drawn straight to the terminal, they
        // also bypassed MultiProgress and desynced the live region. They must
        // go through the progress note channel (stderr) instead.
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(MockProvider::new(
            vec!["https://example.com/a".to_string()],
            false,
        ))];
        let provider_names = vec!["Mock".to_string()];

        let mut args = build_test_args();
        args.verbose = true;
        args.silent = false;

        let (progress_manager, notes) = ProgressManager::capturing();
        let _ = process_domains(
            vec!["example.com".to_string()],
            &args,
            &progress_manager,
            &providers,
            &provider_names,
            None,
        )
        .await;

        let notes = notes.lock().unwrap().join("\n");
        assert!(
            notes.contains("Mock: Found 1 URLs for example.com"),
            "per-provider verbose line should reach the note channel: {notes:?}"
        );
        assert!(
            notes.contains("Domain completed: example.com"),
            "domain completion line should reach the note channel: {notes:?}"
        );
    }

    #[tokio::test]
    async fn test_zero_timeout_does_not_panic() {
        let provider = MockProvider::new(vec!["https://example.com/page1".to_string()], false)
            .with_delay_ms(25);
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];
        let provider_names = vec!["MockProvider".to_string()];

        let mut args = build_test_args();
        args.timeout = 0;

        let result = process_domains(
            vec!["example.com".to_string()],
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            None,
        )
        .await;

        assert!(result.urls.contains_key("https://example.com/page1"));
    }
}
