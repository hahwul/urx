use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::task;

use crate::cli::Args;
use crate::network::{NetworkScope, NetworkSettings};
use crate::progress::ProgressManager;
use crate::providers::Provider;
use crate::utils::verbose_print;

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
            let mut completion_map = self.domain_completion.lock().unwrap();
            if let Some(count) = completion_map.get_mut(domain) {
                *count += 1;
                is_domain_complete = *count >= self.total_providers;
            }
        }

        if is_domain_complete {
            let mut count = self.processed_domains.lock().unwrap();
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
    provider.with_parallel(settings.parallel);

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

    // Create a queue for each provider
    let provider_queues: Vec<Arc<Mutex<VecDeque<String>>>> = (0..providers.len())
        .map(|_| Arc::new(Mutex::new(VecDeque::from(domains.clone()))))
        .collect();

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
    let timeout = args.timeout;
    let verbose = args.verbose;
    let silent = args.silent;
    let no_progress = args.no_progress;

    for (p_idx, (provider_clone, provider_name, original_idx)) in
        provider_data.into_iter().enumerate()
    {
        let all_urls_clone = Arc::clone(&all_urls);
        let stats_clone = Arc::clone(&stats);
        let queue = Arc::clone(&provider_queues[p_idx]);
        let provider_bar = provider_bars[original_idx].clone();

        let completion_ctx = DomainCompletionCtx {
            total_providers,
            total_domains,
            domain_completion: Arc::clone(&domain_completion),
            processed_domains: Arc::clone(&processed_domains),
            overall_bar: overall_bar.clone(),
            verbose,
            silent,
        };

        // Spawn a task for this provider
        let provider_future = task::spawn(async move {
            // Track the current domain index for this provider
            let mut current_domain_idx = 0;

            // Process all domains assigned to this provider
            loop {
                // Get the next domain from this provider's queue
                let domain = {
                    let mut queue = queue.lock().unwrap();
                    match queue.pop_front() {
                        Some(domain) => {
                            current_domain_idx += 1;
                            domain
                        }
                        None => break, // No more domains to process for this provider
                    }
                };

                // Update the progress bar message to show which domain is being processed
                provider_bar.set_message(format!(
                    "({current_domain_idx}/{total_domains}) Fetching data for {domain}"
                ));

                // Use ticker for progress visualization
                let bar_clone = provider_bar.clone();

                // Clear line after setting initial message to ensure proper positioning
                if !no_progress && !silent {
                    provider_bar.tick();
                }

                let ticker_handle = tokio::spawn(async move {
                    let start_time = std::time::Instant::now();
                    let total_duration_ms = timeout * 1000;

                    let spinner_phase_duration =
                        std::time::Duration::from_millis(total_duration_ms / 10);
                    tokio::time::sleep(spinner_phase_duration).await;

                    let progress_style = ProgressStyle::with_template(
                        "{prefix:.bold.dim} [{bar:40.green/white}] {spinner} {wide_msg}",
                    )
                    .unwrap()
                    .progress_chars("=> ")
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);

                    bar_clone.set_style(progress_style);

                    let update_interval_ms = 100;
                    let end_time = start_time + std::time::Duration::from_millis(total_duration_ms);

                    while std::time::Instant::now() < end_time {
                        let now = std::time::Instant::now();
                        let elapsed = now.duration_since(start_time).as_millis() as u64;
                        let progress = (elapsed * 100) / total_duration_ms;

                        bar_clone.set_position(progress.min(99));

                        tokio::time::sleep(std::time::Duration::from_millis(update_interval_ms))
                            .await;
                    }

                    bar_clone.set_position(100);
                });

                // Fetch URLs for this domain using this provider
                let fetch_start = std::time::Instant::now();
                let fetch_result = provider_clone.fetch_urls(&domain).await;
                let fetch_elapsed = fetch_start.elapsed();
                match fetch_result {
                    Ok(urls) => {
                        provider_bar.set_position(100);
                        provider_bar.set_message(format!(
                            "({}/{}) Found {} URLs for {}",
                            current_domain_idx,
                            total_domains,
                            urls.len(),
                            domain
                        ));
                        ticker_handle.abort();
                        provider_bar.set_style(
                            ProgressStyle::with_template(
                                "{prefix:.bold.dim} [{bar:40.green/white}] ✓ {wide_msg}",
                            )
                            .unwrap()
                            .progress_chars("=>"),
                        );

                        // Force refresh to maintain line position
                        provider_bar.tick();

                        let url_count = urls.len();

                        // Add URLs to the shared map (URL -> set of providers).
                        {
                            let mut url_map = all_urls_clone.lock().unwrap();
                            for url in urls {
                                url_map
                                    .entry(url)
                                    .or_default()
                                    .insert(provider_name.clone());
                            }
                        }

                        // Update per-provider stats.
                        {
                            let mut s = stats_clone.lock().unwrap();
                            s[original_idx].url_count += url_count;
                            s[original_idx].elapsed += fetch_elapsed;
                        }

                        completion_ctx.track(&domain);

                        if verbose && !silent {
                            println!(
                                "  - {}: Found {} URLs for {}",
                                provider_name, url_count, domain
                            );
                        }
                    }
                    Err(e) => {
                        provider_bar.set_position(100);
                        provider_bar.set_message(format!(
                            "({current_domain_idx}/{total_domains}) Error: for {domain}"
                        ));
                        ticker_handle.abort();
                        provider_bar.set_style(
                            ProgressStyle::with_template(
                                "{prefix:.bold.dim} [{bar:40.red/white}] ✗ {wide_msg}",
                            )
                            .unwrap()
                            .progress_chars("=>"),
                        );

                        // Force refresh to maintain line position
                        provider_bar.tick();

                        {
                            let mut s = stats_clone.lock().unwrap();
                            s[original_idx].error_count += 1;
                            s[original_idx].elapsed += fetch_elapsed;
                        }

                        completion_ctx.track(&domain);

                        if verbose && !silent {
                            eprintln!("Error fetching URLs for {domain} from {provider_name}: {e}");
                        }
                    }
                }

                // Get ready for the next domain if any
                if current_domain_idx < total_domains {
                    provider_bar.set_position(0); // Reset progress for next domain
                }
            }

            // This provider has finished all its domains
            if current_domain_idx >= total_domains {
                provider_bar.finish_with_message(format!(
                    "({total_domains}/{total_domains}) Completed all domains"
                ));
            }

            if verbose && !silent {
                println!("Provider {provider_name} has completed processing all domains");
            }
        });

        provider_futures.push(provider_future);
    }

    // Wait for all provider tasks to finish, honouring --max-time when set.
    // We grab abort handles up front so a timeout can cancel in-flight tasks
    // while we keep whatever URLs they've already pushed into the shared map.
    let abort_handles: Vec<_> = provider_futures.iter().map(|h| h.abort_handle()).collect();
    let join_future = join_all(provider_futures);
    let deadline = (args.max_time > 0).then(|| std::time::Duration::from_secs(args.max_time));

    let finished_within_deadline = if let Some(d) = deadline {
        match tokio::time::timeout(d, join_future).await {
            Ok(_) => true,
            Err(_) => {
                for h in abort_handles {
                    h.abort();
                }
                if !args.silent {
                    eprintln!(
                        "[urx] --max-time {}s elapsed; aborting in-flight provider fetches and returning partial results",
                        d.as_secs()
                    );
                }
                false
            }
        }
    } else {
        join_future.await;
        true
    };

    if finished_within_deadline {
        overall_bar.finish_with_message("All domains processed");
    } else {
        overall_bar.finish_with_message("Stopped by --max-time deadline");
    }

    // Reclaim the shared state. If tasks were aborted the inner Arc may still
    // have outstanding strong counts for a brief moment; drain via clone in
    // that case rather than panicking.
    let urls = match Arc::try_unwrap(all_urls) {
        Ok(m) => m.into_inner().unwrap(),
        Err(arc) => arc.lock().unwrap().clone(),
    };
    let stats = match Arc::try_unwrap(stats) {
        Ok(s) => s.into_inner().unwrap(),
        Err(arc) => arc.lock().unwrap().clone(),
    };
    ProviderRunResult { urls, stats }
}
