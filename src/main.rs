use anyhow::Result;
use clap::Parser;

mod cache;
mod cli;
mod config;
mod filters;
#[cfg(feature = "mcp")]
mod mcp;
mod network;
mod output;
mod progress;
mod providers;
mod readers;
mod runner;
mod tester_manager;
mod testers;
mod url_utils;
mod utils;

use cache::{CacheEntry, CacheFilters, CacheKey, CacheManager};
use cli::{read_domains_from_stdin, Args};
use config::Config;
use filters::{HostValidator, UrlFilter};
use network::NetworkSettings;
use output::create_outputter;
use progress::ProgressManager;
use providers::{
    CommonCrawlProvider, OTXProvider, Provider, RobotsProvider, SitemapProvider, UrlscanProvider,
    VirusTotalProvider, WaybackMachineProvider,
};
use readers::read_urls_from_file;
use runner::{add_provider, process_domains};
use tester_manager::{apply_network_settings_to_tester, process_urls_with_testers};
use testers::{LinkExtractor, StatusChecker, Tester};
use url_utils::UrlTransformer;
use utils::verbose_print;

/// Parse API keys from environment variable (comma-separated) and combine with CLI keys
pub fn parse_api_keys(cli_keys: Vec<String>, env_var_name: &str) -> Vec<String> {
    let mut all_keys = cli_keys;

    // Add keys from environment variable if present (comma-separated)
    if let Ok(env_keys) = std::env::var(env_var_name) {
        let env_keys: Vec<String> = env_keys
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        all_keys.extend(env_keys);
    }

    // Remove duplicates while preserving order
    let mut unique_keys = Vec::new();
    for key in all_keys {
        if !unique_keys.contains(&key) {
            unique_keys.push(key);
        }
    }

    unique_keys
}

/// Helper function to auto-enable providers if API key is present
pub fn auto_enable_provider(
    providers_list: &mut Vec<String>,
    api_keys: &[String],
    provider_name: &str,
    verbose: bool,
    silent: bool,
) {
    if !api_keys.is_empty() && !providers_list.iter().any(|p| p == provider_name) {
        providers_list.push(provider_name.to_string());
        if verbose && !silent {
            println!("Auto-enabling {provider_name} provider because API key is provided");
        }
    }
}

/// Create cache manager based on arguments
async fn create_cache_manager(args: &Args) -> Result<Option<CacheManager>> {
    if args.no_cache {
        return Ok(None);
    }

    match args.cache_type.as_str() {
        "sqlite" => {
            let cache_path = args.cache_path.clone().unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                std::path::PathBuf::from(home).join(".urx").join("cache.db")
            });

            verbose_print(
                args,
                format!("Using SQLite cache at: {}", cache_path.display()),
            );
            let manager = CacheManager::new_sqlite(cache_path).await?;
            Ok(Some(manager))
        }
        #[cfg(feature = "redis-cache")]
        "redis" => {
            if let Some(redis_url) = &args.redis_url {
                verbose_print(args, format!("Using Redis cache at: {}", redis_url));
                let manager = CacheManager::new_redis(redis_url).await?;
                Ok(Some(manager))
            } else {
                if !args.silent {
                    eprintln!("Error: Redis cache type selected but no --redis-url provided");
                }
                Err(anyhow::anyhow!("Redis URL required for Redis cache type"))
            }
        }
        #[cfg(not(feature = "redis-cache"))]
        "redis" => {
            if !args.silent {
                eprintln!("Error: Redis cache support not compiled in. Use 'sqlite' or compile with --features redis-cache");
            }
            Err(anyhow::anyhow!("Redis cache not supported"))
        }
        _ => {
            if !args.silent {
                eprintln!(
                    "Error: Unknown cache type '{}'. Use 'sqlite' or 'redis'",
                    args.cache_type
                );
            }
            Err(anyhow::anyhow!("Invalid cache type"))
        }
    }
}

