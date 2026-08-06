//! Fixtures shared by the unit tests across `src/`.
//!
//! Everything here is `#[cfg(test)]`-only. It lives at the crate root rather
//! than inside one test module so `app`, `runner`, and `tester_manager` tests
//! can share a single `Args` fixture and one set of mocks — previously each
//! test module carried its own copy of a 70-field `Args` literal, and they
//! drifted apart every time a flag was added.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::cli::Args;
use crate::providers::Provider;
use crate::testers::Tester;

/// Strip ANSI escapes so layout assertions hold regardless of the ambient
/// colour state — cargo runs tests in parallel and both `colored` and
/// `console` key off process-global toggles.
pub fn plain(s: &str) -> String {
    console::strip_ansi_codes(s).to_string()
}

/// Serializes tests that mutate environment variables. `std::env::set_var` is
/// process-wide, so without this the parallel test threads race each other.
pub fn env_mutex() -> &'static Mutex<()> {
    static INSTANCE: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(()))
}

/// Save the current values of `vars`, clear them, and restore them on drop.
///
/// Tests that read API keys have to run against a known-empty environment, but
/// an early `panic!` in an assertion would otherwise leak the cleared state
/// into every later test. Restoring from `Drop` makes that unwind-safe.
pub struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    /// Clear `vars` for the lifetime of the guard.
    pub fn unset(vars: &[&'static str]) -> Self {
        let saved = vars
            .iter()
            .map(|k| {
                let previous = std::env::var(k).ok();
                std::env::remove_var(k);
                (*k, previous)
            })
            .collect();
        Self { saved }
    }

    /// Set `vars` to the given values for the lifetime of the guard.
    pub fn set(vars: &[(&'static str, &str)]) -> Self {
        let saved = vars
            .iter()
            .map(|(k, v)| {
                let previous = std::env::var(k).ok();
                std::env::set_var(k, v);
                (*k, previous)
            })
            .collect();
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, previous) in &self.saved {
            match previous {
                Some(val) => std::env::set_var(key, val),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// A fully-defaulted [`Args`] for tests that only care about a field or two.
///
/// Defaults are chosen to keep tests hermetic: `silent`/`no_progress` suppress
/// console output, and robots/sitemap are excluded so provider selection is
/// driven purely by what the test sets.
pub fn build_test_args() -> Args {
    Args {
        domains: vec![],
        config: None,
        files: vec![],
        output: None,
        format: "plain".to_string(),
        merge_endpoint: false,
        normalize_url: false,
        stream: false,
        providers: vec!["mock".to_string()],
        subs: false,
        cc_index: vec!["CC-MAIN-2026-17".to_string()],
        vt_api_key: vec![],
        urlscan_api_key: vec![],
        zoomeye_api_key: vec![],
        verbose: false,
        silent: true,
        no_progress: true,
        no_color: false,
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
        no_strict: false,
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
        from: None,
        to: None,
        archive_status: vec![],
        archive_exclude_status: vec![],
        archive_mime: vec![],
        archive_exclude_mime: vec![],
        github_api_key: vec![],
    }
}

/// A [`Provider`] that returns a canned URL list, optionally after a delay or
/// as a failure, and records the domains it was asked about.
#[derive(Clone)]
pub struct MockProvider {
    urls: Vec<String>,
    should_fail: bool,
    delay_ms: u64,
    /// Domains passed to `fetch_urls`, in call order.
    pub calls: Arc<Mutex<Vec<String>>>,
}

impl MockProvider {
    pub fn new(urls: Vec<String>, should_fail: bool) -> Self {
        MockProvider {
            urls,
            should_fail,
            delay_ms: 0,
            calls: Arc::new(Mutex::new(vec![])),
        }
    }

    /// Sleep for `ms` inside `fetch_urls`, so concurrency and timeout tests can
    /// observe overlapping work.
    pub fn with_delay_ms(mut self, ms: u64) -> Self {
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
    fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
}

/// A [`Tester`] that echoes a fixed result list for every URL.
#[derive(Clone)]
pub struct MockStatusChecker {
    results: Vec<String>,
}

impl MockStatusChecker {
    pub fn new(results: Vec<String>) -> Self {
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
