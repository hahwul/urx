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
//!
//! # What a replayed body is worth beyond its links
//!
//! Once a body has been fetched, the marginal cost of doing more with it is
//! zero, so two other flags compose with this one rather than duplicating its
//! requests:
//!
//! - `--extract-js-endpoints` mines *archived* scripts. This is the case the
//!   live path structurally cannot reach: a bundle is named by build hash, so
//!   `app.a3f9c2.js` 404s the moment the site redeploys, and with it goes the
//!   string table where a modern app's entire API surface lives. The archive
//!   still holds those bytes.
//! - `--archive-body-dir` writes each body to disk, so the run leaves behind a
//!   corpus to grep for the things no extractor looks for: comments, inlined
//!   credentials, internal hostnames. See [`super::body_archive`].

use anyhow::Result;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;
use url::Url;

use super::body_archive::BodyArchive;
use super::js_endpoint_extractor::{classify, BodyKind};
use super::link_extractor::{is_html_like, LinkExtractor, MAX_BODY_BYTES};
// --- spec-expansion ---
use super::spec_expander::{expand_spec_body, spec_body_kind};
use super::{JsEndpointExtractor, Tester};
use crate::network::client::{read_body_capped, HttpClientConfig};
use crate::network::CustomHeaders;
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
    // --- spec-expansion ---
    /// Whether `--expand-specs` is also on, in which case an archived
    /// specification is read as one instead of being handed to the HTML link
    /// extractor. See [`Tester::test_url`] for why the combination matters.
    expand_specs: bool,
    /// Whether `--extract-js-endpoints` is also on, in which case an archived
    /// script body is mined for endpoints instead of being discarded, and an
    /// archived page's inline `<script>` blocks are mined alongside its links.
    extract_js_endpoints: bool,
    /// `--archive-body-dir`: where to keep each replayed body, if anywhere.
    /// Shared across the cloned workers so they write into one corpus and one
    /// index.
    body_archive: Option<Arc<BodyArchive>>,
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
            // --- spec-expansion ---
            expand_specs: false,
            extract_js_endpoints: false,
            body_archive: None,
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

    // --- spec-expansion ---
    /// Read archived API specifications as specifications, as `--expand-specs`
    /// does for live ones.
    pub fn with_expand_specs(&mut self, enabled: bool) -> &mut Self {
        self.expand_specs = enabled;
        self
    }

    /// Whether archived specifications will be expanded.
    #[cfg(test)]
    pub fn expands_specs(&self) -> bool {
        self.expand_specs
    }

    /// Mine archived script bodies for endpoints, as `--extract-js-endpoints`
    /// does for live ones.
    pub fn with_extract_js_endpoints(&mut self, enabled: bool) -> &mut Self {
        self.extract_js_endpoints = enabled;
        self
    }

    /// Whether archived scripts will be mined.
    #[cfg(test)]
    pub fn extracts_js_endpoints(&self) -> bool {
        self.extract_js_endpoints
    }

    /// Keep every replayed body in `archive` (`--archive-body-dir`).
    pub fn with_body_archive(&mut self, archive: Option<Arc<BodyArchive>>) -> &mut Self {
        self.body_archive = archive;
        self
    }

    /// A handle on the corpus, valid for the life of every clone, so the run
    /// summary can report what was stored.
    pub fn body_archive(&self) -> Option<Arc<BodyArchive>> {
        self.body_archive.clone()
    }

    fn client_config(&self) -> HttpClientConfig {
        HttpClientConfig {
            headers: CustomHeaders::default(),
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

    /// Write one replayed body to `--archive-body-dir`, if that flag is on.
    ///
    /// A failed write is reported and then dropped rather than failing the
    /// URL: the corpus is a by-product of the run, and a full disk three hours
    /// into a replay should not discard the links already being collected.
    /// Creation of the directory was checked up front for exactly this reason,
    /// so anything failing here is an unusual, per-file problem.
    async fn persist(
        &self,
        url: &str,
        capture: &ArchiveCapture,
        content_type: &Option<String>,
        body: &str,
    ) {
        let Some(archive) = &self.body_archive else {
            return;
        };
        if let Err(e) = archive
            .store(
                url,
                &capture.timestamp,
                capture.digest.as_deref(),
                content_type.as_deref(),
                body,
            )
            .await
        {
            eprintln!("[urx] --archive-body-dir: failed to store {url}: {e}");
        }
    }
}

/// Whether a replayed body is text worth keeping in `--archive-body-dir`.
///
/// The link extractor only ever wanted markup, but a corpus meant for `grep`
/// wants every *readable* body: a JSON config, a stylesheet with a commented
/// staging URL, a `.txt` left in the webroot. What it does not want is the
/// site's images, fonts and video, which would dominate the directory in both
/// count and bytes while containing nothing anyone will grep for.
///
/// Decided from `Content-Type` when the archive replayed one, and from the
/// path extension when it did not — the same two signals, in the same order,
/// that [`classify`] and [`is_html_like`] use.
fn is_text_like(headers: &reqwest::header::HeaderMap, url: &Url) -> bool {
    if let Some(ct) = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        let ct = ct.to_ascii_lowercase();
        let ct = ct.split(';').next().unwrap_or(&ct).trim().to_string();
        return ct.starts_with("text/")
            || ct.ends_with("+json")
            || ct.ends_with("+xml")
            || matches!(
                ct.as_str(),
                "application/json"
                    | "application/xml"
                    | "application/javascript"
                    | "application/x-javascript"
                    | "application/ecmascript"
                    | "application/graphql"
                    | "application/yaml"
                    | "application/x-yaml"
                    | "application/xhtml+xml"
                    | "application/x-httpd-php"
                    | "application/sql"
            );
    }

    // No type at all: trust the extension, and treat "no extension" as a
    // server-rendered page, which is what an extensionless archived URL almost
    // always is.
    match super::shared::path_extension(url) {
        None => true,
        Some(ext) => matches!(
            ext.as_str(),
            "html"
                | "htm"
                | "xhtml"
                | "shtml"
                | "php"
                | "asp"
                | "aspx"
                | "jsp"
                | "do"
                | "js"
                | "mjs"
                | "cjs"
                | "jsx"
                | "ts"
                | "tsx"
                | "json"
                | "xml"
                | "yaml"
                | "yml"
                | "css"
                | "scss"
                | "less"
                | "txt"
                | "md"
                | "csv"
                | "svg"
                | "map"
                | "sql"
                | "conf"
                | "ini"
                | "env"
                | "log"
        ),
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
                        // Read before `read_body_capped` consumes the
                        // response; the index records it verbatim.
                        let content_type = response
                            .headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);
                        // --- spec-expansion ---
                        // The combination that recovers an API which no longer
                        // exists: the archive still holds the specification
                        // that described it. The replay is `id_`, so the body
                        // and its `Content-Type` are the originals, and the
                        // same two signals decide as on the live path. Checked
                        // before `is_html_like`, which rejects
                        // `application/json` outright.
                        if self.expand_specs {
                            if let Some(kind) = spec_body_kind(response.headers(), &base_url) {
                                let body = read_body_capped(response, MAX_BODY_BYTES).await?;
                                self.persist(url, capture, &content_type, &body).await;
                                return Ok(expand_spec_body(&base_url, kind, &body));
                            }
                        }

                        // --- archived JS ---
                        // Script is decided before HTML, not after. A `.js`
                        // capture the archive replayed without a
                        // `Content-Type` satisfies `is_html_like` (which
                        // treats an absent type as "might be markup"), so
                        // asking that question first would hand every such
                        // bundle to the HTML parser and mine nothing.
                        let script = self.extract_js_endpoints
                            && classify(response.headers(), &base_url) == BodyKind::Script;
                        let html = is_html_like(response.headers());
                        // A body nothing will read is still worth storing when
                        // the user asked for a corpus — JSON, CSS and plain
                        // text hold the comments and credentials that no
                        // extractor looks for — but an image never is.
                        // `script` is included on its own: `classify` reaches
                        // that verdict for a `.js` served as
                        // `application/octet-stream` (misconfigured static
                        // hosts are full of them), which `is_text_like` refuses
                        // on the strength of the declared type. Mining a body
                        // and then leaving it out of the corpus would lose
                        // exactly the file whose endpoints the run just
                        // reported.
                        let keep = self.body_archive.is_some()
                            && (script || is_text_like(response.headers(), &base_url));
                        if !script && !html && !keep {
                            return Ok(Vec::new());
                        }

                        let body = read_body_capped(response, MAX_BODY_BYTES).await?;
                        if keep {
                            self.persist(url, capture, &content_type, &body).await;
                        }

                        if script {
                            return Ok(JsEndpointExtractor::extract_endpoints(&base_url, &body));
                        }
                        if !html {
                            return Ok(Vec::new());
                        }
                        let mut links = LinkExtractor::extract_links(&base_url, &body);
                        if self.extract_js_endpoints {
                            // An archived page's inline scripts get the same
                            // treatment the live path gives them, and the two
                            // sources are merged in first-seen order.
                            let mut seen: HashSet<String> = links.iter().cloned().collect();
                            for endpoint in
                                JsEndpointExtractor::extract_inline_endpoints(&base_url, &body)
                            {
                                if seen.insert(endpoint.clone()) {
                                    links.push(endpoint);
                                }
                            }
                        }
                        return Ok(links);
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

    /// Deliberately not implemented beyond the no-op default: every request
    /// this tester makes goes to the Wayback Machine, never to the target, and
    /// the user's `-H` may well carry the target's session cookie.
    fn with_headers(&mut self, _headers: CustomHeaders) {}
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

    #[test]
    fn what_counts_as_a_text_body_worth_keeping() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};

        fn typed(ct: &str, url: &str) -> bool {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(ct).unwrap());
            is_text_like(&headers, &Url::parse(url).unwrap())
        }
        fn typeless(url: &str) -> bool {
            is_text_like(&HeaderMap::new(), &Url::parse(url).unwrap())
        }

        // A declared type decides, and decides against the extension: the
        // server knows what it served.
        assert!(typed("text/html; charset=utf-8", "https://e.com/a.png"));
        assert!(typed("application/json", "https://e.com/a"));
        assert!(typed("image/svg+xml", "https://e.com/a"));
        assert!(!typed("image/png", "https://e.com/a.html"));
        assert!(!typed("font/woff2", "https://e.com/a.css"));
        assert!(!typed("video/mp4", "https://e.com/a"));

        // No type at all: the extension decides instead.
        assert!(typeless("https://e.com/app.js"));
        assert!(typeless("https://e.com/config.json"));
        assert!(typeless("https://e.com/style.css"));
        assert!(typeless("https://e.com/notes.txt"));
        assert!(!typeless("https://e.com/logo.png"));
        assert!(!typeless("https://e.com/movie.mp4"));
        assert!(!typeless("https://e.com/bundle.zip"));

        // No type and no extension is a server-rendered page, which is what an
        // extensionless archived URL almost always is.
        assert!(typeless("https://e.com/checkout"));
        assert!(typeless("https://e.com/"));
    }

    // --- archived JS ---

    #[tokio::test]
    async fn an_archived_script_is_mined_when_extract_js_endpoints_is_on() {
        // The case the live path structurally cannot reach: a build-hashed
        // bundle the site stopped serving the day it redeployed.
        let mut server = mockito::Server::new_async().await;
        let replay = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/javascript")
            .with_body(r#"fetch("/api/v2/internal/users");var x="/admin/legacy/panel";"#)
            .expect(1)
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/static/app.a3f9c2.js",
                capture("20180101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_extract_js_endpoints(true);

        let found = ex
            .test_url("https://example.com/static/app.a3f9c2.js")
            .await
            .unwrap();
        assert!(
            found.contains(&"https://example.com/api/v2/internal/users".to_string()),
            "{found:?}"
        );
        assert!(
            found.contains(&"https://example.com/admin/legacy/panel".to_string()),
            "{found:?}"
        );
        replay.assert();
    }

    #[tokio::test]
    async fn an_archived_script_is_still_ignored_without_the_flag() {
        // --archive-body alone keeps its old behaviour: scripts are not markup,
        // so the link extractor has nothing to say about them.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/javascript")
            .with_body(r#"fetch("/api/v2/internal/users");"#)
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/static/app.a3f9c2.js",
                capture("20180101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        assert!(!ex.extracts_js_endpoints());

        let found = ex
            .test_url("https://example.com/static/app.a3f9c2.js")
            .await
            .unwrap();
        assert!(found.is_empty(), "{found:?}");
    }

    #[tokio::test]
    async fn a_typeless_archived_script_is_mined_rather_than_parsed_as_markup() {
        // `is_html_like` answers "yes" to a missing Content-Type, so deciding
        // HTML before script would hand every such bundle to the HTML parser
        // and mine nothing. The extension has to win here.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"axios.get("/api/orders/pending");"#)
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/bundle.js",
                capture("20180101000000", None),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_extract_js_endpoints(true);

        let found = ex.test_url("https://example.com/bundle.js").await.unwrap();
        assert_eq!(found, vec!["https://example.com/api/orders/pending"]);
    }

    #[tokio::test]
    async fn an_archived_pages_inline_scripts_are_mined_alongside_its_links() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(
                r#"<a href="/still-here">x</a>
                   <script>fetch("/api/hidden/thing")</script>"#,
            )
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/page.html",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_extract_js_endpoints(true);

        let found = ex.test_url("https://example.com/page.html").await.unwrap();
        // Markup links first, then what only the inline script knew.
        assert_eq!(
            found,
            vec![
                "https://example.com/still-here".to_string(),
                "https://example.com/api/hidden/thing".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn a_link_found_in_both_markup_and_inline_script_is_reported_once() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(
                r#"<a href="/api/shared">x</a>
                   <script>fetch("/api/shared")</script>"#,
            )
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/page.html",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_extract_js_endpoints(true);

        let found = ex.test_url("https://example.com/page.html").await.unwrap();
        assert_eq!(found, vec!["https://example.com/api/shared".to_string()]);
    }

    // --- --archive-body-dir ---

    #[tokio::test]
    async fn replayed_bodies_are_stored_when_a_directory_is_given() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<a href="/x">x</a><!-- staging.internal -->"#)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let archive = Arc::new(BodyArchive::create(dir.path().to_path_buf()).unwrap());

        let mut ex = extractor(
            &[(
                "https://example.com/page.html",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_body_archive(Some(Arc::clone(&archive)));

        // Links still come back: storing the body is a by-product, not a mode.
        let links = ex.test_url("https://example.com/page.html").await.unwrap();
        assert_eq!(links, vec!["https://example.com/x".to_string()]);

        assert_eq!(archive.written(), 1);
        let index = std::fs::read_to_string(dir.path().join(BodyArchive::INDEX_FILE)).unwrap();
        let entry: serde_json::Value = serde_json::from_str(index.trim()).unwrap();
        assert_eq!(entry["url"], "https://example.com/page.html");
        let stored =
            std::fs::read_to_string(dir.path().join(entry["file"].as_str().unwrap())).unwrap();
        // The comment is the point: no extractor would ever have reported it.
        assert!(stored.contains("staging.internal"), "{stored}");
    }

    #[tokio::test]
    async fn a_body_no_extractor_wants_is_still_stored_if_it_is_text() {
        // JSON is not markup and not script, so without --archive-body-dir the
        // fetch is skipped entirely. With it, the bytes are worth keeping: a
        // config left in the webroot is exactly what a corpus is grepped for.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"apiKey":"sk-live-000"}"#)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let archive = Arc::new(BodyArchive::create(dir.path().to_path_buf()).unwrap());

        let mut ex = extractor(
            &[(
                "https://example.com/config.json",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_body_archive(Some(Arc::clone(&archive)));

        let links = ex
            .test_url("https://example.com/config.json")
            .await
            .unwrap();
        assert!(links.is_empty(), "{links:?}");
        assert_eq!(archive.written(), 1);
    }

    #[tokio::test]
    async fn a_mined_script_is_stored_even_when_its_declared_type_is_loose() {
        // Misconfigured static hosts serve bundles as octet-stream. The
        // extension still makes it script, so it gets mined — and a corpus
        // missing the very file whose endpoints the run just reported would be
        // a confusing gap.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .with_body(r#"fetch("/api/v2/orders");"#)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let archive = Arc::new(BodyArchive::create(dir.path().to_path_buf()).unwrap());

        let mut ex = extractor(
            &[(
                "https://example.com/app.a3f9c2.js",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_extract_js_endpoints(true);
        ex.with_body_archive(Some(Arc::clone(&archive)));

        let found = ex
            .test_url("https://example.com/app.a3f9c2.js")
            .await
            .unwrap();
        assert_eq!(found, vec!["https://example.com/api/v2/orders".to_string()]);
        assert_eq!(archive.written(), 1);
    }

    #[tokio::test]
    async fn binary_bodies_are_never_stored() {
        // An image corpus would dominate the directory in both count and bytes
        // while containing nothing anyone greps for.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "image/png")
            .with_body(&b"\x89PNG\r\n"[..])
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let archive = Arc::new(BodyArchive::create(dir.path().to_path_buf()).unwrap());

        let mut ex = extractor(
            &[(
                "https://example.com/logo.png",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_body_archive(Some(Arc::clone(&archive)));

        let links = ex.test_url("https://example.com/logo.png").await.unwrap();
        assert!(links.is_empty());
        assert_eq!(archive.written(), 0);
    }

    #[tokio::test]
    async fn an_archived_specification_is_stored_too() {
        // The spec path returns early, so it needs its own persist call —
        // otherwise --expand-specs would silently punch a hole in the corpus.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"openapi":"3.0.0","paths":{"/pets":{"get":{}}}}"#)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let archive = Arc::new(BodyArchive::create(dir.path().to_path_buf()).unwrap());

        let mut ex = extractor(
            &[(
                "https://example.com/swagger.json",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_expand_specs(true);
        ex.with_body_archive(Some(Arc::clone(&archive)));

        let found = ex
            .test_url("https://example.com/swagger.json")
            .await
            .unwrap();
        assert!(!found.is_empty(), "spec should still expand");
        assert_eq!(archive.written(), 1);
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
    // --- spec-expansion ---

    #[tokio::test]
    async fn an_archived_specification_is_expanded_when_expand_specs_is_on() {
        let mut server = mockito::Server::new_async().await;
        let _replay = server
            .mock(
                "GET",
                "/web/20180101000000id_/https://gone.example.com/swagger.json",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"swagger":"2.0","basePath":"/v1","paths":{"/users":{},"/users/{id}":{}}}"#,
            )
            .expect(2)
            .create_async()
            .await;

        let entries = [(
            "https://gone.example.com/swagger.json",
            capture("20180101000000", Some("D1")),
        )];

        // Off by default: --archive-body alone hands the body to the HTML link
        // extractor, which `is_html_like` refuses for application/json — the
        // gap this combination closes.
        let mut ex = extractor(&entries, 10);
        ex.with_origin(server.url());
        assert!(ex
            .test_url("https://gone.example.com/swagger.json")
            .await
            .unwrap()
            .is_empty());

        // On: the API is gone, but the archive still holds what described it.
        let mut ex = extractor(&entries, 10);
        ex.with_origin(server.url());
        ex.with_expand_specs(true);
        let mut got = ex
            .test_url("https://gone.example.com/swagger.json")
            .await
            .unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                "https://gone.example.com/v1/users",
                "https://gone.example.com/v1/users/{id}",
            ]
        );
    }

    #[tokio::test]
    async fn expand_specs_leaves_archived_html_to_the_link_extractor() {
        let mut server = mockito::Server::new_async().await;
        let _replay = server
            .mock(
                "GET",
                "/web/20200101000000id_/https://example.com/page.html",
            )
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<a href="/still-here">x</a>"#)
            .create_async()
            .await;

        let mut ex = extractor(
            &[(
                "https://example.com/page.html",
                capture("20200101000000", Some("D1")),
            )],
            10,
        );
        ex.with_origin(server.url());
        ex.with_expand_specs(true);
        assert_eq!(
            ex.test_url("https://example.com/page.html").await.unwrap(),
            vec!["https://example.com/still-here".to_string()]
        );
    }
}
