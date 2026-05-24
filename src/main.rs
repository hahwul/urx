use anyhow::Result;
use clap::Parser;

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

use cache::{CacheEntry, CacheFilters, CacheKey, CacheManager};
use cli::{read_domains_from_file, read_domains_from_stdin, Args};
use config::Config;
use filters::{HostValidator, UrlFilter};
use network::NetworkSettings;
use output::create_outputter;
use progress::ProgressManager;
use providers::{
    CommonCrawlProvider, GitHubProvider, OTXProvider, Provider, RobotsProvider, SitemapProvider,
    UrlscanProvider, VirusTotalProvider, WaybackMachineProvider, ZoomEyeProvider,
};
use readers::read_urls_from_file;
use runner::{add_provider, process_domains, ProviderRunResult};
use tester_manager::{apply_network_settings_to_tester, process_urls_with_testers};
use testers::{LinkExtractor, StatusChecker, Tester};
use utils::verbose_print;
use utils::UrlTransformer;

/// Type alias for provider initialization result
type ProviderList = (Vec<Box<dyn Provider>>, Vec<String>);

/// Static metadata for one of urx's URL providers.
struct ProviderInfo {
    /// Short identifier accepted on the command line (e.g. "wayback").
    id: &'static str,
    /// Human-readable display name shown in stats and `--list-providers`.
    display_name: &'static str,
    /// True when the provider can only be enabled with an API key.
    requires_key: bool,
    /// One-line description shown by `--list-providers`.
    summary: &'static str,
}

/// Catalog of every provider urx knows about. The order here drives the
/// `--list-providers` output and the meaning of `--all-providers`.
fn provider_catalog() -> &'static [ProviderInfo] {
    &[
        ProviderInfo {
            id: "wayback",
            display_name: "Wayback Machine",
            requires_key: false,
            summary: "Internet Archive CDX index",
        },
        ProviderInfo {
            id: "cc",
            display_name: "Common Crawl",
            requires_key: false,
            summary: "Common Crawl monthly URL index",
        },
        ProviderInfo {
            id: "otx",
            display_name: "OTX",
            requires_key: false,
            summary: "AlienVault Open Threat Exchange passive DNS / URLs",
        },
        ProviderInfo {
            id: "vt",
            display_name: "VirusTotal",
            requires_key: true,
            summary: "VirusTotal observed URLs (URX_VT_API_KEY)",
        },
        ProviderInfo {
            id: "urlscan",
            display_name: "Urlscan",
            requires_key: true,
            summary: "Urlscan.io search (URX_URLSCAN_API_KEY)",
        },
        ProviderInfo {
            id: "zoomeye",
            display_name: "ZoomEye",
            requires_key: true,
            summary: "ZoomEye search (URX_ZOOMEYE_API_KEY)",
        },
        ProviderInfo {
            id: "github",
            display_name: "GitHub",
            requires_key: true,
            summary: "GitHub Code Search (URX_GITHUB_API_KEY)",
        },
        ProviderInfo {
            id: "robots",
            display_name: "robots.txt",
            requires_key: false,
            summary: "Discovery from the target's robots.txt",
        },
        ProviderInfo {
            id: "sitemap",
            display_name: "sitemap.xml",
            requires_key: false,
            summary: "Discovery from the target's sitemap.xml",
        },
    ]
}

/// Print the provider catalog to stdout in a `--list-providers` format.
fn print_provider_list() {
    println!("Available providers:");
    println!("  {:<9}  {:<16}  {:<8}  description", "id", "name", "key");
    println!(
        "  {:<9}  {:<16}  {:<8}  -----------",
        "---------", "----------------", "--------"
    );
    for p in provider_catalog() {
        println!(
            "  {:<9}  {:<16}  {:<8}  {}",
            p.id,
            p.display_name,
            if p.requires_key { "required" } else { "—" },
            p.summary
        );
    }
    println!();
    println!("Use --providers id1,id2 to select. --all-providers enables every entry");
    println!("(API-keyed providers only activate when a key is available).");
    println!("--exclude-providers wins on conflict.");
}

