use anyhow::Result;
use clap::Parser;

mod cli;
mod config;
mod filters;
mod network;
mod output;
mod progress;
mod providers;
mod runner;
mod tester_manager;
mod testers;
mod url_utils;
mod utils;

use cli::{read_domains_from_stdin, Args};
use config::Config;
use filters::UrlFilter;
use network::NetworkSettings;
use output::create_outputter;
use progress::ProgressManager;
use providers::{
    CommonCrawlProvider, OTXProvider, Provider, VirusTotalProvider, WaybackMachineProvider,
};
use runner::{add_provider, process_domains};
use tester_manager::{apply_network_settings_to_tester, process_urls_with_testers};
use testers::{LinkExtractor, StatusChecker, Tester};
use url_utils::UrlTransformer;
use utils::verbose_print;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = Args::parse();

    // Load configuration and apply it to args
    // This ensures command line options take precedence over config file
    let config = Config::load(&args);
    config.apply_to_args(&mut args);

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

    // Create common network settings from args
    let network_settings = NetworkSettings::from_args(&args);

    // Initialize providers based on command-line flags
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    let mut provider_names: Vec<String> = Vec::new();

    if args.providers.iter().any(|p| p == "wayback") {
        add_provider(
            &args,
            &network_settings,
            &mut providers,
            &mut provider_names,
            "Wayback Machine".to_string(),
            WaybackMachineProvider::new,
        );
    }

    if args.providers.iter().any(|p| p == "cc") {
        add_provider(
            &args,
            &network_settings,
            &mut providers,
            &mut provider_names,
            args.cc_index.to_string(),
            || CommonCrawlProvider::with_index(args.cc_index.clone()),
        );
    }

    if args.providers.iter().any(|p| p == "otx") {
        add_provider(
            &args,
            &network_settings,
            &mut providers,
            &mut provider_names,
            "OTX".to_string(),
            OTXProvider::new,
        );
    }

    if args.providers.iter().any(|p| p == "vt") {
        // First check command-line argument, then fall back to environment variable
        let api_key = args
            .vt_api_key
            .clone()
            .or_else(|| std::env::var("URX_VT_API_KEY").ok());

        if let Some(api_key) = api_key {
            add_provider(
                &args,
                &network_settings,
                &mut providers,
                &mut provider_names,
                "VirusTotal".to_string(),
                || VirusTotalProvider::new(api_key.clone()),
            );
        } else if !args.silent {
            eprintln!("Error: The VirusTotal provider (vt) requires an API key. Please use --vt-api-key or set the URX_VT_API_KEY environment variable.");
        }
    }

    if providers.is_empty() {
        if !args.silent {
            eprintln!("Error: No valid providers specified. Please use --providers with valid provider names (wayback, cc, otx, vt)");
        }
        return Ok(());
    }

    // Check for progress bar options
    let progress_check = args.no_progress || args.silent;

    // Setup progress bars
    let progress_manager = ProgressManager::new(progress_check);

    // Process each domain
    let all_urls = process_domains(
        domains,
        &args,
        &progress_manager,
        &providers,
        &provider_names,
    )
    .await;

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

    let outputter = create_outputter(&args.format);

    // Determine if we need to do status checking (either explicitly requested or needed for filters)
    let should_check_status =
        args.check_status || !args.include_status.is_empty() || !args.exclude_status.is_empty();

    let final_urls = if should_check_status || args.extract_links {
        // Initialize appropriate testers
        let mut testers: Vec<Box<dyn Tester>> = Vec::new();

        // Initialize StatusChecker if any status check or filtering is needed
        if should_check_status {
            verbose_print(&args, "Checking HTTP status codes for URLs");

            let mut status_checker = StatusChecker::new();
            apply_network_settings_to_tester(&mut status_checker, &network_settings);

            // Apply status filters if provided
            if !args.include_status.is_empty() {
                status_checker.with_include_status(Some(args.include_status.clone()));
                verbose_print(
                    &args,
                    format!(
                        "Including only status codes that match: {}",
                        args.include_status.join(", ")
                    ),
                );
            }

            if !args.exclude_status.is_empty() {
                status_checker.with_exclude_status(Some(args.exclude_status.clone()));
                verbose_print(
                    &args,
                    format!(
                        "Excluding status codes that match: {}",
                        args.exclude_status.join(", ")
                    ),
                );
            }

            testers.push(Box::new(status_checker));
        }

        if args.extract_links {
            if args.verbose && !args.silent {
                println!("Extracting links from HTML content");
            }

            let mut link_extractor = LinkExtractor::new();
            apply_network_settings_to_tester(&mut link_extractor, &network_settings);
            testers.push(Box::new(link_extractor));
        }

        // Process URLs with testers
        process_urls_with_testers(
            transformed_urls,
            &args,
            &network_settings,
            &progress_manager,
            testers,
            should_check_status,
        )
        .await
    } else {
        // No testing, just convert the string URLs to UrlData
        transformed_urls
            .iter()
            .map(|url| output::UrlData::new(url.clone()))
            .collect()
    };

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
