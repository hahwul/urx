use futures::future::join_all;
use indicatif::ProgressStyle;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::task;

use crate::cli::Args;
use crate::network::{NetworkScope, NetworkSettings};
use crate::progress::ProgressManager;
use crate::providers::Provider;
use crate::utils::verbose_print;

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
    provider_name: String,
    provider_builder: impl FnOnce() -> T,
) {
    if args.verbose && !args.silent {
        let mut config_info = vec![
            format!("Adding {provider_name} provider"),
            format!("  Timeout: {} seconds", network_settings.timeout),
            format!("  Retries: {}", network_settings.retries),
            format!("  Parallel requests: {}", network_settings.parallel),
        ];

        if network_settings.include_subdomains {
            config_info.push("  Subdomain inclusion: enabled".to_string());
        }

        if let Some(proxy) = &network_settings.proxy {
            config_info.push(format!("  Proxy: {}", proxy));
        }

        if network_settings.random_agent {
            config_info.push("  Random User-Agent: enabled".to_string());
        }

        if let Some(rate) = network_settings.rate_limit {
            config_info.push(format!("  Rate limit: {} requests/second", rate));
        }

        println!("{}", config_info.join("\n"));
    }

    let mut provider = provider_builder();
    apply_network_settings_to_provider(&mut provider, network_settings);
    providers.push(Box::new(provider));
    provider_names.push(provider_name);
}

/// Process domains using a provider-based concurrency pattern
pub async fn process_domains(
    domains: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    providers: &[Box<dyn Provider>],
    provider_names: &[String],
) -> HashSet<String> {
    // Create a shared set to collect all URLs
    let all_urls = Arc::new(Mutex::new(HashSet::new()));
    let total_domains = domains.len();
    let total_providers = providers.len();

    // Create a progress bar for overall progress
    let overall_bar = progress_manager.create_domain_bar(total_domains);
    overall_bar.set_message("Processing domains");

    // Create a shared counter for processed domains
    let processed_domains = Arc::new(Mutex::new(0usize));

    // Create provider bars - one bar per provider
    let provider_bars = progress_manager.create_provider_bars(provider_names);

    // Create a queue for each provider
    let provider_queues: Vec<Arc<Mutex<Vec<String>>>> = (0..providers.len())
        .map(|_| Arc::new(Mutex::new(domains.clone())))
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
        let processed_domains_clone = Arc::clone(&processed_domains);
        let queue = Arc::clone(&provider_queues[p_idx]);
        let domain_completion_clone = Arc::clone(&domain_completion);
        let overall_bar_clone = overall_bar.clone();
        let provider_bar = provider_bars[original_idx].clone();

        // Spawn a task for this provider
        let provider_future = task::spawn(async move {
            // Track the current domain index for this provider
            let mut current_domain_idx = 0;

            // Process all domains assigned to this provider
            loop {
                // Get the next domain from this provider's queue
                let domain = {
                    let mut queue = queue.lock().unwrap();
                    if queue.is_empty() {
                        break; // No more domains to process for this provider
                    }
                    current_domain_idx += 1;
                    queue.remove(0)
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
                let result = match provider_clone.fetch_urls(&domain).await {
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

                        // Add URLs to the shared set
                        {
                            let mut url_set = all_urls_clone.lock().unwrap();
                            for url in &urls {
                                url_set.insert(url.clone());
                            }
                        }

                        // Mark this provider as completed for this domain
                        let mut is_domain_complete = false;
                        {
                            let mut completion_map = domain_completion_clone.lock().unwrap();
                            if let Some(count) = completion_map.get_mut(&domain) {
                                *count += 1;
                                is_domain_complete = *count >= total_providers;
                            }
                        }

                        // If all providers for this domain are done, update domain counter
                        if is_domain_complete {
                            let mut count = processed_domains_clone.lock().unwrap();
                            *count += 1;
                            overall_bar_clone.set_position(*count as u64);
                            overall_bar_clone.set_message(format!(
                                "Completed {}/{} domains",
                                *count, total_domains
                            ));

                            if verbose && !silent {
                                println!(
                                    "Domain completed: {} ({}/{})",
                                    domain, *count, total_domains
                                );
                            }
                        }

                        if verbose && !silent {
                            println!(
                                "  - {}: Found {} URLs for {}",
                                provider_name,
                                urls.len(),
                                domain
                            );
                        }

                        Ok(urls.len())
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

                        // Mark this provider as completed for this domain
                        let mut is_domain_complete = false;
                        {
                            let mut completion_map = domain_completion_clone.lock().unwrap();
                            if let Some(count) = completion_map.get_mut(&domain) {
                                *count += 1;
                                is_domain_complete = *count >= total_providers;
                            }
                        }

                        if is_domain_complete {
                            let mut count = processed_domains_clone.lock().unwrap();
                            *count += 1;
                            overall_bar_clone.set_position(*count as u64);
                            overall_bar_clone.set_message(format!(
                                "Completed {}/{} domains",
                                *count, total_domains
                            ));

                            if verbose && !silent {
                                println!(
                                    "Domain completed: {} ({}/{})",
                                    domain, *count, total_domains
                                );
                            }
                        }

                        if verbose && !silent {
                            eprintln!("Error fetching URLs for {domain} from {provider_name}: {e}");
                        }

                        Err(e.to_string())
                    }
                };

                if let Err(err) = result {
                    if verbose && !silent {
                        println!("  - {provider_name}: Error - {err} for {domain}");
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

    // Wait for all provider tasks to finish
    join_all(provider_futures).await;

    overall_bar.finish_with_message("All domains processed");

    // Return the collected URLs
    Arc::try_unwrap(all_urls).unwrap().into_inner().unwrap()
}