/// Create cache key from arguments and domains
fn create_cache_key(domain: &str, args: &Args) -> CacheKey {
    let filters = CacheFilters {
        subs: args.subs,
        extensions: args.extensions.clone(),
        exclude_extensions: args.exclude_extensions.clone(),
        patterns: args.patterns.clone(),
        exclude_patterns: args.exclude_patterns.clone(),
        presets: args.preset.clone(),
        min_length: args.min_length,
        max_length: args.max_length,
        strict: args.strict,
        normalize_url: args.normalize_url,
        merge_endpoint: args.merge_endpoint,
    };

    CacheKey::new(domain, &args.providers, &filters)
}

/// Process domains with cache support
async fn process_domains_with_cache(
    domains: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    providers: &[Box<dyn Provider>],
    provider_names: &[String],
    cache_manager: Option<&CacheManager>,
) -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let mut final_urls = HashSet::new();

    // If caching is disabled, use normal processing
    if cache_manager.is_none() {
        return process_domains(domains, args, progress_manager, providers, provider_names).await;
    }

    let cache = cache_manager.unwrap();
    let mut domains_to_process = Vec::new();
    let mut cached_urls = HashSet::new();

    // Check cache for each domain
    for domain in &domains {
        let cache_key = create_cache_key(domain, args);

        if cache
            .is_valid(&cache_key, args.cache_ttl)
            .await
            .unwrap_or(false)
        {
            if let Ok(Some(cached_entry)) = cache.get_cached_urls(&cache_key).await {
                verbose_print(args, format!("Using cached results for domain: {}", domain));

                if args.incremental {
                    // For incremental mode, we still need to fetch fresh URLs to compare
                    domains_to_process.push(domain.clone());
                } else {
                    // Use cached results directly
                    cached_urls.extend(cached_entry.urls);
                    continue;
                }
            }
        }

        // Domain not in cache or cache expired, needs processing
        domains_to_process.push(domain.clone());
    }

    // Add cached URLs to final result
    final_urls.extend(cached_urls);

    // Process domains that need fresh data
    if !domains_to_process.is_empty() {
        verbose_print(
            args,
            format!(
                "Processing {} domains (cache miss/expired)",
                domains_to_process.len()
            ),
        );

        let fresh_urls = process_domains(
            domains_to_process.clone(),
            args,
            progress_manager,
            providers,
            provider_names,
        )
        .await;

        // Handle incremental scanning and cache updates
        if args.incremental {
            for domain in &domains_to_process {
                let cache_key = create_cache_key(domain, args);

                // Get domain-specific URLs (this is a simplification - in reality we'd need to track per-domain)
                let domain_fresh_urls: HashSet<String> = fresh_urls
                    .iter()
                    .filter(|url| url.contains(domain))
                    .cloned()
                    .collect();

                let new_urls = cache
                    .get_new_urls(&cache_key, &domain_fresh_urls)
                    .await
                    .unwrap_or(domain_fresh_urls.clone());

                if !new_urls.is_empty() {
                    verbose_print(
                        args,
                        format!("Found {} new URLs for domain: {}", new_urls.len(), domain),
                    );
                    final_urls.extend(new_urls);
                }

                // Update cache with all fresh URLs for this domain
                let entry = CacheEntry::new(domain_fresh_urls.into_iter().collect());
                let _ = cache.store_urls(&cache_key, &entry).await;
            }
        } else {
            // Normal mode: add all fresh URLs and update cache
            final_urls.extend(fresh_urls.clone());

            // For simplicity, store all URLs for each domain (this could be optimized)
            for domain in &domains_to_process {
                let cache_key = create_cache_key(domain, args);
                let domain_urls: Vec<String> = fresh_urls
                    .iter()
                    .filter(|url| url.contains(domain))
                    .cloned()
                    .collect();

                if !domain_urls.is_empty() {
                    let entry = CacheEntry::new(domain_urls);
                    let _ = cache.store_urls(&cache_key, &entry).await;
                }
            }
        }
    }

    // Clean up expired cache entries
    let _ = cache.cleanup_expired(args.cache_ttl * 2).await;

    final_urls
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = Args::parse();

    // Check if MCP mode is enabled
    #[cfg(feature = "mcp")]
    if args.mcp {
        return run_mcp_server().await;
    }

    // Load configuration and apply it to args
    // This ensures command line options take precedence over config file
    let config = Config::load(&args);
    config.apply_to_args(&mut args);

    // Check if file input is provided
    let urls_from_file = if !args.files.is_empty() {
        let mut all_file_urls = Vec::new();

        for file_path in &args.files {
            match read_urls_from_file(file_path) {
                Ok(urls) => {
                    if args.verbose && !args.silent {
                        println!(
                            "Read {} URLs from file: {}",
                            urls.len(),
                            file_path.display()
                        );
                    }
                    all_file_urls.extend(urls);
                }
                Err(e) => {
                    if !args.silent {
                        eprintln!("Error reading file {}: {}", file_path.display(), e);
                    }
                    return Err(e);
                }
            }
        }

        Some(all_file_urls)
    } else {
        None
    };

    let all_urls = if let Some(urls) = urls_from_file {
        // URLs read from file(s) - skip provider processing
        if args.verbose && !args.silent {
            println!(
                "Read {} URLs total from {} file(s)",
                urls.len(),
                args.files.len()
            );
        }
        urls.into_iter().collect()
    } else {
        // No file input - use traditional domain-based approach
        // Collect domains either from arguments or stdin
        let domains = if args.domains.is_empty() {
            read_domains_from_stdin()?
        } else {
            args.domains.clone()
        };

        if domains.is_empty() {
            if !args.silent {
                eprintln!(
                    "No domains provided. Please specify domains or pipe them through stdin."
                );
            }
            return Ok(());
        }

        // Create common network settings from args
        let network_settings = NetworkSettings::from_args(&args);

        // Initialize providers based on command-line flags and API keys
        let mut providers: Vec<Box<dyn Provider>> = Vec::new();
        let mut provider_names: Vec<String> = Vec::new();

        // Get VirusTotal and Urlscan API keys (from CLI and env vars)
        let vt_api_keys = parse_api_keys(args.vt_api_key.clone(), "URX_VT_API_KEY");
        let urlscan_api_keys = parse_api_keys(args.urlscan_api_key.clone(), "URX_URLSCAN_API_KEY");

        // Auto-enable providers if API keys are provided but not explicitly included in providers
        let mut providers_list = args.providers.clone();

        // Auto-enable VirusTotal and Urlscan providers
        auto_enable_provider(
            &mut providers_list,
            &vt_api_keys,
            "vt",
            args.verbose,
            args.silent,
        );
        auto_enable_provider(
            &mut providers_list,
            &urlscan_api_keys,
            "urlscan",
            args.verbose,
            args.silent,
        );

        if providers_list.iter().any(|p| p == "wayback") {
            add_provider(
                &args,
                &network_settings,
                &mut providers,
                &mut provider_names,
                "Wayback Machine".to_string(),
                WaybackMachineProvider::new,
            );
        }

        if providers_list.iter().any(|p| p == "cc") {
            add_provider(
                &args,
                &network_settings,
                &mut providers,
                &mut provider_names,
                args.cc_index.to_string(),
                || CommonCrawlProvider::with_index(args.cc_index.clone()),
            );
        }

        if args.should_use_robots() {
            add_provider(
                &args,
                &network_settings,
                &mut providers,
                &mut provider_names,
                "Robots.txt".to_string(),
                RobotsProvider::new,
            );
        }

        if args.should_use_sitemap() {
            add_provider(
                &args,
                &network_settings,
                &mut providers,
                &mut provider_names,
                "Sitemap".to_string(),
                SitemapProvider::new,
            );
        }

        if providers_list.iter().any(|p| p == "otx") {
            add_provider(
                &args,
                &network_settings,
                &mut providers,
                &mut provider_names,
                "OTX".to_string(),
                OTXProvider::new,
            );
        }

        if providers_list.iter().any(|p| p == "vt") {
            if !vt_api_keys.is_empty() {
                add_provider(
                    &args,
                    &network_settings,
                    &mut providers,
                    &mut provider_names,
                    "VirusTotal".to_string(),
                    || VirusTotalProvider::new_with_keys(vt_api_keys.clone()),
                );
            } else if !args.silent {
                eprintln!("Error: The VirusTotal provider (vt) requires an API key. Please use --vt-api-key or set the URX_VT_API_KEY environment variable.");
            }
        }

        if providers_list.iter().any(|p| p == "urlscan") {
            if !urlscan_api_keys.is_empty() {
                add_provider(
                    &args,
                    &network_settings,
                    &mut providers,
                    &mut provider_names,
                    "Urlscan".to_string(),
                    || UrlscanProvider::new_with_keys(urlscan_api_keys.clone()),
                );
            } else if !args.silent {
                eprintln!("Error: The Urlscan provider (urlscan) requires an API key. Please use --urlscan-api-key or set the URX_URLSCAN_API_KEY environment variable.");
            }
        }

        if providers.is_empty() {
            if !args.silent {
                eprintln!("Error: No valid providers specified. Please use --providers with valid provider names (wayback, cc, otx, vt, urlscan)");
            }
            return Ok(());
        }

        // Check for progress bar options
        let progress_check = args.no_progress || args.silent;

        // Setup progress bars
        let progress_manager = ProgressManager::new(progress_check);

        // Initialize cache manager if caching is enabled
        let cache_manager = create_cache_manager(&args).await.ok().flatten();

        // Process each domain with caching support
        process_domains_with_cache(
            domains.clone(),
            &args,
            &progress_manager,
            &providers,
            &provider_names,
            cache_manager.as_ref(),
        )
        .await
    };

    // Create common network settings from args (for testing phase)
    let network_settings = NetworkSettings::from_args(&args);

    // Check for progress bar options
    let progress_check = args.no_progress || args.silent;

    // Setup progress bars
    let progress_manager = ProgressManager::new(progress_check);

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

    // Apply URL filters
    let mut sorted_urls = url_filter.apply_filters(&all_urls);

    // Apply host validation if strict mode is enabled and we have domains (not from file)
    if args.strict && args.files.is_empty() {
        if args.verbose && !args.silent {
            println!("Enforcing strict host validation...");
        }
        // We need to get domains from the original input
        let domains = if args.domains.is_empty() {
            read_domains_from_stdin().unwrap_or_default()
        } else {
            args.domains.clone()
        };

        if !domains.is_empty() {
            let host_validator = HostValidator::new(&domains, args.subs);
            sorted_urls.retain(|url| host_validator.is_valid_host(url));

            if args.verbose && !args.silent {
                println!(
                    "Number of valid URLs after host validation: {}",
                    sorted_urls.len()
                );
            }
        }
    }

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
        .with_normalize_url(args.normalize_url)
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
                eprintln!("Error writing output: {e}");
            }
        }
    }

    Ok(())
}

