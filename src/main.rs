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
use filters::{HostValidator, UrlFilter};
use network::NetworkSettings;
use output::create_outputter;
use progress::ProgressManager;
use providers::{
    CommonCrawlProvider, OTXProvider, Provider, RobotsProvider, SitemapProvider, UrlscanProvider,
    VirusTotalProvider, WaybackMachineProvider,
};
use runner::{add_provider, process_domains};
use tester_manager::{apply_network_settings_to_tester, process_urls_with_testers};
use testers::{LinkExtractor, StatusChecker, Tester};
use url_utils::UrlTransformer;
use utils::verbose_print;

/// Helper function to auto-enable providers if API key is present
pub fn auto_enable_provider(
    providers_list: &mut Vec<String>,
    api_key: &Option<String>,
    provider_name: &str,
    verbose: bool,
    silent: bool,
) {
    if api_key.is_some() && !providers_list.iter().any(|p| p == provider_name) {
        providers_list.push(provider_name.to_string());
        if verbose && !silent {
            println!(
                "Auto-enabling {} provider because API key is provided",
                provider_name
            );
        }
    }
}

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

    // Initialize providers based on command-line flags and API keys
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    let mut provider_names: Vec<String> = Vec::new();

    // Get VirusTotal and Urlscan API keys
    let vt_api_key = args
        .vt_api_key
        .clone()
        .or_else(|| std::env::var("URX_VT_API_KEY").ok());

    let urlscan_api_key = args
        .urlscan_api_key
        .clone()
        .or_else(|| std::env::var("URX_URLSCAN_API_KEY").ok());

    // Auto-enable providers if API keys are provided but not explicitly included in providers
    let mut providers_list = args.providers.clone();

    // Auto-enable VirusTotal and Urlscan providers
    auto_enable_provider(
        &mut providers_list,
        &vt_api_key,
        "vt",
        args.verbose,
        args.silent,
    );
    auto_enable_provider(
        &mut providers_list,
        &urlscan_api_key,
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
        if let Some(api_key) = vt_api_key.clone() {
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

    if providers_list.iter().any(|p| p == "urlscan") {
        if let Some(api_key) = urlscan_api_key.clone() {
            add_provider(
                &args,
                &network_settings,
                &mut providers,
                &mut provider_names,
                "Urlscan".to_string(),
                || UrlscanProvider::new(api_key.clone()),
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

    // Process each domain
    let all_urls = process_domains(
        domains.clone(),
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

    // Apply URL filters
    let mut sorted_urls = url_filter.apply_filters(&all_urls);

    // Apply host validation if strict mode is enabled
    if args.strict {
        if args.verbose && !args.silent {
            println!("Enforcing strict host validation...");
        }
        let host_validator = HostValidator::new(&domains);
        sorted_urls.retain(|url| host_validator.is_valid_host(url));

        if args.verbose && !args.silent {
            println!(
                "Number of valid URLs after host validation: {}",
                sorted_urls.len()
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::collections::HashSet;
    use std::env;

    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_auto_enable_provider() {
        // Test the auto_enable_provider helper function directly
        let mut providers_list = vec!["wayback".to_string(), "cc".to_string()];
        let api_key = Some("test_api_key".to_string());

        // Should add vt to the list
        auto_enable_provider(&mut providers_list, &api_key, "vt", false, false);
        assert!(providers_list.contains(&"vt".to_string()));
        assert_eq!(providers_list.len(), 3);

        // Calling again shouldn't add duplicates
        auto_enable_provider(&mut providers_list, &api_key, "vt", false, false);
        assert_eq!(providers_list.len(), 3);

        // Empty API key should not add the provider
        let empty_key: Option<String> = None;
        auto_enable_provider(&mut providers_list, &empty_key, "urlscan", false, false);
        assert!(!providers_list.contains(&"urlscan".to_string()));
        assert_eq!(providers_list.len(), 3);
    }

    #[test]
    fn test_auto_enable_providers_with_env_vars() {
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

        // Get API keys (this simulates part of main function)
        let vt_api_key = args
            .vt_api_key
            .clone()
            .or_else(|| std::env::var("URX_VT_API_KEY").ok());

        let urlscan_api_key = args
            .urlscan_api_key
            .clone()
            .or_else(|| std::env::var("URX_URLSCAN_API_KEY").ok());

        // Test auto-enabling providers
        auto_enable_provider(&mut providers_list, &vt_api_key, "vt", false, false);
        auto_enable_provider(
            &mut providers_list,
            &urlscan_api_key,
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
    fn test_api_key_precedence() {
        // This test verifies command-line arguments take precedence over env vars

        // Save current environment
        let old_vt_key = env::var("URX_VT_API_KEY").ok();

        // Set environment variable
        env::set_var("URX_VT_API_KEY", "env_vt_key");

        // Create args with explicit API key
        let args = Args::parse_from(["urx", "example.com", "--vt-api-key", "arg_vt_key"]);

        // Verify command line arg takes precedence
        let vt_api_key = args
            .vt_api_key
            .clone()
            .or_else(|| std::env::var("URX_VT_API_KEY").ok());

        assert_eq!(vt_api_key, Some("arg_vt_key".to_string()));

        // Create args without explicit API key
        let args = Args::parse_from(["urx", "example.com"]);

        // Verify environment variable is used as fallback
        let vt_api_key = args
            .vt_api_key
            .clone()
            .or_else(|| std::env::var("URX_VT_API_KEY").ok());

        assert_eq!(vt_api_key, Some("env_vt_key".to_string()));

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
            // Removed duplicate field
            domains: vec!["example.com".to_string()],
            config: None,
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            providers: vec!["mock".to_string()],
            subs: false,
            cc_index: "CC-MAIN-2025-13".to_string(),
            vt_api_key: None,
            urlscan_api_key: None,
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
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            providers: vec![],
            subs: false,
            cc_index: "CC-MAIN-2025-13".to_string(),
            vt_api_key: None,
            urlscan_api_key: None,
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
