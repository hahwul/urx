//! Fixtures shared by the unit tests across `src/`.
//!
//! Everything here is `#[cfg(test)]`-only. It lives at the crate root rather
//! than inside one test module so `app`, `runner`, and `tester_manager` tests
//! can share a single `Args` fixture and one set of mocks.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::cli::Args;
use crate::output::UrlData;
use crate::progress::ProgressReporter;
use crate::providers::{Provider, UrlRecord};
use crate::testers::Tester;

/// Strip ANSI escapes so layout assertions hold regardless of the ambient
/// colour state — cargo runs tests in parallel and `console` keys off
/// process-global toggles.
pub fn plain(s: &str) -> String {
    console::strip_ansi_codes(s).to_string()
}

/// Serializes tests that mutate environment variables. `std::env::set_var` is
/// process-wide, so without this the parallel test threads race each other.
pub static ENV: Mutex<()> = Mutex::new(());

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
    use clap::Parser;
    let mut args = Args::parse_from([
        "urx",
        "--silent",
        "--no-progress",
        "--exclude-robots",
        "--exclude-sitemap",
    ]);
    args.providers = vec!["mock".to_string()];
    args.cc_index = vec!["CC-MAIN-2026-17".to_string()];
    // Pinned rather than taken from clap, so a CLI default change can't
    // silently move every test built on this fixture.
    args.timeout = 30;
    args.retries = 3;
    // clap defaults these to true; the fixture keeps them off.
    args.strict = false;
    args.include_robots = false;
    args.include_sitemap = false;
    args
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

#[async_trait::async_trait]
#[async_trait::async_trait]
impl Provider for MockProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn fetch_urls_with_progress(
        &self,
        domain: &str,
        _reporter: ProgressReporter,
    ) -> Result<Vec<UrlRecord>> {
        self.calls.lock().unwrap().push(domain.to_string());

        if self.delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        }

        if self.should_fail {
            Err(anyhow::anyhow!("Mock provider failure"))
        } else {
            Ok(self.urls.iter().cloned().map(UrlRecord::bare).collect())
        }
    }

    fn with_subdomains(&mut self, _include: bool) {}
    fn with_network(&mut self, _net: crate::network::NetConfig) {}
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
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlData>>> + Send + 'a>> {
        let results = self.results.iter().cloned().map(UrlData::new).collect();
        Box::pin(async move { Ok(results) })
    }
}