/// Run URX as an MCP server
#[cfg(feature = "mcp")]
async fn run_mcp_server() -> Result<()> {
    use mcp::UrxMcpServer;
    use rmcp::{ServiceExt, transport::stdio};

    // Create the MCP server
    let server = UrxMcpServer::new();

    // Get API keys from environment variables
    let vt_api_keys = parse_api_keys(vec![], "URX_VT_API_KEY");
    let urlscan_api_keys = parse_api_keys(vec![], "URX_URLSCAN_API_KEY");

    // Set API keys if available
    if !vt_api_keys.is_empty() {
        server.set_vt_api_keys(vt_api_keys).await;
    }
    if !urlscan_api_keys.is_empty() {
        server.set_urlscan_api_keys(urlscan_api_keys).await;
    }

    // Start the MCP server with stdio transport
    eprintln!("Starting URX MCP server...");
    let service = server.serve(stdio()).await?;

    // Wait for the server to finish
    service.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::collections::HashSet;
    use std::env;

    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    // Serialize tests that mutate environment variables to avoid race conditions
    fn env_mutex() -> &'static std::sync::Mutex<()> {
        static INSTANCE: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn test_auto_enable_provider() {
        // Test the auto_enable_provider helper function directly
        let mut providers_list = vec!["wayback".to_string(), "cc".to_string()];
        let api_keys = vec!["test_api_key".to_string()];

        // Should add vt to the list
        auto_enable_provider(&mut providers_list, &api_keys, "vt", false, false);
        assert!(providers_list.contains(&"vt".to_string()));
        assert_eq!(providers_list.len(), 3);

        // Calling again shouldn't add duplicates
        auto_enable_provider(&mut providers_list, &api_keys, "vt", false, false);
        assert_eq!(providers_list.len(), 3);

        // Empty API key should not add the provider
        let empty_keys: Vec<String> = vec![];
        auto_enable_provider(&mut providers_list, &empty_keys, "urlscan", false, false);
        assert!(!providers_list.contains(&"urlscan".to_string()));
        assert_eq!(providers_list.len(), 3);
    }

    #[test]
    fn test_auto_enable_providers_with_env_vars() {
        let _env_lock = env_mutex().lock().unwrap();
        // Save current environment to restore later
        let old_vt_key = env::var("URX_VT_API_KEY").ok();
        let old_urlscan_key = env::var("URX_URLSCAN_API_KEY").ok();

        // Set environment variables for testing
        env::set_var("URX_VT_API_KEY", "test_vt_key");
        env::set_var("URX_URLSCAN_API_KEY", "test_urlscan_key");

        // Create args without specifying providers (will use default)
        let args = Args::parse_from(["urx", "example.com"]);

        // Create our own empty providers list for testing
        let mut providers_list = Vec::new();

        // Get API keys using the new parsing function (this simulates part of main function)
        let vt_api_keys = parse_api_keys(args.vt_api_key.clone(), "URX_VT_API_KEY");
        let urlscan_api_keys = parse_api_keys(args.urlscan_api_key.clone(), "URX_URLSCAN_API_KEY");

        // Test auto-enabling providers
        auto_enable_provider(&mut providers_list, &vt_api_keys, "vt", false, false);
        auto_enable_provider(
            &mut providers_list,
            &urlscan_api_keys,
            "urlscan",
            false,
            false,
        );

        // Verify both providers were added
        assert!(providers_list.contains(&"vt".to_string()));
        assert!(providers_list.contains(&"urlscan".to_string()));
        assert_eq!(providers_list.len(), 2);

        // Restore environment
        match old_vt_key {
            Some(val) => env::set_var("URX_VT_API_KEY", val),
            None => env::remove_var("URX_VT_API_KEY"),
        }

        match old_urlscan_key {
            Some(val) => env::set_var("URX_URLSCAN_API_KEY", val),
            None => env::remove_var("URX_URLSCAN_API_KEY"),
        }
    }

    #[test]
    fn test_parse_api_keys() {
        // Test CLI keys only
        let cli_keys = vec!["key1".to_string(), "key2".to_string()];
        let result = parse_api_keys(cli_keys, "NONEXISTENT_ENV_VAR");
        assert_eq!(result, vec!["key1", "key2"]);

        // Test environment keys only (using an actual env var for testing)
        let _env_lock = env_mutex().lock().unwrap();
        env::set_var("TEST_API_KEYS", "env_key1,env_key2, env_key3 ");
        let result = parse_api_keys(vec![], "TEST_API_KEYS");
        assert_eq!(result, vec!["env_key1", "env_key2", "env_key3"]);
        env::remove_var("TEST_API_KEYS");

        // Test CLI + environment (CLI should come first)
        env::set_var("TEST_API_KEYS", "env_key1,env_key2");
        let cli_keys = vec!["cli_key1".to_string()];
        let result = parse_api_keys(cli_keys, "TEST_API_KEYS");
        assert_eq!(result, vec!["cli_key1", "env_key1", "env_key2"]);
        env::remove_var("TEST_API_KEYS");

        // Test duplicate removal
        env::set_var("TEST_API_KEYS", "key1,key2");
        let cli_keys = vec!["key1".to_string(), "key3".to_string()];
        let result = parse_api_keys(cli_keys, "TEST_API_KEYS");
        assert_eq!(result, vec!["key1", "key3", "key2"]);
        env::remove_var("TEST_API_KEYS");

        // Test empty strings are filtered
        env::set_var("TEST_API_KEYS", "key1,,key2, ,key3");
        let result = parse_api_keys(vec![], "TEST_API_KEYS");
        assert_eq!(result, vec!["key1", "key2", "key3"]);
        env::remove_var("TEST_API_KEYS");
    }

    #[test]
    fn test_multiple_api_keys_integration() {
        let _env_lock = env_mutex().lock().unwrap();

        // Save and clear environment variables to isolate from ambient env
        let old_vt_key = env::var("URX_VT_API_KEY").ok();
        let old_urlscan_key = env::var("URX_URLSCAN_API_KEY").ok();
        env::remove_var("URX_VT_API_KEY");
        env::remove_var("URX_URLSCAN_API_KEY");

        // Test multiple VT API keys via CLI
        let args = Args::parse_from([
            "urx",
            "example.com",
            "--vt-api-key",
            "vt_key1",
            "--vt-api-key",
            "vt_key2",
            "--urlscan-api-key",
            "url_key1",
        ]);

        assert_eq!(args.vt_api_key, vec!["vt_key1", "vt_key2"]);
        assert_eq!(args.urlscan_api_key, vec!["url_key1"]);

        // Test that parse_api_keys works with the CLI args
        let vt_keys = parse_api_keys(args.vt_api_key, "URX_VT_API_KEY");
        let url_keys = parse_api_keys(args.urlscan_api_key, "URX_URLSCAN_API_KEY");

        assert_eq!(vt_keys, vec!["vt_key1", "vt_key2"]);
        assert_eq!(url_keys, vec!["url_key1"]);

        // Restore environment
        match old_vt_key {
            Some(val) => env::set_var("URX_VT_API_KEY", val),
            None => env::remove_var("URX_VT_API_KEY"),
        }
        match old_urlscan_key {
            Some(val) => env::set_var("URX_URLSCAN_API_KEY", val),
            None => env::remove_var("URX_URLSCAN_API_KEY"),
        }
    }

    #[test]
    fn test_api_key_precedence() {
        let _env_lock = env_mutex().lock().unwrap();
        // This test verifies command-line arguments take precedence over env vars

        // Save current environment
        let old_vt_key = env::var("URX_VT_API_KEY").ok();

        // Set environment variable
        env::set_var("URX_VT_API_KEY", "env_vt_key");

        // Create args with explicit API key
        let args = Args::parse_from(["urx", "example.com", "--vt-api-key", "arg_vt_key"]);

        // Verify command line arg takes precedence using parse_api_keys
        let vt_api_keys = parse_api_keys(args.vt_api_key.clone(), "URX_VT_API_KEY");
        assert_eq!(vt_api_keys, vec!["arg_vt_key", "env_vt_key"]);
        // CLI arg should be first (taking precedence)
        assert_eq!(vt_api_keys[0], "arg_vt_key");

        // Create args without explicit API key
        let args = Args::parse_from(["urx", "example.com"]);

        // Verify environment variable is used as fallback
        let vt_api_keys = parse_api_keys(args.vt_api_key.clone(), "URX_VT_API_KEY");
        assert_eq!(vt_api_keys, vec!["env_vt_key"]);

        // Restore environment
        match old_vt_key {
            Some(val) => env::set_var("URX_VT_API_KEY", val),
            None => env::remove_var("URX_VT_API_KEY"),
        }
    }

    // Mock Provider for testing
    #[derive(Clone)]
    struct MockProvider {
        urls: Vec<String>,
        should_fail: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MockProvider {
        fn new(urls: Vec<String>, should_fail: bool) -> Self {
            MockProvider {
                urls,
                should_fail,
                calls: Arc::new(Mutex::new(vec![])),
            }
        }
    }

    impl Provider for MockProvider {
        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        fn fetch_urls<'a>(
            &'a self,
            domain: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
            let urls = self.urls.clone();
            let should_fail = self.should_fail;
            let calls = self.calls.clone();

            Box::pin(async move {
                // Record the call
                calls.lock().unwrap().push(domain.to_string());

                if should_fail {
                    Err(anyhow::anyhow!("Mock provider failure"))
                } else {
                    Ok(urls)
                }
            })
        }

        fn with_subdomains(&mut self, _include: bool) {}
        fn with_proxy(&mut self, _proxy: Option<String>) {}
        fn with_proxy_auth(&mut self, _auth: Option<String>) {}
        fn with_timeout(&mut self, _seconds: u64) {}
        fn with_retries(&mut self, _count: u32) {}
        fn with_random_agent(&mut self, _enabled: bool) {}
        fn with_insecure(&mut self, _enabled: bool) {}
        fn with_parallel(&mut self, _parallel: u32) {}
        fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
    }

    // Mock StatusChecker for testing
    #[derive(Clone)]
    struct MockStatusChecker {
        results: Vec<String>,
    }

    impl MockStatusChecker {
        fn new(results: Vec<String>) -> Self {
            MockStatusChecker { results }
        }
    }

    impl Tester for MockStatusChecker {
        fn clone_box(&self) -> Box<dyn Tester> {
            Box::new(self.clone())
        }

        fn test_url<'a>(
            &'a self,
            _url: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
            let results = self.results.clone();
            Box::pin(async move { Ok(results) })
        }

        fn with_timeout(&mut self, _seconds: u64) {}
        fn with_retries(&mut self, _count: u32) {}
        fn with_random_agent(&mut self, _enabled: bool) {}
        fn with_insecure(&mut self, _enabled: bool) {}
        fn with_proxy(&mut self, _proxy: Option<String>) {}
        fn with_proxy_auth(&mut self, _auth: Option<String>) {}
    }

    #[tokio::test]
    async fn test_process_domains() {
        // Create mock providers
        let mock_urls = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ];

        let provider = MockProvider::new(mock_urls.clone(), false);
        let calls = provider.calls.clone();

        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];
        let provider_names = vec!["MockProvider".to_string()];

        // Setup test args with minimal settings
        let args = Args {
            domains: vec!["example.com".to_string()],
            config: None,
            files: vec![],
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            normalize_url: false,
            providers: vec!["mock".to_string()],
            subs: false,
            cc_index: "CC-MAIN-2025-13".to_string(),
            vt_api_key: vec![],
            urlscan_api_key: vec![],
            verbose: false,
            silent: true,      // Silent to avoid console output during tests
            no_progress: true, // No progress bars during tests
            preset: vec![],
            extensions: vec![],
            exclude_extensions: vec![],
            patterns: vec![],
            exclude_patterns: vec![],
            show_only_host: false,
            show_only_path: false,
            show_only_param: false,
            min_length: None,
            max_length: None,
            strict: true, // Default strict mode enabled
            network_scope: "all".to_string(),
            proxy: None,
            proxy_auth: None,
            insecure: false,
            random_agent: false,
            timeout: 30,
            retries: 3,
            parallel: Some(5),
            rate_limit: None,
            check_status: false,
            include_status: vec![],
            exclude_status: vec![],
            extract_links: false,
            include_robots: true,
            include_sitemap: true,
            exclude_robots: false,
            exclude_sitemap: false,
            incremental: false,
            cache_type: "sqlite".to_string(),
            cache_path: None,
            redis_url: None,
            cache_ttl: 86400,
            no_cache: false,
            #[cfg(feature = "mcp")]
            mcp: false,
        };

        let progress_manager = ProgressManager::new(true);

        // Process domains with mock provider
        let urls = process_domains(
            vec!["example.com".to_string()],
            &args,
            &progress_manager,
            &providers,
            &provider_names,
        )
        .await;

        // Verify that the provider was called with the correct domain
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "example.com");

        // Verify that the URLs were correctly returned
        assert_eq!(urls.len(), 2);
        assert!(urls.contains("https://example.com/page1"));
        assert!(urls.contains("https://example.com/page2"));
    }

    #[tokio::test]
    async fn test_process_urls_with_testers() {
        // Create mock tester
        let mock_results = vec![
            "https://example.com/result1".to_string(),
            "https://example.com/result2".to_string(),
        ];
        let mock_tester = MockStatusChecker::new(mock_results.clone());
        let testers: Vec<Box<dyn Tester>> = vec![Box::new(mock_tester)];

        // Create test input
        let input_urls = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ];

        // Setup minimal args
        let args = Args {
            domains: vec![],
            config: None,
            files: vec![],
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            normalize_url: false,
            providers: vec![],
            subs: false,
            cc_index: "CC-MAIN-2025-13".to_string(),
            vt_api_key: vec![],
            urlscan_api_key: vec![],
            verbose: false,
            silent: true,
            no_progress: true,
            preset: vec![],
            extensions: vec![],
            exclude_extensions: vec![],
            patterns: vec![],
            exclude_patterns: vec![],
            show_only_host: false,
            show_only_path: false,
            show_only_param: false,
            min_length: None,
            max_length: None,
            strict: true,
            network_scope: "all".to_string(),
            proxy: None,
            proxy_auth: None,
            insecure: false,
            random_agent: false,
            timeout: 30,
            retries: 3,
            parallel: Some(5),
            rate_limit: None,
            check_status: false,
            include_status: vec![],
            exclude_status: vec![],
            extract_links: false,
            include_robots: true,
            include_sitemap: true,
            exclude_robots: false,
            exclude_sitemap: false,
            incremental: false,
            cache_type: "sqlite".to_string(),
            cache_path: None,
            redis_url: None,
            cache_ttl: 86400,
            no_cache: false,
            #[cfg(feature = "mcp")]
            mcp: false,
        };

        let network_settings = NetworkSettings::new();
        let progress_manager = ProgressManager::new(true);

        // Process URLs with mock tester
        let result_data = process_urls_with_testers(
            input_urls,
            &args,
            &network_settings,
            &progress_manager,
            testers,
            false, // 여기를 false로 변경 (should_check_status)
        )
        .await;

        // URLs가 올바른지 검증 - 모든 URL이 UrlData 구조체로 래핑됨
        let result_urls: Vec<String> = result_data.iter().map(|data| data.url.clone()).collect();

        // 결과 데이터에 원본 입력 URL이 포함되어 있는지 확인
        assert_eq!(result_urls.len(), 2);
        assert!(result_urls.contains(&"https://example.com/page1".to_string()));
        assert!(result_urls.contains(&"https://example.com/page2".to_string()));
    }

    #[test]
    fn test_url_filtering() {
        // Create a set of test URLs
        let urls = HashSet::from([
            "https://example.com/page1.html".to_string(),
            "https://example.com/image.jpg".to_string(),
            "https://example.com/script.js".to_string(),
            "https://example.com/styles.css".to_string(),
        ]);

        // Create filter to only include .html and .js files
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["html".to_string(), "js".to_string()]);

        // Apply filter
        let filtered = filter.apply_filters(&urls);

        // Verify results
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/page1.html".to_string()));
        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(!filtered.contains(&"https://example.com/image.jpg".to_string()));
        assert!(!filtered.contains(&"https://example.com/styles.css".to_string()));
    }

    #[test]
    fn test_url_transformation() {
        // Test URLs
        let urls = vec![
            "https://example.com/path/to/page?param1=value1&param2=value2".to_string(),
            "https://subdomain.example.com/another/path?id=123".to_string(),
        ];

        // Test host-only transformation
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_host(true);

        let host_only = transformer.transform(urls.clone());
        assert_eq!(host_only.len(), 2);
        assert!(host_only.contains(&"example.com".to_string()));
        assert!(host_only.contains(&"subdomain.example.com".to_string()));

        // Test path-only transformation
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_path(true);

        let path_only = transformer.transform(urls.clone());
        assert_eq!(path_only.len(), 2);
        assert!(path_only.contains(&"/path/to/page".to_string()));
        assert!(path_only.contains(&"/another/path".to_string()));

        // Test param-only transformation
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_param(true);

        let param_only = transformer.transform(urls);
        assert_eq!(param_only.len(), 2);
        assert!(
            param_only.contains(&"param1=value1&param2=value2".to_string())
                || param_only.contains(&"param2=value2&param1=value1".to_string())
        );
        assert!(param_only.contains(&"id=123".to_string()));
    }
}
