//! Small helpers shared by more than one tester.
//!
//! Only things a tester already used and another needs *verbatim* belong here.
//! The noise policies stay with the tester whose regression tests define them:
//! `js_endpoint_extractor::is_noise` exists because minified bundles produce
//! specific garbage shapes, and none of that applies to a document that lists
//! its endpoints outright.

use reqwest::header::{HeaderMap, CONTENT_TYPE};
use reqwest::{RequestBuilder, Response};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

use crate::network::RateLimiter;
use crate::output::UrlData;

/// The extension of the last path segment of `url`, lower-cased, if any.
///
/// Lifted unchanged out of `js_endpoint_extractor` when `spec_expander` needed
/// the same test: both decide whether a URL is worth a request from the shape
/// of its file name, and both must agree that `.htaccess`, a trailing dot, and
/// a long non-alphanumeric tail are not extensions.
pub(super) fn path_extension(url: &Url) -> Option<String> {
    let last = url.path_segments()?.next_back()?;
    let (_, ext) = last.rsplit_once('.')?;
    // `.htaccess`-style names and trailing dots are not extensions.
    if ext.is_empty() || ext.len() > 5 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Discovered URLs as tester results.
pub(super) fn found(urls: Vec<String>) -> Vec<UrlData> {
    urls.into_iter().map(UrlData::new).collect()
}

/// The response's `Content-Type`, lower-cased, if it sent a readable one.
pub(super) fn content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase())
}

/// Send `request`, retrying transport errors up to `retries` more times, 500ms
/// apart, each attempt paced by `limiter`.
///
/// Any response ends the loop, whatever its status: a tester reads a non-2xx
/// as "nothing here", not as a reason to ask again. That is why the testers do
/// not share [`crate::network::client::send_with_retry`], which retries
/// statuses and buffers the body before the caller has seen the headers.
pub(super) async fn send(
    retries: u32,
    limiter: Option<&RateLimiter>,
    request: impl Fn() -> RequestBuilder,
) -> Result<Response, reqwest::Error> {
    let mut attempt = 0;
    loop {
        if let Some(limiter) = limiter {
            limiter.acquire().await;
        }
        match request().send().await {
            Err(_) if attempt < retries => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            result => return result,
        }
    }
}

/// A run-wide cap on fetches, shared across `clone_box` clones so it holds
/// globally rather than per worker. `max == 0` is unlimited.
#[derive(Clone)]
pub(super) struct FetchBudget {
    pub(super) max: usize,
    fetched: Arc<AtomicUsize>,
}

impl FetchBudget {
    pub(super) fn new(max: usize) -> Self {
        FetchBudget {
            max,
            fetched: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Reserve one slot under the cap, or `false` if the cap is spent.
    pub(super) fn try_reserve(&self) -> bool {
        if self.max == 0 {
            self.fetched.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        self.fetched
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < self.max).then_some(n + 1)
            })
            .is_ok()
    }

    /// Fetches reserved so far.
    #[cfg(test)]
    pub(super) fn fetched(&self) -> usize {
        self.fetched.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_extension_reads_only_real_extensions() {
        let ext = |u: &str| path_extension(&Url::parse(u).unwrap());
        assert_eq!(ext("https://x/a/swagger.json").as_deref(), Some("json"));
        assert_eq!(ext("https://x/OPENAPI.YAML").as_deref(), Some("yaml"));
        // No extension at all, a bare dotfile, a trailing dot, and a tail too
        // long or too odd to be one.
        assert_eq!(ext("https://x/v3/api-docs"), None);
        assert_eq!(ext("https://x/.htaccess"), None);
        assert_eq!(ext("https://x/name."), None);
        assert_eq!(ext("https://x/a.verylong"), None);
        assert_eq!(ext("https://x/a.js%20x"), None);
    }
}