/// Collect the effective domain list from CLI positional args, `--domain-list`
/// files, and (when both are empty) stdin. Duplicates are removed while
/// preserving first-seen order so the run order is predictable.
fn collect_domains(args: &Args) -> Result<Vec<String>> {
    let mut domains: Vec<String> = args.domains.clone();

    for path in &args.domain_list {
        let file_domains = read_domains_from_file(path)?;
        if args.verbose && !args.silent {
            println!(
                "Loaded {} domains from {}",
                file_domains.len(),
                path.display()
            );
        }
        domains.extend(file_domains);
    }

    // Only fall back to stdin when no domains were supplied via flags/files,
    // otherwise piped data would silently get appended on every invocation.
    if domains.is_empty() {
        domains.extend(read_domains_from_stdin()?);
    }

    let mut seen = std::collections::HashSet::new();
    domains.retain(|d| seen.insert(d.clone()));
    Ok(domains)
}

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

/// Initialize all providers based on args and API keys
fn initialize_providers(args: &Args, network_settings: &NetworkSettings) -> Result<ProviderList> {
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    let mut provider_names: Vec<String> = Vec::new();

    // Get API keys (from CLI and env vars)
    let vt_api_keys = parse_api_keys(args.vt_api_key.clone(), "URX_VT_API_KEY");
    let urlscan_api_keys = parse_api_keys(args.urlscan_api_key.clone(), "URX_URLSCAN_API_KEY");
    let zoomeye_api_keys = parse_api_keys(args.zoomeye_api_key.clone(), "URX_ZOOMEYE_API_KEY");
    let github_api_keys = parse_api_keys(args.github_api_key.clone(), "URX_GITHUB_API_KEY");

    // Build the effective providers list. `--all-providers` expands to every
    // catalog entry whose required key (if any) is available; otherwise we
    // start from the user-supplied list and let auto-enable add API-keyed
    // providers whenever a key is set.
    let mut providers_list: Vec<String> = if args.all_providers {
        provider_catalog()
            .iter()
            .filter(|p| {
                if !p.requires_key {
                    return true;
                }
                match p.id {
                    "vt" => !vt_api_keys.is_empty(),
                    "urlscan" => !urlscan_api_keys.is_empty(),
                    "zoomeye" => !zoomeye_api_keys.is_empty(),
                    "github" => !github_api_keys.is_empty(),
                    _ => false,
                }
            })
            // Robots/sitemap are gated by the dedicated should_use_* flags, so
            // we leave them out here and let those code paths add them.
            .filter(|p| p.id != "robots" && p.id != "sitemap")
            .map(|p| p.id.to_string())
            .collect()
    } else {
        args.providers.clone()
    };

    // Auto-enable API-keyed providers when a key is present but the user did
    // not list them explicitly. Skipped under --all-providers (already
    // expanded above).
    if !args.all_providers {
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
        auto_enable_provider(
            &mut providers_list,
            &zoomeye_api_keys,
            "zoomeye",
            args.verbose,
            args.silent,
        );
        auto_enable_provider(
            &mut providers_list,
            &github_api_keys,
            "github",
            args.verbose,
            args.silent,
        );
    }

    // Apply negative selection: --exclude-providers wins on conflict.
    if !args.exclude_providers.is_empty() {
        let excluded: std::collections::HashSet<&str> =
            args.exclude_providers.iter().map(String::as_str).collect();
        providers_list.retain(|p| !excluded.contains(p.as_str()));
    }

    // --all-providers users don't want a noisy error when a key is missing,
    // so suppress the per-provider "needs API key" messages in that mode.
    let suppress_key_errors = args.all_providers;

    if providers_list.iter().any(|p| p == "wayback") {
        // Normalise --wayback-from/--wayback-to up front so a malformed value
        // produces a single warning instead of one per domain. CDX wants
        // YYYYMMDDhhmmss.
        let wayback_from = args.wayback_from.as_deref().and_then(|s| {
            let parsed = providers::wayback::normalize_cdx_timestamp(s, false);
            if parsed.is_none() && !args.silent {
                eprintln!("Ignoring --wayback-from={s:?}: expected YYYY, YYYYMM, YYYYMMDD, or YYYYMMDDhhmmss");
            }
            parsed
        });
        let wayback_to = args.wayback_to.as_deref().and_then(|s| {
            let parsed = providers::wayback::normalize_cdx_timestamp(s, true);
            if parsed.is_none() && !args.silent {
                eprintln!("Ignoring --wayback-to={s:?}: expected YYYY, YYYYMM, YYYYMMDD, or YYYYMMDDhhmmss");
            }
            parsed
        });
        let wb_from = wayback_from.clone();
        let wb_to = wayback_to.clone();
        add_provider(
            args,
            network_settings,
            &mut providers,
            &mut provider_names,
            "wayback",
            "Wayback Machine".to_string(),
            move || {
                let mut p = WaybackMachineProvider::new();
                p.with_from(wb_from).with_to(wb_to);
                p
            },
        );
    }

    if providers_list.iter().any(|p| p == "cc") {
        // Each --cc-index entry becomes its own provider instance so they
        // run in parallel and the per-provider stats stay distinct.
        for index in &args.cc_index {
            let index = index.clone();
            add_provider(
                args,
                network_settings,
                &mut providers,
                &mut provider_names,
                "cc",
                index.clone(),
                || CommonCrawlProvider::with_index(index.clone()),
            );
        }
    }

    let excluded: std::collections::HashSet<&str> =
        args.exclude_providers.iter().map(String::as_str).collect();

    if args.should_use_robots() && !excluded.contains("robots") {
        add_provider(
            args,
            network_settings,
            &mut providers,
            &mut provider_names,
            "robots",
            "Robots.txt".to_string(),
            RobotsProvider::new,
        );
    }

    if args.should_use_sitemap() && !excluded.contains("sitemap") {
        add_provider(
            args,
            network_settings,
            &mut providers,
            &mut provider_names,
            "sitemap",
            "Sitemap".to_string(),
            SitemapProvider::new,
        );
    }

    if providers_list.iter().any(|p| p == "otx") {
        add_provider(
            args,
            network_settings,
            &mut providers,
            &mut provider_names,
            "otx",
            "OTX".to_string(),
            OTXProvider::new,
        );
    }

    if providers_list.iter().any(|p| p == "vt") {
        if !vt_api_keys.is_empty() {
            add_provider(
                args,
                network_settings,
                &mut providers,
                &mut provider_names,
                "vt",
                "VirusTotal".to_string(),
                || VirusTotalProvider::new_with_keys(vt_api_keys.clone()),
            );
        } else if !args.silent && !suppress_key_errors {
            eprintln!("Error: The VirusTotal provider (vt) requires an API key. Please use --vt-api-key or set the URX_VT_API_KEY environment variable.");
        }
    }

    if providers_list.iter().any(|p| p == "urlscan") {
        if !urlscan_api_keys.is_empty() {
            add_provider(
                args,
                network_settings,
                &mut providers,
                &mut provider_names,
                "urlscan",
                "Urlscan".to_string(),
                || UrlscanProvider::new_with_keys(urlscan_api_keys.clone()),
            );
        } else if !args.silent && !suppress_key_errors {
            eprintln!("Error: The Urlscan provider (urlscan) requires an API key. Please use --urlscan-api-key or set the URX_URLSCAN_API_KEY environment variable.");
        }
    }

    if providers_list.iter().any(|p| p == "zoomeye") {
        if !zoomeye_api_keys.is_empty() {
            add_provider(
                args,
                network_settings,
                &mut providers,
                &mut provider_names,
                "zoomeye",
                "ZoomEye".to_string(),
                || ZoomEyeProvider::new_with_keys(zoomeye_api_keys.clone()),
            );
        } else if !args.silent && !suppress_key_errors {
            eprintln!("Error: The ZoomEye provider (zoomeye) requires an API key. Please use --zoomeye-api-key or set the URX_ZOOMEYE_API_KEY environment variable.");
        }
    }

    if providers_list.iter().any(|p| p == "github") {
        if !github_api_keys.is_empty() {
            add_provider(
                args,
                network_settings,
                &mut providers,
                &mut provider_names,
                "github",
                "GitHub".to_string(),
                || GitHubProvider::new_with_keys(github_api_keys.clone()),
            );
        } else if !args.silent && !suppress_key_errors {
            eprintln!("Error: The GitHub provider (github) requires an API key. Please use --github-api-key or set the URX_GITHUB_API_KEY environment variable.");
        }
    }

    if providers.is_empty() {
        if !args.silent {
            eprintln!("Error: No valid providers specified. Please use --providers with valid provider names (wayback, cc, otx, vt, urlscan, zoomeye)");
        }
        return Err(anyhow::anyhow!("No valid providers specified"));
    }

    Ok((providers, provider_names))
}

