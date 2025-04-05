use futures::future::join_all;
use std::collections::HashSet;
use std::sync::Arc;
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
        println!("Adding {} provider", provider_name);
        if network_settings.include_subdomains {
            println!("Subdomain inclusion enabled for {}", provider_name);
        }
        if network_settings.proxy.is_some() {
            println!(
                "Using proxy for {}: {}",
                provider_name,
                network_settings.proxy.as_ref().unwrap()
            );
        }
        if network_settings.random_agent && !args.silent {
            println!("Random User-Agent enabled for {}", provider_name);
        }
        println!(
            "Timeout set to {} seconds for {}",
            network_settings.timeout, provider_name
        );
        println!(
            "Retries set to {} for {}",
            network_settings.retries, provider_name
        );
        println!(
            "Parallel requests set to {} for {}",
            network_settings.parallel, provider_name
        );
        if let Some(rate) = network_settings.rate_limit {
            println!(
                "Rate limit set to {} requests/second for {}",
                rate, provider_name
            );
        }
    }

    let mut provider = provider_builder();
    apply_network_settings_to_provider(&mut provider, network_settings);
    providers.push(Box::new(provider));
    provider_names.push(provider_name);
}

/// Process domains using the provided providers
pub async fn process_domains(
    domains: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    providers: &[Box<dyn Provider>],
    provider_names: &[String],
) -> HashSet<String> {
    let mut all_urls = HashSet::new();
    let total_domains = domains.len();
    let domain_bar = progress_manager.create_domain_bar(domains.len());

    for (idx, domain) in domains.into_iter().enumerate() {
        domain_bar.set_position(idx as u64);
        domain_bar.set_message(format!("Processing {}", domain));

        verbose_print(
            args,
            format!(
                "Processing domain [{}/{}]: {}",
                idx + 1,
                total_domains,
                domain
            ),
        );

        let mut tasks = Vec::new();
        let provider_bars = progress_manager.create_provider_bars(provider_names);

        let provider_bars_arc = Arc::new(provider_bars);

        for (p_idx, provider) in providers.iter().enumerate() {
            let domain_clone = domain.clone();
            let provider_clone = provider.clone_box();
            let provider_name = provider_names[p_idx].clone();
            let bars = Arc::clone(&provider_bars_arc);

            // Set initial message
            bars[p_idx].set_message(format!("Starting fetch for {}", domain_clone));

            let task = task::spawn(async move {
                let bar = &bars[p_idx];
                bar.set_message(format!("Fetching data for {}", domain_clone));
                bar.set_position(30);

                let result = match provider_clone.fetch_urls(&domain_clone).await {
                    Ok(urls) => {
                        bar.set_position(100);
                        bar.set_message(format!("Found {} URLs", urls.len()));
                        Ok(urls)
                    }
                    Err(e) => {
                        bar.set_position(100);
                        bar.set_message(format!("Error: {}", e));
                        Err(e)
                    }
                };

                (result, provider_name)
            });

            tasks.push(task);
        }

        let results = join_all(tasks).await;
        let mut domain_urls_count = 0;
        let mut provider_results = Vec::new();

        for result in results {
            match result {
                Ok((Ok(urls), provider_name)) => {
                    domain_urls_count += urls.len();
                    provider_results.push(format!("{}: {} URLs", provider_name, urls.len()));
                    for url in urls {
                        all_urls.insert(url);
                    }
                }
                Ok((Err(e), provider_name)) => {
                    provider_results.push(format!("{}: Error - {}", provider_name, e));
                    if !args.silent {
                        eprintln!(
                            "Error fetching URLs for {} from {}: {}",
                            domain, provider_name, e
                        );
                    }
                }
                Err(e) => {
                    if !args.silent {
                        eprintln!("Task error: {}", e);
                    }
                }
            }
        }

        // Complete all progress bars for this domain
        for bar in provider_bars_arc.iter() {
            bar.finish();
        }

        if args.verbose && !args.silent {
            println!("Results for {}:", domain);
            for result in &provider_results {
                println!("  - {}", result);
            }
            println!("Total: {} URLs for {}", domain_urls_count, domain);
        }
    }

    domain_bar.finish_with_message("All domains processed");
    all_urls
}
