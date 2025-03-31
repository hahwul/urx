use anyhow::Result;
use clap::Parser;
use futures::future::join_all;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::task;

mod cli;
mod filters;
mod output;
mod progress;
mod providers;
mod testers;
mod url_utils;

use cli::{read_domains_from_stdin, Args};
use filters::UrlFilter;
use output::create_outputter;
use progress::ProgressManager;
use providers::{CommonCrawlProvider, OTXProvider, Provider, WaybackMachineProvider};
use testers::{LinkExtractor, StatusChecker, Tester};
use url_utils::UrlTransformer;

/// Prints messages only when verbose mode is enabled
///
/// This helper function is used throughout the application to conditionally
/// print information messages based on the command-line arguments.
fn verbose_print(args: &Args, message: impl AsRef<str>) {
    if args.verbose && !args.silent {
        println!("{}", message.as_ref());
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Collect domains either from arguments or stdin
    let domains = if args.domains.is_empty() {
        read_domains_from_stdin()?
    } else {
        args.domains.clone()
    };

    if domains.is_empty() {
        if !args.silent {
            eprintln!("No domains provided. Please specify domains or pipe them through stdin.");
        }
        return Ok(());
    }

    // Initialize providers based on command-line flags
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    let mut provider_names: Vec<String> = Vec::new();

    if args.providers.iter().any(|p| p == "wayback") {
        if args.verbose && !args.silent {
            println!("Adding Wayback Machine provider");
            if args.subs {
                println!("Subdomain inclusion enabled for Wayback Machine");
            }
            if args.proxy.is_some() {
                println!(
                    "Using proxy for Wayback Machine: {}",
                    args.proxy.as_ref().unwrap()
                );
            }
        }

        let mut wayback_provider = WaybackMachineProvider::new();

        // Apply common settings to Wayback provider
        wayback_provider.with_subdomains(args.subs);
        if let Some(proxy) = &args.proxy {
            wayback_provider.with_proxy(Some(proxy.clone()));

            if let Some(auth) = &args.proxy_auth {
                wayback_provider.with_proxy_auth(Some(auth.clone()));
            }
        }

        // Apply new settings
        wayback_provider.with_timeout(args.timeout);
        wayback_provider.with_retries(args.retries);
        wayback_provider.with_random_agent(args.random_agent);
        wayback_provider.with_parallel(args.parallel);
        if let Some(rate) = args.rate_limit {
            wayback_provider.with_rate_limit(Some(rate));
        }

        if args.verbose && args.random_agent && !args.silent {
            println!("Random User-Agent enabled for Wayback Machine");
        }

        if args.verbose && !args.silent {
            println!(
                "Timeout set to {} seconds for Wayback Machine",
                args.timeout
            );
            println!("Retries set to {} for Wayback Machine", args.retries);
            println!(
                "Parallel requests set to {} for Wayback Machine",
                args.parallel
            );
            if let Some(rate) = args.rate_limit {
                println!(
                    "Rate limit set to {} requests/second for Wayback Machine",
                    rate
                );
            }
        }

        providers.push(Box::new(wayback_provider));
        provider_names.push("Wayback Machine".to_string());
    }

    if args.providers.iter().any(|p| p == "cc") {
        if args.verbose && !args.silent {
            println!("Adding Common Crawl provider with index: {}", args.cc_index);
            if args.subs {
                println!("Subdomain inclusion enabled for Common Crawl");
            }
            if args.proxy.is_some() {
                println!(
                    "Using proxy for Common Crawl: {}",
                    args.proxy.as_ref().unwrap()
                );
            }
        }

        let mut cc_provider = CommonCrawlProvider::with_index(args.cc_index.clone());

        // Apply common settings to Common Crawl provider
        cc_provider.with_subdomains(args.subs);
        if let Some(proxy) = &args.proxy {
            cc_provider.with_proxy(Some(proxy.clone()));

            if let Some(auth) = &args.proxy_auth {
                cc_provider.with_proxy_auth(Some(auth.clone()));
            }
        }

        // Apply new settings
        cc_provider.with_timeout(args.timeout);
        cc_provider.with_retries(args.retries);
        cc_provider.with_random_agent(args.random_agent);
        cc_provider.with_parallel(args.parallel);
        if let Some(rate) = args.rate_limit {
            cc_provider.with_rate_limit(Some(rate));
        }

        if args.verbose && args.random_agent && !args.silent {
            println!("Random User-Agent enabled for Common Crawl");
        }

        if args.verbose && !args.silent {
            println!("Timeout set to {} seconds for Common Crawl", args.timeout);
            println!("Retries set to {} for Common Crawl", args.retries);
            println!(
                "Parallel requests set to {} for Common Crawl",
                args.parallel
            );
            if let Some(rate) = args.rate_limit {
                println!(
                    "Rate limit set to {} requests/second for Common Crawl",
                    rate
                );
            }
        }

        providers.push(Box::new(cc_provider));
        provider_names.push(format!("Common Crawl ({})", args.cc_index));
    }

    if args.providers.iter().any(|p| p == "otx") {
        if args.verbose && !args.silent {
            println!("Adding OTX provider");
            if args.subs {
                println!("Subdomain inclusion enabled for OTX");
            }
            if args.proxy.is_some() {
                println!("Using proxy for OTX: {}", args.proxy.as_ref().unwrap());
            }
        }

        let mut otx_provider = OTXProvider::new();

        // Apply common settings to OTX provider
        otx_provider.with_subdomains(args.subs);
        if let Some(proxy) = &args.proxy {
            otx_provider.with_proxy(Some(proxy.clone()));

            if let Some(auth) = &args.proxy_auth {
                otx_provider.with_proxy_auth(Some(auth.clone()));
            }
        }

        // Apply new settings
        otx_provider.with_timeout(args.timeout);
        otx_provider.with_retries(args.retries);
        otx_provider.with_random_agent(args.random_agent);
        otx_provider.with_parallel(args.parallel);
        if let Some(rate) = args.rate_limit {
            otx_provider.with_rate_limit(Some(rate));
        }

        if args.verbose && args.random_agent && !args.silent {
            println!("Random User-Agent enabled for OTX");
        }

        if args.verbose && !args.silent {
            println!("Timeout set to {} seconds for OTX", args.timeout);
            println!("Retries set to {} for OTX", args.retries);
            println!("Parallel requests set to {} for OTX", args.parallel);
            if let Some(rate) = args.rate_limit {
                println!("Rate limit set to {} requests/second for OTX", rate);
            }
        }

        providers.push(Box::new(otx_provider));
        provider_names.push("OTX".to_string());
    }

    if providers.is_empty() {
        if !args.silent {
            eprintln!("Error: No valid providers specified. Please use --providers with valid provider names (wayback, cc, otx)");
        }
        return Ok(());
    }

    // Check for progress bar options
    let progress_check = args.no_progress || args.silent;

    // Setup progress bars
    let progress_manager = ProgressManager::new(progress_check);
    let domain_bar = progress_manager.create_domain_bar(domains.len());

    // Process each domain
    let mut all_urls = HashSet::new();
    let total_domains = domains.len();

    for (idx, domain) in domains.into_iter().enumerate() {
        domain_bar.set_position(idx as u64);
        domain_bar.set_message(format!("Processing {}", domain));

        verbose_print(
            &args,
            format!(
                "Processing domain [{}/{}]: {}",
                idx + 1,
                total_domains,
                domain
            ),
        );

        let mut tasks = Vec::new();
        let provider_bars = progress_manager.create_provider_bars(&provider_names);

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

    // Create a progress bar for filtering
    let filter_bar = if !args.extensions.is_empty()
        || !args.patterns.is_empty()
        || !args.exclude_extensions.is_empty()
        || !args.exclude_patterns.is_empty()
        || args.min_length.is_some()
        || args.max_length.is_some()
    {
        let bar = progress_manager.create_filter_bar();
        bar.set_message("Applying filters to URLs...");
        Some(bar)
    } else {
        None
    };

    // Apply URL filtering
    let mut url_filter = UrlFilter::new();

    // Apply presets if specified
    if !args.preset.is_empty() {
        url_filter.apply_presets(&args.preset);
    }

    // Apply additional filters (will be combined with preset filters)
    url_filter
        .with_extensions(args.extensions.clone())
        .with_exclude_extensions(args.exclude_extensions.clone())
        .with_patterns(args.patterns.clone())
        .with_exclude_patterns(args.exclude_patterns.clone())
        .with_min_length(args.min_length)
        .with_max_length(args.max_length);

    let sorted_urls = url_filter.apply_filters(&all_urls);

    if let Some(bar) = filter_bar {
        bar.finish_with_message(format!("Filtered to {} URLs", sorted_urls.len()));
    }

    if args.verbose && !args.silent {
        println!("Total unique URLs after filtering: {}", sorted_urls.len());
    }

    // Apply URL transformation based on display options
    let transform_bar = if args.merge_endpoint
        || args.show_only_host
        || args.show_only_path
        || args.show_only_param
    {
        let bar = progress_manager.create_transform_bar();
        bar.set_message("Applying URL transformations...");
        Some(bar)
    } else {
        None
    };

    // Apply URL transformations
    let mut url_transformer = UrlTransformer::new();
    url_transformer
        .with_merge_endpoint(args.merge_endpoint)
        .with_show_only_host(args.show_only_host)
        .with_show_only_path(args.show_only_path)
        .with_show_only_param(args.show_only_param);

    let transformed_urls = url_transformer.transform(sorted_urls);

    if let Some(bar) = transform_bar {
        bar.finish_with_message(format!("Transformed to {} URLs", transformed_urls.len()));
    }

    // Output results
    let outputter = create_outputter(&args.format);

    // Apply testers if requested
    let mut final_urls = Vec::with_capacity(transformed_urls.len());

    if args.check_status || args.extract_links {
        verbose_print(&args, "Applying testing options...");

        // Create progress bar for testing
        let test_bar = progress_manager.create_test_bar(transformed_urls.len());
        test_bar.set_message("Preparing URL testing...");

        // Initialize appropriate testers
        let mut testers: Vec<Box<dyn Tester>> = Vec::new();

        if args.check_status {
            verbose_print(&args, "Checking HTTP status codes for URLs");

            // Apply network settings
            let mut status_checker = StatusChecker::new();

            // Apply network settings
            status_checker.with_timeout(args.timeout);
            status_checker.with_retries(args.retries);
            status_checker.with_random_agent(args.random_agent);

            if let Some(proxy) = &args.proxy {
                status_checker.with_proxy(Some(proxy.clone()));

                if let Some(auth) = &args.proxy_auth {
                    status_checker.with_proxy_auth(Some(auth.clone()));
                }
            }

            testers.push(Box::new(status_checker));
        }

        if args.extract_links {
            if args.verbose && !args.silent {
                println!("Extracting links from HTML content");
            }

            let mut link_extractor = LinkExtractor::new();

            // Apply network settings
            link_extractor.with_timeout(args.timeout);
            link_extractor.with_retries(args.retries);
            link_extractor.with_random_agent(args.random_agent);

            if let Some(proxy) = &args.proxy {
                link_extractor.with_proxy(Some(proxy.clone()));

                if let Some(auth) = &args.proxy_auth {
                    link_extractor.with_proxy_auth(Some(auth.clone()));
                }
            }

            testers.push(Box::new(link_extractor));
        }

        // Process URLs with testers
        let mut new_urls = Vec::new();

        // Create tasks for parallel processing
        let mut tasks = Vec::new();
        let url_chunks: Vec<_> = transformed_urls.chunks(10).collect();
        let chunk_count = url_chunks.len();

        for (chunk_idx, url_chunk) in url_chunks.into_iter().enumerate() {
            let url_vec = url_chunk.to_vec();
            let testers_clone: Vec<_> = testers.iter().map(|t| t.clone_box()).collect();
            let verbose = args.verbose;
            let check_status = args.check_status;
            let extract_links = args.extract_links;
            let silent = args.silent;

            let task = task::spawn(async move {
                let mut result_urls = Vec::new();

                for url in url_vec {
                    let mut status_result = None;
                    let mut links_result = None;

                    // Process URL with each tester
                    for (i, tester) in testers_clone.iter().enumerate() {
                        match tester.test_url(&url).await {
                            Ok(results) => {
                                if i == 0 && check_status {
                                    // Status checker results (first tester if check_status is enabled)
                                    status_result = Some(results);
                                } else if extract_links {
                                    // Link extractor results
                                    links_result = Some(results);
                                }
                            }
                            Err(e) => {
                                if verbose && !silent {
                                    eprintln!("Error testing URL {}: {}", url, e);
                                }
                            }
                        }
                    }

                    // Create UrlData for this URL
                    if let Some(status_urls) = status_result {
                        for status_url in status_urls {
                            // Parse the status URL (format: "{url} - {status}")
                            result_urls.push(output::UrlData::from_string(status_url));
                        }
                    } else {
                        // If no status but URL should be included anyway
                        if check_status {
                            let url_data = output::UrlData::with_status(
                                url.clone(),
                                "Status check failed".to_string(),
                            );
                            result_urls.push(url_data);
                        } else {
                            let url_data = output::UrlData::new(url.clone());
                            result_urls.push(url_data);
                        }
                    }

                    // If we have extracted links, add them to the result
                    if let Some(link_urls) = links_result {
                        for link_url in link_urls {
                            result_urls.push(output::UrlData::new(link_url));
                        }
                    }
                }

                (result_urls, chunk_idx)
            });

            tasks.push(task);

            // Update progress bar
            test_bar.set_position((chunk_idx as u64 * 10).min(transformed_urls.len() as u64));
            test_bar.set_message(format!(
                "Processing chunk {}/{}",
                chunk_idx + 1,
                chunk_count
            ));
        }

        // Collect results
        let results = join_all(tasks).await;

        for result in results {
            match result {
                Ok((urls, _)) => {
                    for url in urls {
                        new_urls.push(url);
                    }
                }
                Err(e) => {
                    if !args.silent {
                        eprintln!("Task error: {}", e);
                    }
                }
            }
        }

        // If we've tested URLs, replace the final list with the new processed URLs
        if !new_urls.is_empty() {
            // Sort URLs by their URL field
            new_urls.sort_by(|a, b| a.url.cmp(&b.url));
            final_urls = new_urls;
        }

        test_bar.finish_with_message(format!("Testing complete, found {} URLs", final_urls.len()));

        if args.verbose && !args.silent {
            println!("Testing complete, final URL count: {}", final_urls.len());
        }
    } else {
        // No testing, just convert the string URLs to UrlData
        final_urls = transformed_urls
            .iter()
            .map(|url| output::UrlData::new(url.clone()))
            .collect();
    }

    match outputter.output(&final_urls, args.output.clone(), args.silent) {
        Ok(_) => {
            if args.verbose && !args.silent {
                if let Some(path) = &args.output {
                    println!("Results written to: {}", path.display());
                }
            }
        }
        Err(e) => {
            if !args.silent {
                eprintln!("Error writing output: {}", e);
            }
        }
    }

    Ok(())
}
