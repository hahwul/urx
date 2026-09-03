//! `--archive-body`: mine the response bodies the archive stored.
//!
//! `--extract-links` fetches every URL from the *live* site, which is useless
//! for the pages an OSINT sweep is most interested in — the ones that no longer
//! exist. The Wayback Machine still holds their bodies, and this tester fetches
//! those instead: the raw bytes of each URL's newest capture, parsed with the
//! same HTML link extraction the live path uses.
//!
//! # Why this needs far fewer requests than waymore
//!
//! Every CDX row carries a content digest, and two captures with the same
//! digest are byte-identical. Archives are full of such duplicates: every
//! `?utm_source=` variant of a page, every `/index.html` next to its `/`, every
//! tracking-parameter permutation serves the same body, so a URL list of tens
//! of thousands routinely collapses to a few thousand distinct bodies. waymore
//! has no notion of this — it downloads one response per URL and copes with
//! the volume through a blunt `-l 5000` cap, which both hammers the archive
//! and truncates the coverage. urx claims each digest the first time it is
//! seen and skips every later URL that would replay the same bytes, so the
//! same coverage costs one request per *distinct body* rather than per URL.
//! `--archive-body-limit` still bounds the run, but it bounds unique bodies,
//! which is a much larger share of the target than the same number of URLs.

use anyhow::Result;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;
use url::Url;

use super::link_extractor::{is_html_like, LinkExtractor, MAX_BODY_BYTES};
use super::Tester;
use crate::network::client::{read_body_capped, HttpClientConfig};
use crate::network::RateLimiter;
use crate::providers::archived::{replay_url, WAYBACK_ORIGIN};

/// The capture to replay for one URL: when it was taken and, when the index
/// recorded one, the digest of the body that replay will return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCapture {
    pub timestamp: String,
    pub digest: Option<String>,
}

/// Counters shared between the extractor and the run that built it, so the
/// summary can say how much the digest deduplication actually saved.
///
/// Shared through `Arc` because the extractor is boxed into the tester list
/// and cloned per worker; the run keeps a handle to read the totals back.
#[derive(Debug, Default)]
pub struct ArchiveBodyStats {
    /// Bodies actually requested from the archive.
    fetched: AtomicUsize,
    /// URLs skipped because a body with the same digest was already claimed.
    duplicate_bodies: AtomicUsize,
    /// URLs skipped because `--archive-body-limit` had been reached.
    over_limit: AtomicUsize,
    /// URLs that carried no capture timestamp, so there was nothing to replay.
    no_capture: AtomicUsize,
}

impl ArchiveBodyStats {
    pub fn fetched(&self) -> usize {
        self.fetched.load(Ordering::Relaxed)
    }
    pub fn duplicate_bodies(&self) -> usize {
        self.duplicate_bodies.load(Ordering::Relaxed)
    }
    pub fn over_limit(&self) -> usize {
        self.over_limit.load(Ordering::Relaxed)
    }
    pub fn no_capture(&self) -> usize {
        self.no_capture.load(Ordering::Relaxed)
    }
}

/// Fetches the archived body of each URL it is handed and extracts the links
/// inside, replaying at most one body per distinct digest.
#[derive(Clone)]
pub struct ArchiveBodyExtractor {
    /// URL → the capture to replay. Built once from the run result; URLs with
    /// no capture timestamp (non-CDX providers, `--files`, cache hits) are
    /// simply absent.
    captures: Arc<HashMap<String, ArchiveCapture>>,
    /// Digests already claimed by an earlier URL. Shared across the cloned
    /// workers, so the deduplication holds under `--parallel`.
    claimed: Arc<Mutex<HashSet<String>>>,
    stats: Arc<ArchiveBodyStats>,
    /// Ceiling on bodies fetched in one run. Never unbounded: every fetch is a
    /// request to a public archive, and a large domain's URL list is easily
    /// six figures.
    limit: usize,
    rate_limit: Option<RateLimiter>,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    /// Built lazily and shared across workers, exactly as the live link
    /// extractor does, so every replay request reuses one connection pool.
    client: Arc<OnceCell<Client>>,
    /// Archive origin, overridable so tests can point at a mock server.
    origin: String,
}