/// Read URLs from multiple files
fn read_urls_from_files(args: &Args) -> Result<Option<Vec<String>>> {
    if args.files.is_empty() {
        return Ok(None);
    }

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

    if args.verbose && !args.silent {
        println!(
            "Read {} URLs total from {} file(s)",
            all_file_urls.len(),
            args.files.len()
        );
    }

    Ok(Some(all_file_urls))
}

/// Apply URL filtering and host validation
fn apply_url_filters(
    args: &Args,
    urls: &std::collections::HashSet<String>,
    progress_manager: &ProgressManager,
) -> Result<Vec<String>> {
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
    let mut sorted_urls = url_filter.apply_filters(urls);

    // Apply host validation if strict mode is enabled and we have domains (not from file)
    if args.strict && args.files.is_empty() {
        if args.verbose && !args.silent {
            println!("Enforcing strict host validation...");
        }
        // Re-resolve the original domain list. We can't read stdin a second
        // time, so the host validator falls back to whatever positional args
        // and --domain-list files supplied.
        let mut domains: Vec<String> = args.domains.clone();
        for path in &args.domain_list {
            domains.extend(read_domains_from_file(path)?);
        }

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

    Ok(sorted_urls)
}

/// Apply URL transformations
fn apply_url_transformations(
    args: &Args,
    urls: Vec<String>,
    progress_manager: &ProgressManager,
) -> Vec<String> {
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

    let transformed_urls = url_transformer.transform(urls);

    if let Some(bar) = transform_bar {
        bar.finish_with_message(format!("Transformed to {} URLs", transformed_urls.len()));
    }

    transformed_urls
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

/// Collect URLs that truly belong to `domain`, using host validation instead of
/// substring matching so cache entries don't bleed across similar domains or
/// query strings.
fn collect_domain_urls(
    urls: &std::collections::HashMap<String, std::collections::HashSet<String>>,
    domain: &str,
    include_subdomains: bool,
) -> std::collections::HashSet<String> {
    let validator = HostValidator::new(&[domain.to_string()], include_subdomains);
    urls.keys()
        .filter(|url| validator.is_valid_host(url))
        .cloned()
        .collect()
}

/// Process domains with cache support
async fn process_domains_with_cache(
    domains: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    providers: &[Box<dyn Provider>],
    provider_names: &[String],
    cache_manager: Option<&CacheManager>,
) -> ProviderRunResult {
    use std::collections::{HashMap, HashSet};

    let mut final_result = ProviderRunResult::default();

    // If caching is disabled, use normal processing
    if cache_manager.is_none() {
        return process_domains(domains, args, progress_manager, providers, provider_names).await;
    }

    let cache = cache_manager.unwrap();
    let mut domains_to_process = Vec::new();
    let mut cached_urls: HashMap<String, HashSet<String>> = HashMap::new();

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
                    // Use cached results directly. Source attribution isn't
                    // persisted in the cache, so cached URLs surface with an
                    // empty provider set.
                    for url in cached_entry.urls {
                        cached_urls.entry(url).or_default();
                    }
                    continue;
                }
            }
        }

        // Domain not in cache or cache expired, needs processing
        domains_to_process.push(domain.clone());
    }

    // Add cached URLs to final result
    for (url, sources) in cached_urls {
        final_result.urls.entry(url).or_default().extend(sources);
    }

    // Process domains that need fresh data
    if !domains_to_process.is_empty() {
        verbose_print(
            args,
            format!(
                "Processing {} domains (cache miss/expired)",
                domains_to_process.len()
            ),
        );

        let fresh_run = process_domains(
            domains_to_process.clone(),
            args,
            progress_manager,
            providers,
            provider_names,
        )
        .await;

        // Carry the provider stats from the fresh run through to the caller.
        final_result.stats = fresh_run.stats;

        // Handle incremental scanning and cache updates
        if args.incremental {
            for domain in &domains_to_process {
                let cache_key = create_cache_key(domain, args);

                let domain_fresh_urls = collect_domain_urls(&fresh_run.urls, domain, args.subs);

                let new_urls = cache
                    .get_new_urls(&cache_key, &domain_fresh_urls)
                    .await
                    .unwrap_or(domain_fresh_urls.clone());

                if !new_urls.is_empty() {
                    verbose_print(
                        args,
                        format!("Found {} new URLs for domain: {}", new_urls.len(), domain),
                    );
                    for url in new_urls {
                        if let Some(sources) = fresh_run.urls.get(&url) {
                            final_result
                                .urls
                                .entry(url)
                                .or_default()
                                .extend(sources.iter().cloned());
                        } else {
                            final_result.urls.entry(url).or_default();
                        }
                    }
                }

                // Update cache with all fresh URLs for this domain
                let entry = CacheEntry::new(domain_fresh_urls.into_iter().collect());
                let _ = cache.store_urls(&cache_key, &entry).await;
            }
        } else {
            // Normal mode: merge all fresh URLs (and their providers) into the result.
            for (url, sources) in &fresh_run.urls {
                final_result
                    .urls
                    .entry(url.clone())
                    .or_default()
                    .extend(sources.iter().cloned());
            }

            // For simplicity, store all URLs for each domain (this could be optimized)
            for domain in &domains_to_process {
                let cache_key = create_cache_key(domain, args);
                let domain_urls: Vec<String> =
                    collect_domain_urls(&fresh_run.urls, domain, args.subs)
                        .into_iter()
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

    final_result
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = Args::parse();

    // Short-circuit: list providers and exit without doing any I/O.
    if args.list_providers {
        print_provider_list();
        return Ok(());
    }

    // Load configuration and apply it to args
    // This ensures command line options take precedence over config file
    // Capture whether the user provided API keys directly via CLI/env *before*
    // either config layer fills them in — this drives the precedence rule
    // CLI/env > provider-config > main config.
    let cli_supplied_vt = !args.vt_api_key.is_empty();
    let cli_supplied_urlscan = !args.urlscan_api_key.is_empty();
    let cli_supplied_zoomeye = !args.zoomeye_api_key.is_empty();

    let config = Config::load(&args)?;
    config.apply_to_args(&mut args);

    // Provider-config file (separate from main config) loads API keys that
    // would otherwise live in the shared config. It overrides main-config
    // values but still loses to anything supplied on the CLI / env.
    let provider_keys = config::ProviderKeysConfig::load(&args)?;
    provider_keys.apply_to_args(
        &mut args,
        cli_supplied_vt,
        cli_supplied_urlscan,
        cli_supplied_zoomeye,
    );

    // Create common network settings and progress manager once
    let network_settings = NetworkSettings::from_args(&args);
    let progress_check = args.no_progress || args.silent;
    let progress_manager = ProgressManager::new(progress_check);

    // Check if file input is provided
    let urls_from_file = read_urls_from_files(&args)?;

    let run_result = if let Some(urls) = urls_from_file {
        // URLs read from file(s) - skip provider processing. Mark every URL
        // as coming from "file" so downstream `--show-sources` is consistent.
        let mut url_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for url in urls {
            url_map.entry(url).or_default().insert("file".to_string());
        }
        ProviderRunResult {
            urls: url_map,
            stats: Vec::new(),
        }
    } else {
        // No file input - use traditional domain-based approach
        let domains = collect_domains(&args)?;

        if domains.is_empty() {
            if !args.silent {
                eprintln!(
                    "No domains provided. Pass DOMAINS positionally, use --domain-list FILE, or pipe them through stdin."
                );
            }
            return Ok(());
        }

        // Initialize providers based on command-line flags and API keys
        let (providers, provider_names) = initialize_providers(&args, &network_settings)?;

        // Initialize cache manager if caching is enabled
        let cache_manager = create_cache_manager(&args).await?;

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

    // URL-only view for filters (they don't care about sources).
    let all_urls: std::collections::HashSet<String> = run_result.urls.keys().cloned().collect();

    // Apply URL filtering
    let sorted_urls = apply_url_filters(&args, &all_urls, &progress_manager)?;

    // Apply URL transformations
    let transformed_urls = apply_url_transformations(&args, sorted_urls, &progress_manager);

    let outputter = create_outputter(&args.format);

    // Determine if we need to do status checking (either explicitly requested or needed for filters)
    let should_check_status =
        args.check_status || !args.include_status.is_empty() || !args.exclude_status.is_empty();

    let mut final_urls = if should_check_status || args.extract_links {
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

    // Attach provider attribution to each surviving UrlData record when the
    // user opted in. URLs introduced by the link extractor — not present in
    // the run result — keep an empty `sources` list.
    if args.show_sources {
        for entry in final_urls.iter_mut() {
            if let Some(providers) = run_result.urls.get(&entry.url) {
                let mut sources: Vec<String> = providers.iter().cloned().collect();
                sources.sort();
                sources.dedup();
                entry.sources = sources;
            }
        }
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
                eprintln!("Error writing output: {e}");
            }
        }
    }

    if let Some(dir) = args.output_dir.clone() {
        if let Err(e) = write_per_domain_output(&final_urls, &dir, &args.format, args.silent) {
            if !args.silent {
                eprintln!("Error writing per-domain output to {}: {e}", dir.display());
            }
        } else if args.verbose && !args.silent {
            println!("Per-domain results written under: {}", dir.display());
        }
    }

    if args.stats && !args.silent {
        print_provider_stats(&run_result.stats);
    }

    Ok(())
}

/// Best-effort filename extension matching `--format`. Anything other than
/// json/csv falls back to `.txt`, mirroring how `create_outputter` treats
/// unknown formats as plain text.
fn output_dir_extension(format: &str) -> &'static str {
    match format.to_lowercase().as_str() {
        "json" => "json",
        "csv" => "csv",
        _ => "txt",
    }
}

/// Group URLs by their host and write one file per domain into `dir`.
/// URLs that fail to parse a host (rare after filtering) land in
/// `_unknown.<ext>` so nothing is silently dropped.
fn write_per_domain_output(
    urls: &[output::UrlData],
    dir: &std::path::Path,
    format: &str,
    silent: bool,
) -> anyhow::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }

    let mut grouped: std::collections::BTreeMap<String, Vec<output::UrlData>> =
        std::collections::BTreeMap::new();
    for entry in urls {
        let host = url::Url::parse(&entry.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "_unknown".to_string());
        grouped.entry(host).or_default().push(entry.clone());
    }

    let outputter = output::create_outputter(format);
    let ext = output_dir_extension(format);

    for (host, entries) in &grouped {
        let file_name = format!("{host}.{ext}");
        let path = dir.join(file_name);
        outputter.output(entries, Some(path), silent)?;
    }
    Ok(())
}

