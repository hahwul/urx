use futures::stream::{self, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::cli::Args;
use crate::network::{NetworkScope, NetworkSettings};
use crate::output;
use crate::progress::ProgressManager;
use crate::testers::Tester;
use crate::utils::verbose_print;

/// Helper function to apply network settings to a tester
pub fn apply_network_settings_to_tester(tester: &mut dyn Tester, settings: &NetworkSettings) {
    // Skip applying settings if network scope doesn't include testers
    if settings.scope == NetworkScope::Providers {
        return;
    }

    tester.with_timeout(settings.timeout);
    tester.with_retries(settings.retries);
    tester.with_random_agent(settings.random_agent);
    tester.with_insecure(settings.insecure);

    if let Some(proxy) = &settings.proxy {
        tester.with_proxy(Some(proxy.clone()));

        if let Some(auth) = &settings.proxy_auth {
            tester.with_proxy_auth(Some(auth.clone()));
        }
    }
}

/// Process URLs with tester components (status checker, link extractor, etc.)
pub async fn process_urls_with_testers(
    transformed_urls: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    testers: Vec<Box<dyn Tester>>,
    should_check_status: bool,
) -> Vec<output::UrlData> {
    verbose_print(args, "Applying testing options...");

    // Create progress bar for testing
    let test_bar = progress_manager.create_test_bar(transformed_urls.len());
    test_bar.set_message("Preparing URL testing...");

    // Process URLs with testers.
    //
    // Concurrency is bounded by --parallel. The previous implementation spawned
    // one task per 10-URL chunk and launched them all at once, so a run over
    // tens of thousands of URLs could open thousands of simultaneous
    // connections — exhausting file descriptors and hammering the target. We
    // instead stream URL chunks through `buffer_unordered`, keeping at most
    // `parallel` chunks in flight at a time, and advance the progress bar as
    // each URL actually completes (not when its task is merely scheduled).
    let parallel = args.parallel.unwrap_or(5).max(1) as usize;
    let total = transformed_urls.len() as u64;
    let completed = Arc::new(AtomicU64::new(0));

    let verbose = args.verbose;
    let check_status = should_check_status;
    let extract_links = args.extract_links;
    let silent = args.silent;
    // With an --include-status allowlist, a URL whose status we could never
    // resolve has not been shown to match it. Emitting it with a placeholder
    // status would smuggle it past the very filter the user asked for — so an
    // allowlist drops unresolvable URLs. An --exclude-status denylist is the
    // other way round: a failed check matched nothing on the list, so the URL
    // is kept (flagged) rather than silently discarded.
    let drop_unresolved = !args.include_status.is_empty();

    let url_chunks: Vec<Vec<String>> = transformed_urls
        .chunks(10)
        .map(|chunk| chunk.to_vec())
        .collect();

    let chunk_results: Vec<Vec<output::UrlData>> =
        stream::iter(url_chunks.into_iter().map(|url_vec| {
            let testers_clone: Vec<_> = testers.iter().map(|t| t.clone_box()).collect();
            let test_bar = test_bar.clone();
            let completed = Arc::clone(&completed);

            async move {
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
                                    eprintln!("Error testing URL {url}: {e}");
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
                            if !drop_unresolved {
                                let url_data = output::UrlData::with_status(
                                    url.clone(),
                                    "Status check failed".to_string(),
                                );
                                result_urls.push(url_data);
                            }
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

                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    test_bar.set_position(done.min(total));
                }

                result_urls
            }
        }))
        .buffer_unordered(parallel)
        .collect()
        .await;

    let mut new_urls = Vec::new();
    for urls in chunk_results {
        new_urls.extend(urls);
    }

    // Sort URLs by their URL field
    new_urls.sort_by(|a, b| a.url.cmp(&b.url));

    test_bar.finish_with_message(format!("Testing complete, found {} URLs", new_urls.len()));

    if args.verbose && !args.silent {
        println!("Testing complete, final URL count: {}", new_urls.len());
    }

    new_urls
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::future::Future;
    use std::pin::Pin;

    /// Mock tester for testing apply_network_settings_to_tester
    #[derive(Clone, Default)]
    struct MockTester {
        timeout: u64,
        retries: u32,
        random_agent: bool,
        insecure: bool,
        proxy: Option<String>,
        proxy_auth: Option<String>,
    }

    impl MockTester {
        fn new() -> Self {
            MockTester::default()
        }
    }

    impl Tester for MockTester {
        fn clone_box(&self) -> Box<dyn Tester> {
            Box::new(self.clone())
        }

        fn test_url<'a>(
            &'a self,
            url: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
            let url = url.to_string();
            Box::pin(async move { Ok(vec![url]) })
        }

        fn with_timeout(&mut self, seconds: u64) {
            self.timeout = seconds;
        }

        fn with_retries(&mut self, count: u32) {
            self.retries = count;
        }

        fn with_random_agent(&mut self, enabled: bool) {
            self.random_agent = enabled;
        }

        fn with_insecure(&mut self, enabled: bool) {
            self.insecure = enabled;
        }

        fn with_proxy(&mut self, proxy: Option<String>) {
            self.proxy = proxy;
        }

        fn with_proxy_auth(&mut self, auth: Option<String>) {
            self.proxy_auth = auth;
        }
    }

    /// A tester whose every request fails, standing in for an unreachable host.
    #[derive(Clone, Default)]
    struct FailingTester;

    impl Tester for FailingTester {
        fn clone_box(&self) -> Box<dyn Tester> {
            Box::new(self.clone())
        }

        fn test_url<'a>(
            &'a self,
            url: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
            let url = url.to_string();
            Box::pin(async move { Err(anyhow::anyhow!("connection refused for {url}")) })
        }

        fn with_timeout(&mut self, _seconds: u64) {}
        fn with_retries(&mut self, _count: u32) {}
        fn with_random_agent(&mut self, _enabled: bool) {}
        fn with_insecure(&mut self, _enabled: bool) {}
        fn with_proxy(&mut self, _proxy: Option<String>) {}
        fn with_proxy_auth(&mut self, _auth: Option<String>) {}
    }

    async fn run_failing_status_check(argv: &[&str]) -> Vec<output::UrlData> {
        use clap::Parser;
        let args = Args::parse_from(argv);
        let progress = ProgressManager::new(true);
        process_urls_with_testers(
            vec!["https://example.com/a".to_string()],
            &args,
            &progress,
            vec![Box::new(FailingTester)],
            true,
        )
        .await
    }

    #[tokio::test]
    async fn test_include_status_drops_urls_whose_status_never_resolved() {
        // Regression: an unreachable URL was emitted with a placeholder
        // "Status check failed" status even under --include-status 200, so the
        // allowlist leaked URLs that were never shown to return 200.
        let out =
            run_failing_status_check(&["urx", "--is", "200", "--silent", "example.com"]).await;
        assert!(out.is_empty(), "{out:?}");
    }

    #[tokio::test]
    async fn test_plain_check_status_still_reports_the_failure() {
        // Without an allowlist there is nothing to leak past, and the failure is
        // itself information worth surfacing.
        let out =
            run_failing_status_check(&["urx", "--check-status", "--silent", "example.com"]).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status.as_deref(), Some("Status check failed"));
    }

    #[tokio::test]
    async fn test_exclude_status_keeps_urls_whose_status_never_resolved() {
        // A denylist is the inverse: a failed check matched nothing on the list.
        let out =
            run_failing_status_check(&["urx", "--es", "404", "--silent", "example.com"]).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status.as_deref(), Some("Status check failed"));
    }

    #[test]
    fn test_apply_network_settings_to_tester_basic() {
        let mut tester = MockTester::new();
        let settings = NetworkSettings::new()
            .with_timeout(60)
            .with_retries(5)
            .with_random_agent(true)
            .with_insecure(true);

        apply_network_settings_to_tester(&mut tester, &settings);

        assert_eq!(tester.timeout, 60);
        assert_eq!(tester.retries, 5);
        assert!(tester.random_agent);
        assert!(tester.insecure);
    }

    #[test]
    fn test_apply_network_settings_to_tester_with_proxy() {
        let mut tester = MockTester::new();
        let settings = NetworkSettings::new()
            .with_proxy(Some("http://proxy:8080".to_string()))
            .with_proxy_auth(Some("user:pass".to_string()));

        apply_network_settings_to_tester(&mut tester, &settings);

        assert_eq!(tester.proxy, Some("http://proxy:8080".to_string()));
        assert_eq!(tester.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_apply_network_settings_to_tester_skips_for_providers_scope() {
        let mut tester = MockTester::new();
        let mut settings = NetworkSettings::new()
            .with_timeout(60)
            .with_retries(5)
            .with_random_agent(true)
            .with_insecure(true);
        settings.scope = NetworkScope::Providers;

        apply_network_settings_to_tester(&mut tester, &settings);

        // Settings should not be applied when scope is Providers
        assert_eq!(tester.timeout, 0);
        assert_eq!(tester.retries, 0);
        assert!(!tester.random_agent);
        assert!(!tester.insecure);
    }

    #[test]
    fn test_apply_network_settings_to_tester_applies_for_testers_scope() {
        let mut tester = MockTester::new();
        let mut settings = NetworkSettings::new()
            .with_timeout(60)
            .with_retries(5)
            .with_random_agent(true)
            .with_insecure(true);
        settings.scope = NetworkScope::Testers;

        apply_network_settings_to_tester(&mut tester, &settings);

        // Settings should be applied when scope is Testers
        assert_eq!(tester.timeout, 60);
        assert_eq!(tester.retries, 5);
        assert!(tester.random_agent);
        assert!(tester.insecure);
    }

    #[test]
    fn test_apply_network_settings_to_tester_applies_for_all_scope() {
        let mut tester = MockTester::new();
        let mut settings = NetworkSettings::new()
            .with_timeout(60)
            .with_retries(5)
            .with_random_agent(true)
            .with_insecure(true);
        settings.scope = NetworkScope::All;

        apply_network_settings_to_tester(&mut tester, &settings);

        // Settings should be applied when scope is All
        assert_eq!(tester.timeout, 60);
        assert_eq!(tester.retries, 5);
        assert!(tester.random_agent);
        assert!(tester.insecure);
    }

    #[test]
    fn test_apply_network_settings_proxy_without_auth() {
        let mut tester = MockTester::new();
        let settings = NetworkSettings::new().with_proxy(Some("http://proxy:8080".to_string()));

        apply_network_settings_to_tester(&mut tester, &settings);

        assert_eq!(tester.proxy, Some("http://proxy:8080".to_string()));
        assert_eq!(tester.proxy_auth, None);
    }
}