impl ArchiveBodyExtractor {
    /// Build an extractor over `captures` that fetches at most `limit` bodies.
    pub fn new(captures: HashMap<String, ArchiveCapture>, limit: usize) -> Self {
        ArchiveBodyExtractor {
            captures: Arc::new(captures),
            claimed: Arc::new(Mutex::new(HashSet::new())),
            stats: Arc::new(ArchiveBodyStats::default()),
            limit,
            rate_limit: None,
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            client: Arc::new(OnceCell::new()),
            origin: WAYBACK_ORIGIN.to_string(),
        }
    }

    /// A handle on the counters, valid for the life of every clone.
    pub fn stats(&self) -> Arc<ArchiveBodyStats> {
        Arc::clone(&self.stats)
    }

    /// Number of URLs that have a capture to replay.
    pub fn candidate_count(&self) -> usize {
        self.captures.len()
    }

    /// Pace replay requests. The archive is one host no matter how many URLs
    /// are in flight, so the limiter is shared across workers.
    pub fn with_rate_limit(&mut self, requests_per_second: Option<f32>) -> &mut Self {
        self.rate_limit = RateLimiter::from_rate(requests_per_second);
        self
    }

    #[cfg(test)]
    pub fn with_origin(&mut self, origin: String) -> &mut Self {
        self.origin = origin;
        self
    }

    fn client_config(&self) -> HttpClientConfig {
        HttpClientConfig {
            timeout: self.timeout,
            insecure: self.insecure,
            random_agent: self.random_agent,
            proxy: self.proxy.clone(),
            proxy_auth: self.proxy_auth.clone(),
        }
    }

    async fn client(&self) -> Result<&Client> {
        self.client
            .get_or_try_init(|| async { self.client_config().build_client() })
            .await
    }

    /// Decide whether `url` is worth a request, and reserve the digest and a
    /// slot under the limit if it is.
    ///
    /// The reservation happens before the fetch, not after, so two workers
    /// holding URLs with the same digest cannot both decide to fetch it. The
    /// digest is claimed before the limit is checked: a URL whose body is
    /// already covered should not consume a slot.
    fn reserve(&self, url: &str) -> Option<&ArchiveCapture> {
        let Some(capture) = self.captures.get(url) else {
            self.stats.no_capture.fetch_add(1, Ordering::Relaxed);
            return None;
        };

        if let Some(digest) = &capture.digest {
            let mut claimed = self
                .claimed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !claimed.insert(digest.clone()) {
                self.stats.duplicate_bodies.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        }

        if self.stats.fetched.fetch_add(1, Ordering::Relaxed) >= self.limit {
            // Undo the optimistic increment so `fetched()` stays an honest
            // count of requests made.
            self.stats.fetched.fetch_sub(1, Ordering::Relaxed);
            self.stats.over_limit.fetch_add(1, Ordering::Relaxed);
            return None;
        }

        Some(capture)
    }
}

impl Tester for ArchiveBodyExtractor {
    fn clone_box(&self) -> Box<dyn Tester> {
        Box::new(self.clone())
    }

    /// Replay the URL's newest capture and extract the links in its body.
    ///
    /// Returns an empty list — never an error — for a URL that has nothing to
    /// replay, is a duplicate body, falls past the limit, or whose capture the
    /// archive does not serve: none of those are failures of the run.
    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let Some(capture) = self.reserve(url) else {
                return Ok(Vec::new());
            };
            // Relative links inside the body resolve against the page the
            // archive captured, not against the replay URL.
            let base_url =
                Url::parse(url).map_err(|_| anyhow::anyhow!("Failed to parse URL: {}", url))?;

            let client = self.client().await?;
            let target = replay_url(&self.origin, &capture.timestamp, url);