/// Render the per-provider summary table to stderr (so it doesn't pollute
/// stdout when callers pipe URL results into other tools).
fn print_provider_stats(stats: &[runner::ProviderStats]) {
    if stats.is_empty() {
        return;
    }
    eprintln!();
    eprintln!("Provider stats:");
    eprintln!(
        "  {:<18}  {:>8}  {:>7}  {:>10}",
        "provider", "urls", "errors", "elapsed"
    );
    eprintln!(
        "  {:<18}  {:>8}  {:>7}  {:>10}",
        "------------------", "--------", "-------", "----------"
    );
    for s in stats {
        let elapsed_ms = s.elapsed.as_millis();
        let elapsed_label = if elapsed_ms >= 1000 {
            format!("{:.2}s", s.elapsed.as_secs_f64())
        } else {
            format!("{}ms", elapsed_ms)
        };
        eprintln!(
            "  {:<18}  {:>8}  {:>7}  {:>10}",
            s.name, s.url_count, s.error_count, elapsed_label
        );
    }
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
        delay_ms: u64,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MockProvider {
        fn new(urls: Vec<String>, should_fail: bool) -> Self {
            MockProvider {
                urls,
                should_fail,
                delay_ms: 0,
                calls: Arc::new(Mutex::new(vec![])),
            }
        }

        fn with_delay_ms(mut self, ms: u64) -> Self {
            self.delay_ms = ms;
            self
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

            let delay = self.delay_ms;
            Box::pin(async move {
                // Record the call
                calls.lock().unwrap().push(domain.to_string());

                if delay > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }

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
            cc_index: vec!["CC-MAIN-2026-17".to_string()],
            vt_api_key: vec![],
            urlscan_api_key: vec![],
            zoomeye_api_key: vec![],
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
            exclude_providers: vec![],
            all_providers: false,
            list_providers: false,
            show_sources: false,
            stats: false,
            domain_list: vec![],
            max_time: 0,
            rate_limit_by: vec![],
            provider_config: None,
            output_dir: None,
            wayback_from: None,
            wayback_to: None,
            github_api_key: vec![],
        };

        let progress_manager = ProgressManager::new(true);

        // Process domains with mock provider
        let result = process_domains(
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

        // Verify that the URLs were correctly returned and attributed.
        assert_eq!(result.urls.len(), 2);
        assert!(result.urls.contains_key("https://example.com/page1"));
        assert!(result.urls.contains_key("https://example.com/page2"));
        assert!(result.urls["https://example.com/page1"].contains("MockProvider"));

        // Stats reflect the provider's URL count.
        assert_eq!(result.stats.len(), 1);
        assert_eq!(result.stats[0].name, "MockProvider");
        assert_eq!(result.stats[0].url_count, 2);
        assert_eq!(result.stats[0].error_count, 0);
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
        let progress_manager = ProgressManager::new(true);

        let started = std::time::Instant::now();
        let result = process_domains(
            vec!["example.com".to_string()],
            &args,
            &progress_manager,
            &providers,
            &provider_names,
        )
        .await;
        let elapsed = started.elapsed();

        // Should bail out well before the provider's 5s sleep finishes.
        assert!(
            elapsed.as_secs() < 4,
            "expected --max-time to abort within ~1s, got {:?}",
            elapsed
        );
        // No URLs were produced because the provider was cut off mid-await.
        assert!(
            result.urls.is_empty(),
            "expected no URLs, got {:?}",
            result.urls
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
        let progress_manager = ProgressManager::new(true);

        let result = process_domains(
            vec!["example.com".to_string()],
            &args,
            &progress_manager,
            &providers,
            &provider_names,
        )
        .await;

        assert!(result.urls.contains_key("https://example.com/page1"));
    }

    #[tokio::test]
    async fn test_create_cache_manager_invalid_type_errors() {
        let mut args = build_test_args();
        args.cache_type = "bogus".to_string();

        match create_cache_manager(&args).await {
            Ok(_) => panic!("expected invalid cache type to error"),
            Err(err) => assert!(err.to_string().contains("Invalid cache type")),
        }
    }

    #[test]
    fn test_output_dir_extension() {
        assert_eq!(output_dir_extension("json"), "json");
        assert_eq!(output_dir_extension("JSON"), "json");
        assert_eq!(output_dir_extension("csv"), "csv");
        assert_eq!(output_dir_extension("plain"), "txt");
        assert_eq!(output_dir_extension("anything-else"), "txt");
    }

    #[test]
    fn test_write_per_domain_output_groups_by_host() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let urls = vec![
            output::UrlData::new("https://example.com/a".to_string()),
            output::UrlData::new("https://example.com/b".to_string()),
            output::UrlData::new("https://other.test/x".to_string()),
            output::UrlData::new("not-a-url".to_string()),
        ];

        write_per_domain_output(&urls, dir.path(), "plain", true)?;

        let example = std::fs::read_to_string(dir.path().join("example.com.txt"))?;
        assert!(example.contains("https://example.com/a"));
        assert!(example.contains("https://example.com/b"));

        let other = std::fs::read_to_string(dir.path().join("other.test.txt"))?;
        assert!(other.contains("https://other.test/x"));

        // Unparseable URLs land in _unknown.txt instead of being dropped.
        let unknown = std::fs::read_to_string(dir.path().join("_unknown.txt"))?;
        assert!(unknown.contains("not-a-url"));
        Ok(())
    }

    #[test]
    fn test_write_per_domain_output_creates_missing_dir() -> anyhow::Result<()> {
        let base = tempfile::tempdir()?;
        let nested = base.path().join("nested/output/dir");
        let urls = vec![output::UrlData::new("https://example.com/a".to_string())];

        write_per_domain_output(&urls, &nested, "json", true)?;

        assert!(nested.is_dir());
        let example = std::fs::read_to_string(nested.join("example.com.json"))?;
        assert!(example.starts_with('['));
        assert!(example.contains("https://example.com/a"));
        Ok(())
    }

    #[test]
    fn test_collect_domains_merges_inputs_and_dedupes() -> anyhow::Result<()> {
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

    /// Helper to build a fully-defaulted Args for tests that only care about
    /// a couple of fields. Keep this in sync with the `Args` struct.
    fn build_test_args() -> Args {
        Args {
            domains: vec![],
            config: None,
            files: vec![],
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            normalize_url: false,
            providers: vec!["mock".to_string()],
            subs: false,
            cc_index: vec!["CC-MAIN-2026-17".to_string()],
            vt_api_key: vec![],
            urlscan_api_key: vec![],
            zoomeye_api_key: vec![],
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
            strict: false,
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
            include_robots: false,
            include_sitemap: false,
            exclude_robots: true,
            exclude_sitemap: true,
            incremental: false,
            cache_type: "sqlite".to_string(),
            cache_path: None,
            redis_url: None,
            cache_ttl: 86400,
            no_cache: false,
            exclude_providers: vec![],
            all_providers: false,
            list_providers: false,
            show_sources: false,
            stats: false,
            domain_list: vec![],
            max_time: 0,
            rate_limit_by: vec![],
            provider_config: None,
            output_dir: None,
            wayback_from: None,
            wayback_to: None,
            github_api_key: vec![],
        }
    }

    #[test]
    fn test_collect_domain_urls_matches_host_only() {
        let urls = std::collections::HashMap::from([
            (
                "https://example.com/path".to_string(),
                std::collections::HashSet::new(),
            ),
            (
                "https://notexample.com/redirect?next=example.com".to_string(),
                std::collections::HashSet::new(),
            ),
            (
                "https://example.com.evil.test/path".to_string(),
                std::collections::HashSet::new(),
            ),
            (
                "https://api.example.com/path".to_string(),
                std::collections::HashSet::new(),
            ),
        ]);

        let exact = collect_domain_urls(&urls, "example.com", false);
        assert_eq!(
            exact,
            std::collections::HashSet::from(["https://example.com/path".to_string()])
        );

        let with_subdomains = collect_domain_urls(&urls, "example.com", true);
        assert_eq!(
            with_subdomains,
            std::collections::HashSet::from([
                "https://example.com/path".to_string(),
                "https://api.example.com/path".to_string(),
            ])
        );
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
            cc_index: vec!["CC-MAIN-2026-17".to_string()],
            vt_api_key: vec![],
            urlscan_api_key: vec![],
            zoomeye_api_key: vec![],
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
            exclude_providers: vec![],
            all_providers: false,
            list_providers: false,
            show_sources: false,
            stats: false,
            domain_list: vec![],
            max_time: 0,
            rate_limit_by: vec![],
            provider_config: None,
            output_dir: None,
            wayback_from: None,
            wayback_to: None,
            github_api_key: vec![],
        };

        let progress_manager = ProgressManager::new(true);

        // Process URLs with mock tester
        let result_data = process_urls_with_testers(
            input_urls,
            &args,
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