            let mut last_error = None;
            for attempt in 0..=self.retries {
                if let Some(rl) = &self.rate_limit {
                    rl.acquire().await;
                }
                match client.get(&target).send().await {
                    Ok(response) => {
                        // 404 means the Wayback Machine holds no capture of
                        // this URL (the timestamp may have come from another
                        // archive). Not an error; there is simply no body.
                        if !response.status().is_success() {
                            return Ok(Vec::new());
                        }
                        if !is_html_like(response.headers()) {
                            return Ok(Vec::new());
                        }
                        let body = read_body_capped(response, MAX_BODY_BYTES).await?;
                        return Ok(LinkExtractor::extract_links(&base_url, &body));
                    }
                    Err(e) => {
                        last_error = Some(e);
                        if attempt < self.retries {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                    }
                }
            }

            Err(anyhow::anyhow!(
                "Failed to fetch archived body of {}: {:?}",
                url,
                last_error
            ))
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(ts: &str, digest: Option<&str>) -> ArchiveCapture {
        ArchiveCapture {
            timestamp: ts.to_string(),
            digest: digest.map(str::to_string),
        }
    }

    fn extractor(entries: &[(&str, ArchiveCapture)], limit: usize) -> ArchiveBodyExtractor {
        ArchiveBodyExtractor::new(
            entries
                .iter()
                .map(|(u, c)| (u.to_string(), c.clone()))
                .collect(),
            limit,
        )
    }

    #[test]
    fn urls_without_a_capture_are_not_fetched() {
        let ex = extractor(&[], 10);
        assert!(ex.reserve("https://example.com/").is_none());
        assert_eq!(ex.stats().no_capture(), 1);
        assert_eq!(ex.stats().fetched(), 0);
    }

    #[test]
    fn the_same_digest_is_fetched_only_once() {
        // The whole point: /a and /b served identical bytes, so replaying both
        // is one request wasted.
        let ex = extractor(
            &[
                (
                    "https://example.com/a",
                    capture("20200101000000", Some("SAME")),
                ),
                (
                    "https://example.com/b",
                    capture("20210101000000", Some("SAME")),
                ),
                (
                    "https://example.com/c",
                    capture("20210101000000", Some("OTHER")),
                ),
            ],
            10,
        );
        assert!(ex.reserve("https://example.com/a").is_some());
        assert!(ex.reserve("https://example.com/b").is_none());
        assert!(ex.reserve("https://example.com/c").is_some());

        let stats = ex.stats();
        assert_eq!(stats.fetched(), 2);
        assert_eq!(stats.duplicate_bodies(), 1);
        assert_eq!(stats.over_limit(), 0);
    }

    #[test]
    fn a_url_without_a_digest_is_still_fetched() {
        // No digest means nothing to deduplicate on, not nothing to fetch.
        let ex = extractor(
            &[
                ("https://example.com/a", capture("20200101000000", None)),
                ("https://example.com/b", capture("20200101000000", None)),
            ],
            10,
        );
        assert!(ex.reserve("https://example.com/a").is_some());
        assert!(ex.reserve("https://example.com/b").is_some());
        assert_eq!(ex.stats().fetched(), 2);
    }

    #[test]
    fn the_limit_bounds_fetches_not_urls() {
        let ex = extractor(
            &[
                (
                    "https://example.com/a",
                    capture("20200101000000", Some("D1")),
                ),
                (
                    "https://example.com/a2",
                    capture("20200101000000", Some("D1")),
                ),
                (
                    "https://example.com/b",
                    capture("20200101000000", Some("D2")),
                ),
                (
                    "https://example.com/c",
                    capture("20200101000000", Some("D3")),
                ),
            ],
            2,
        );
        assert!(ex.reserve("https://example.com/a").is_some());
        // A duplicate body does not consume a slot...
        assert!(ex.reserve("https://example.com/a2").is_none());
        assert!(ex.reserve("https://example.com/b").is_some());
        // ...so the third distinct body is the one that hits the ceiling.
        assert!(ex.reserve("https://example.com/c").is_none());

        let stats = ex.stats();
        assert_eq!(
            stats.fetched(),
            2,
            "fetched must count requests, not attempts"
        );
        assert_eq!(stats.duplicate_bodies(), 1);
        assert_eq!(stats.over_limit(), 1);
    }

    #[test]
    fn deduplication_is_shared_across_worker_clones() {
        // The tester stage hands each worker a clone_box; the claimed set has
        // to be the same set in every one of them.
        let ex = extractor(
            &[
                (
                    "https://example.com/a",
                    capture("20200101000000", Some("SAME")),
                ),
                (
                    "https://example.com/b",
                    capture("20200101000000", Some("SAME")),
                ),
            ],
            10,
        );
        let worker = ex.clone();
        assert!(ex.reserve("https://example.com/a").is_some());
        assert!(worker.reserve("https://example.com/b").is_none());
        assert_eq!(ex.stats().fetched(), 1);
        assert_eq!(worker.stats().duplicate_bodies(), 1);
    }

    #[tokio::test]
    async fn replays_the_raw_capture_and_extracts_its_links() {
        let mut server = mockito::Server::new_async().await;
        let replay = server
            .mock(
                "GET",
                "/web/20200101000000id_/https://example.com/gone/page.html",
            )
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(
                r#"<a href="/still-here">x</a><script src="assets/old.js"></script>
                   <a href="https://other.test/x">off-site</a>"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/gone/page.html",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());

        let links = ex
            .test_url("https://example.com/gone/page.html")
            .await
            .unwrap();
        // Relative links resolve against the *captured* URL, not the replay URL.
        assert_eq!(
            links,
            vec![
                "https://example.com/still-here".to_string(),
                "https://example.com/gone/assets/old.js".to_string(),
                "https://other.test/x".to_string(),
            ]
        );
        replay.assert();
    }

    #[tokio::test]
    async fn a_capture_the_archive_does_not_serve_is_skipped_quietly() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(404)
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/x",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_retries(0);

        let links = ex.test_url("https://example.com/x").await.unwrap();
        assert!(links.is_empty());
    }

    #[tokio::test]
    async fn non_markup_bodies_are_not_parsed() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "image/png")
            .with_body(r#"<a href="/not-really">x</a>"#)
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/logo.png",
                capture("20200101000000", None),
            )],
            10,
        );
        ex.with_origin(server.url());

        let links = ex.test_url("https://example.com/logo.png").await.unwrap();
        assert!(links.is_empty());
    }

    #[tokio::test]
    async fn a_duplicate_body_costs_no_request() {
        let mut server = mockito::Server::new_async().await;
        let replay = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<a href="/found">x</a>"#)
            .expect(1)
            .create_async()
            .await;

        let mut ex = extractor(
            &[
                (
                    "https://example.com/a",
                    capture("20200101000000", Some("SAME")),
                ),
                (
                    "https://example.com/a?utm_source=x",
                    capture("20200101000000", Some("SAME")),
                ),
            ],
            10,
        );
        ex.with_origin(server.url());

        let first = ex.test_url("https://example.com/a").await.unwrap();
        let second = ex
            .test_url("https://example.com/a?utm_source=x")
            .await
            .unwrap();
        assert_eq!(first, vec!["https://example.com/found".to_string()]);
        assert!(second.is_empty());
        replay.assert();
    }

    #[tokio::test]
    async fn rate_limit_paces_replay_requests() {
        use std::time::{Duration, Instant};
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<p>empty</p>")
            .expect(2)
            .create_async()
            .await;

        let mut ex = extractor(
            &[
                (
                    "https://example.com/a",
                    capture("20200101000000", Some("D1")),
                ),
                (
                    "https://example.com/b",
                    capture("20200101000000", Some("D2")),
                ),
            ],
            10,
        );
        ex.with_origin(server.url());
        // 5 req/s => a 200ms minimum gap before the second request.
        ex.with_rate_limit(Some(5.0));

        let start = Instant::now();
        ex.test_url("https://example.com/a").await.unwrap();
        ex.test_url("https://example.com/b").await.unwrap();
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "rate limit was not applied; elapsed {:?}",
            start.elapsed()
        );
    }
}
