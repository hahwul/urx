//! The CDX page-walking engine shared by every CDX-backed provider, and the
//! generic [`CdxProvider`] that points it at a user-supplied index server.
//!
//! Every web archive that exposes a CDX API paginates in one of two ways, and
//! the two are not interchangeable — they do not even agree on what a page is:
//!
//! | | walk | row format | dialect |
//! |---|---|---|---|
//! | [`walk_resume_key`] | `limit=N&showResumeKey=true`, follow the cursor | space-separated text | [`CdxDialect::Classic`] (web.archive.org) |
//! | [`walk_block_pages`] | `showNumPages=true`, then `page=0..N` | one JSON object per line | [`CdxDialect::Pywb`] (Common Crawl, pywb, vefsafn.is) |
//!
//! Wayback and Common Crawl each own one of these walks; a pluggable
//! `--cdx-endpoint` needs both, chosen by dialect. So the walks live here, once,
//! and the named providers call them with their own query base — the only thing
//! that differs between "Wayback" and "some other classic CDX server" is the
//! origin in front of `?url=`.
//!
//! # Bot challenges
//!
//! A public archive behind an anti-bot proxy (Anubis and friends) answers a CDX
//! query with an HTML "Session Verification" page rather than an error status.
//! Parsed as CDX that page is simply zero rows, which reads as "this domain has
//! no captures" — the one outcome the tool must never fake. Every body the
//! engine reads goes through [`reject_html`] first, so a challenge page is an
//! error that names the endpoint, not an empty success.

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::OnceCell;

use super::filters::{ArchiveFilters, CdxDialect};
use super::wayback::split_page;
use super::{CaptureMeta, Provider, RecordSet, UrlRecord};
use crate::network::client::{get_with_retry, HttpClientConfig};
use crate::network::RateLimiter;
use crate::progress::ProgressReporter;

/// How many rows to ask a resume-key server for per request. A bounded `limit`
/// is what keeps large domains from failing: an unbounded query makes the
/// server compute and buffer the *entire* result set (it then routinely times
/// out), whereas a capped request streams a slice and returns promptly. Most
/// domains fit in a single page; only the large ones the user cares about
/// paginate.
pub(crate) const RESUME_PAGE_LIMIT: usize = 50_000;

/// Hard ceiling on the number of pages either walk will follow, so a
/// misbehaving cursor or an absurd page count can never spin forever. At
/// `RESUME_PAGE_LIMIT` rows each this covers domains with up to ~500M captured
/// URLs — far beyond anything real.
pub(crate) const MAX_PAGES: usize = 10_000;

/// How many block pages in a row may fail before the walk gives up. A single
/// failed page is skipped (they are independently addressable), but a run of
/// them means the index is unhealthy and continuing would only add requests
/// and back-off delay to an already-doomed fetch.
pub(crate) const MAX_CONSECUTIVE_PAGE_FAILURES: usize = 3;

/// The classic-dialect `fl=` list: URL first (see [`split_page`]), then the
/// capture metadata the archive already holds.
pub(crate) const CLASSIC_FIELDS: &str = "original,timestamp,mimetype,statuscode,digest";

/// The pywb-dialect `fl=` list — same columns, pywb's names.
pub(crate) const PYWB_FIELDS: &str = "url,timestamp,mime,status,digest";

impl FromStr for CdxDialect {
    type Err = String;

    /// The `--cdx-dialect` spellings.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "classic" => Ok(CdxDialect::Classic),
            "pywb" => Ok(CdxDialect::Pywb),
            other => Err(format!(
                "Unknown CDX dialect {other:?}. Allowed values: classic, pywb"
            )),
        }
    }
}

/// One CDXJ / NDJSON row of a pywb-dialect index. Every value arrives as a JSON
/// string, the status code included, and the metadata is named `status`/`mime`
/// rather than the classic `statuscode`/`mimetype`.
#[derive(Debug, Deserialize)]
pub(crate) struct PywbRow {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub digest: Option<String>,
}

impl PywbRow {
    pub(crate) fn into_record(self) -> UrlRecord {
        let meta = CaptureMeta::capture(
            self.timestamp.as_deref(),
            self.mime.as_deref(),
            self.status.as_deref(),
            self.digest.as_deref(),
        );
        UrlRecord::new(self.url, meta)
    }
}

/// Parse a pywb NDJSON body into capture records. Each non-empty line is an
/// independent JSON object, so a single malformed line (e.g. a stray error
/// message) is skipped rather than aborting the whole page. Rows without an
/// `http(s)` URL are dropped.
pub(crate) fn parse_pywb_rows(text: &str) -> Vec<UrlRecord> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let row: PywbRow = serde_json::from_str(line).ok()?;
            if !row.url.starts_with("http://") && !row.url.starts_with("https://") {
                return None;
            }
            Some(row.into_record())
        })
        .collect()
}

/// Whether a body is an HTML document rather than CDX rows. No CDX row — text
/// or JSON — can begin with `<`, so the first non-blank character is decisive.
pub(crate) fn looks_like_html(body: &str) -> bool {
    body.trim_start().starts_with('<')
}

/// Refuse an HTML body. See the module docs: an anti-bot challenge page parsed
/// as CDX is zero rows, which would read as "no captures".
pub(crate) fn reject_html(body: &str, endpoint: &str) -> Result<()> {
    if looks_like_html(body) {
        return Err(anyhow::anyhow!(
            "{endpoint} answered with an HTML page instead of CDX rows — most likely a bot-protection \
             challenge (e.g. Anubis \"Session Verification\"). Slow down with --rate-limit, or retry later."
        ));
    }
    Ok(())
}

/// Response shape of a `&showNumPages=true` probe — a block-paginating index
/// server reports how many pages a query spans. Extra fields (pageSize,
/// blocks) are ignored.
#[derive(Deserialize)]
struct PageInfo {
    pages: usize,
}

/// Everything one CDX walk needs to make its requests: the client, the retry
/// budget, the rate limiter, the progress reporter, and the endpoint name for
/// error messages.
pub(crate) struct CdxSession<'a> {
    pub client: &'a Client,
    pub retries: u32,
    pub limiter: Option<&'a RateLimiter>,
    pub reporter: Option<&'a ProgressReporter>,
    /// Origin named in error messages, e.g. `https://web.archive.org`.
    pub endpoint: &'a str,
}

impl CdxSession<'_> {
    /// Fetch one page, racing the request against the stop signal.
    ///
    /// Checking the signal only between pages is not enough: one CDX slice can
    /// take tens of seconds, so a deadline landing mid-page went unnoticed
    /// until long after the runner's grace window had closed and the hard
    /// cancel had thrown the buffer away. `None` means the run asked us to
    /// stop while the request was in flight.
    pub(crate) async fn get(&self, url: &str) -> Option<Result<String>> {
        if let Some(rl) = self.limiter {
            rl.acquire().await;
        }
        let fetched = match self.reporter {
            Some(r) => tokio::select! {
                biased;
                _ = r.stopped() => None,
                res = get_with_retry(self.client, url, self.retries) => Some(res),
            },
            None => Some(get_with_retry(self.client, url, self.retries).await),
        };
        fetched.map(|res| res.and_then(|body| reject_html(&body, self.endpoint).map(|()| body)))
    }

    fn detail(&self, count: usize) {
        if let Some(r) = self.reporter {
            r.detail(format!("{count} URLs…"));
        }
    }

    fn mark_partial(&self) {
        if let Some(r) = self.reporter {
            r.mark_partial();
        }
    }

    fn stop_requested(&self) -> bool {
        self.reporter.is_some_and(|r| r.stop_requested())
    }
}

/// Percent-encode a resume key so opaque cursor bytes (`+`, `/`, `=` in some
/// base64 variants) survive being spliced back into the query string.
pub(crate) fn encode_resume_key(key: &str) -> String {
    url::form_urlencoded::byte_serialize(key.as_bytes()).collect()
}

/// Walk a classic CDX server's resume-key cursor.
///
/// Each request returns at most [`RESUME_PAGE_LIMIT`] rows plus a resume key
/// pointing at the next slice. Following the key lets arbitrarily large domains
/// complete as a series of bounded, fast requests instead of one unbounded
/// request that times out. Rows are folded per URL as they arrive:
/// `collapse=urlkey` only collapses *adjacent* rows, so the same URL still
/// shows up on several pages, each time with a different capture.
///
/// `query_base` is the full query minus the pagination parameters.
pub(crate) async fn walk_resume_key(
    session: &CdxSession<'_>,
    query_base: &str,
) -> Result<Vec<UrlRecord>> {
    let mut records = RecordSet::new();
    let mut resume_key: Option<String> = None;
    // Every cursor position we have already requested. A key we have seen
    // before means the cursor is stuck or cycling, which is the only way this
    // walk can fail to terminate — comparing against just the previous key
    // misses cycles of length two or more.
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut pages = 0usize;
    // Set when we stop while the server was still advertising more results,
    // so a truncated crawl is never reported as a clean one.
    let mut truncated = false;

    loop {
        pages += 1;
        if pages > MAX_PAGES {
            // We only get here holding a resume key, i.e. with results left
            // on the server.
            truncated = true;
            break;
        }

        let mut url = format!("{query_base}&limit={RESUME_PAGE_LIMIT}&showResumeKey=true");
        if let Some(key) = &resume_key {
            url.push_str("&resumeKey=");
            url.push_str(&encode_resume_key(key));
        }

        let Some(result) = session.get(&url).await else {
            // Stopped mid-request: keep the pages already walked.
            truncated = true;
            break;
        };
        let text = match result {
            Ok(text) => text,
            Err(e) => {
                // Best effort: a mid-cursor failure shouldn't discard the pages
                // we already pulled. Only a failure on the very first request
                // (nothing collected) is fatal.
                if records.is_empty() {
                    return Err(e);
                }
                // We're returning a truncated result. Flag it so the caller can
                // mark the line partial and warn rather than present an
                // incomplete crawl as a clean success.
                session.mark_partial();
                break;
            }
        };

        let (page_records, next_key) = split_page(&text);
        records.extend(page_records);
        session.detail(records.len());

        // The run asked us to stop (--max-time elapsed, or Ctrl-C). Hand back
        // the pages already walked instead of losing them to the hard cancel
        // after the runner's grace window.
        if session.stop_requested() {
            truncated = true;
            break;
        }

        // No resume key ⇒ the server said this was the last slice, so the walk
        // is complete. A key means more results remain: follow it whenever it
        // is one we have not used yet. A *new* key is progress even when the
        // page carried no rows — a server-side `filter=` can empty an entire
        // slice — while a key we have already requested means the cursor is
        // not advancing, and continuing would just re-fetch the same slices
        // forever.
        match next_key {
            None => break,
            Some(key) => {
                if !seen_keys.insert(key.clone()) {
                    truncated = true;
                    break;
                }
                resume_key = Some(key);
            }
        }
    }

    if truncated {
        session.mark_partial();
    }

    Ok(records.into_sorted())
}

/// Walk a block-paginating (pywb) index server.
///
/// A single request returns only the first block (historically ~15k records on
/// Common Crawl). We must ask how many pages the query spans via
/// `&showNumPages=true` and then walk every page, or large domains are silently
/// truncated to their first block.
///
/// Not every pywb-flavoured server honours the pagination parameters: vefsafn.is
/// and Arquivo.pt were both measured to ignore `showNumPages`, `page` and
/// `limit` alike and answer every request with a result set. The probe then
/// comes back holding rows instead of a page count, and the walk falls back to
/// a single `page=0` request. That body is *not* kept as page 0, tempting as
/// it is: vefsafn.is was measured to drop every other parameter — `from=`,
/// `filter=`, `collapse=` — whenever `showNumPages=true` is present, so the
/// rows it sends alongside are the unfiltered result set, not the answer to
/// the query.
///
/// `query_base` is the full query minus the pagination parameters.
pub(crate) async fn walk_block_pages(
    session: &CdxSession<'_>,
    query_base: &str,
) -> Result<Vec<UrlRecord>> {
    let count_url = format!("{query_base}&showNumPages=true");
    let pages = match session.get(&count_url).await {
        Some(Ok(body)) => serde_json::from_str::<PageInfo>(body.trim())
            .map(|info| info.pages)
            // A 200 that isn't a page-count document: the server ignored the
            // probe. Fall back to a single page rather than giving up — and
            // see above for why that page is fetched afresh.
            .unwrap_or(1),
        // The index returns 404 for a domain with no captures. Don't hard-fail
        // the probe; fall through to a single page=0 fetch so genuine "no data"
        // stays an empty/`Err` result exactly as a single request produces.
        Some(Err(_)) => 1,
        // Stopped before the first row arrived: nothing to keep.
        None => {
            session.mark_partial();
            return Ok(Vec::new());
        }
    };

    if pages == 0 {
        return Ok(Vec::new());
    }
    let pages = pages.min(MAX_PAGES);

    // The index is capture-level: a URL crawled repeatedly appears once per
    // capture, so rows are folded per URL as they arrive.
    let mut records = RecordSet::new();
    let mut consecutive_failures = 0usize;
    for page in 0..pages {
        let page_url = format!("{query_base}&page={page}");
        let Some(result) = session.get(&page_url).await else {
            // Stopped mid-request: keep the pages already walked.
            session.mark_partial();
            break;
        };
        match result {
            Ok(text) => {
                consecutive_failures = 0;
                records.extend(parse_pywb_rows(&text));
                session.detail(records.len());
                // The run asked us to stop (--max-time elapsed, or Ctrl-C).
                // Hand back the pages already walked instead of losing them to
                // the hard cancel after the runner's grace window.
                if session.stop_requested() {
                    if page + 1 < pages {
                        session.mark_partial();
                    }
                    break;
                }
            }
            Err(e) => {
                // A failure on the very first page (e.g. the 404 the index
                // returns for a domain it has no captures for) is a hard
                // failure, matching single-request behaviour.
                if page == 0 {
                    return Err(e);
                }
                // Later pages are *independently addressable* — `page=N` is a
                // direct block address, not a cursor — so one bad page says
                // nothing about the rest. Skipping it costs a slice; abandoning
                // the walk here used to throw away every remaining page (on a
                // 266-page domain, a single hiccup on page 5 discarded 98% of
                // the result).
                session.mark_partial();
                consecutive_failures += 1;
                if consecutive_failures >= MAX_CONSECUTIVE_PAGE_FAILURES {
                    // The index itself is unhealthy rather than one page being
                    // unlucky; stop hammering it.
                    break;
                }
            }
        }
    }

    Ok(records.into_sorted())
}

/// Normalise a `--cdx-endpoint` value into the query base it will be used as:
/// trimmed, and without a dangling `?`/`&`. Rejects anything that is not an
/// absolute `http(s)` URL with a host, so a typo fails at startup instead of
/// producing a request to nowhere on every domain.
pub fn normalize_endpoint(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches(['?', '&']);
    let parsed = url::Url::parse(trimmed)
        .map_err(|e| anyhow::anyhow!("Invalid --cdx-endpoint {raw:?}: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(anyhow::anyhow!(
            "Invalid --cdx-endpoint {raw:?}: expected an absolute http(s) URL such as https://vefsafn.is/cdx"
        ));
    }
    if parsed.query().is_some() {
        return Err(anyhow::anyhow!(
            "Invalid --cdx-endpoint {raw:?}: give the CDX API path only, without a query string"
        ));
    }
    Ok(trimmed.to_string())
}

/// A CDX index server the user pointed urx at with `--cdx-endpoint`.
///
/// This is the same machinery Wayback and Common Crawl run on, with two things
/// left open: the origin, and which of the two [`CdxDialect`]s the server
/// speaks. The dialect decides the field names, the filter semantics, the row
/// format and the pagination scheme all at once — a wrong guess produces no
/// error, just empty or truncated results — so when the user does not name it
/// the provider probes once (see [`CdxProvider::effective_dialect`]) before the
/// first real query.
#[derive(Clone)]
pub struct CdxProvider {
    /// Full CDX API URL, e.g. `https://vefsafn.is/cdx`. Query parameters are
    /// appended directly after a `?`.
    endpoint: String,
    /// The dialect the user named, or `None` to detect it.
    dialect: Option<CdxDialect>,
    /// Memoised detection result, shared across clones so the probe happens at
    /// most once per endpoint per run — not once per domain.
    detected: Arc<OnceCell<CdxDialect>>,
    include_subdomains: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    rate_limit: Option<RateLimiter>,
    /// Server-side CDX predicates (date range, status code, MIME type).
    filters: ArchiveFilters,
}

impl CdxProvider {
    /// Point the engine at `endpoint`. Pass `None` as the dialect to detect it
    /// from the server's first answer.
    pub fn new(endpoint: String, dialect: Option<CdxDialect>) -> Self {
        CdxProvider {
            endpoint,
            dialect,
            detected: Arc::new(OnceCell::new()),
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 60,
            retries: 3,
            random_agent: false,
            insecure: false,
            rate_limit: None,
            filters: ArchiveFilters::default(),
        }
    }

    /// Apply server-side CDX predicates (date range, status code, MIME type).
    /// Rendered in whichever dialect the server turns out to speak.
    pub fn with_filters(&mut self, filters: ArchiveFilters) -> &mut Self {
        self.filters = filters;
        self
    }

    /// The endpoint this provider queries.
    #[cfg(test)]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Build an `HttpClientConfig` from the current provider settings.
    fn client_config(&self) -> HttpClientConfig {
        HttpClientConfig {
            timeout: self.timeout,
            insecure: self.insecure,
            random_agent: self.random_agent,
            proxy: self.proxy.clone(),
            proxy_auth: self.proxy_auth.clone(),
        }
    }

    /// The `url=` value: a leading `*.` matches subdomains, a trailing `/*`
    /// matches the host and all of its paths — the wildcard forms every CDX
    /// server urx queries honours.
    fn url_pattern(&self, domain: &str) -> String {
        if self.include_subdomains {
            format!("*.{domain}/*")
        } else {
            format!("{domain}/*")
        }
    }

    /// The query without pagination parameters, in `dialect`.
    ///
    /// `fl=` is restricted to the five columns the parsers read: a full-record
    /// page of a large domain is several times the size of the same rows
    /// narrowed to a field list, and the whole body is buffered before parsing.
    /// `collapse=urlkey` asks a capture-level index to fold adjacent captures of
    /// one URL; servers that ignore it (vefsafn.is does) simply send every
    /// capture and the [`RecordSet`] folds them instead.
    fn query_base(&self, dialect: CdxDialect, domain: &str) -> String {
        let pattern = self.url_pattern(domain);
        let mut url = match dialect {
            CdxDialect::Classic => format!(
                "{}?url={pattern}&fl={CLASSIC_FIELDS}&collapse=urlkey",
                self.endpoint
            ),
            CdxDialect::Pywb => format!(
                "{}?url={pattern}&output=json&fl={PYWB_FIELDS}&collapse=urlkey",
                self.endpoint
            ),
        };
        url.push_str(&self.filters.query_params(dialect));
        url
    }

    /// Classify a server's answer to `output=json&limit=1`.
    ///
    /// pywb streams one JSON object per line, so the body opens with `{`. The
    /// classic Internet Archive server (and OutbackCDX, which mimics it)
    /// answers `output=json` with a JSON array of arrays, so the body opens
    /// with `[`. Anything else — an empty body for a domain with no captures,
    /// or plain-text rows from a server that ignores `output=` — carries no
    /// signal, and the pywb default stands.
    fn classify_probe(body: &str) -> Option<CdxDialect> {
        match body.trim_start().chars().next() {
            Some('{') => Some(CdxDialect::Pywb),
            Some('[') => Some(CdxDialect::Classic),
            _ => None,
        }
    }

    /// The dialect to query in: what the user named, else what one probe of
    /// the server says, else pywb.
    ///
    /// The probe is memoised across clones, so it runs once per endpoint per
    /// run. A probe that *fails* (network error, 404 for a domain the archive
    /// has never seen) is not cached — the next domain probes again — but it
    /// does not fail the fetch either: the pywb default is used for this
    /// domain. An HTML answer is the one exception, because it is the bot
    /// challenge the module docs describe and must surface as an error.
    async fn effective_dialect(&self, client: &Client, domain: &str) -> Result<CdxDialect> {
        if let Some(dialect) = self.dialect {
            return Ok(dialect);
        }
        if let Some(dialect) = self.detected.get() {
            return Ok(*dialect);
        }

        let probe_url = format!("{}?url={domain}&output=json&limit=1", self.endpoint);
        if let Some(rl) = &self.rate_limit {
            rl.acquire().await;
        }
        let body = match get_with_retry(client, &probe_url, self.retries).await {
            Ok(body) => body,
            Err(_) => return Ok(CdxDialect::Pywb),
        };
        reject_html(&body, &self.endpoint)?;

        let dialect = Self::classify_probe(&body).unwrap_or(CdxDialect::Pywb);
        // `set` fails only when another clone won the race; both saw the same
        // server, so either answer is fine.
        let _ = self.detected.set(dialect);
        Ok(dialect)
    }
}

impl Provider for CdxProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlRecord>>> + Send + 'a>> {
        self.fetch_urls_with_progress(domain, None)
    }

    fn fetch_urls_with_progress<'a>(
        &'a self,
        domain: &'a str,
        reporter: Option<ProgressReporter>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlRecord>>> + Send + 'a>> {
        Box::pin(async move {
            let client = self.client_config().build_client()?;

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            let dialect = self.effective_dialect(&client, domain).await?;
            let query_base = self.query_base(dialect, domain);
            let session = CdxSession {
                client: &client,
                retries: self.retries,
                limiter: self.rate_limit.as_ref(),
                reporter: reporter.as_ref(),
                endpoint: &self.endpoint,
            };

            match dialect {
                CdxDialect::Classic => walk_resume_key(&session, &query_base).await,
                CdxDialect::Pywb => walk_block_pages(&session, &query_base).await,
            }
        })
    }

    fn with_subdomains(&mut self, include: bool) {
        self.include_subdomains = include;
    }

    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
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

    fn with_rate_limit(&mut self, rate_limit: Option<f32>) {
        self.rate_limit = RateLimiter::from_rate(rate_limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::urls_of;
    use mockito::Matcher;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn provider(server: &mockito::ServerGuard, dialect: Option<CdxDialect>) -> CdxProvider {
        let mut p = CdxProvider::new(format!("{}/cdx", server.url()), dialect);
        p.with_retries(0);
        p
    }

    #[test]
    fn dialect_parses_the_documented_spellings() {
        assert_eq!("classic".parse::<CdxDialect>(), Ok(CdxDialect::Classic));
        assert_eq!(" PyWB ".parse::<CdxDialect>(), Ok(CdxDialect::Pywb));
        let err = "wayback".parse::<CdxDialect>().unwrap_err();
        assert!(err.contains("classic, pywb"), "{err}");
    }

    #[test]
    fn endpoint_normalisation_accepts_a_bare_api_url_only() {
        assert_eq!(
            normalize_endpoint(" https://vefsafn.is/cdx? ").unwrap(),
            "https://vefsafn.is/cdx"
        );
        assert_eq!(
            normalize_endpoint("http://localhost:8080/cdx&").unwrap(),
            "http://localhost:8080/cdx"
        );
        for bad in ["vefsafn.is/cdx", "ftp://vefsafn.is/cdx", "https://", ""] {
            assert!(normalize_endpoint(bad).is_err(), "{bad:?} must be rejected");
        }
        // A query string would be spliced in front of our own `?url=`.
        let err = normalize_endpoint("https://vefsafn.is/cdx?output=json").unwrap_err();
        assert!(err.to_string().contains("without a query string"), "{err}");
    }

    #[test]
    fn query_base_speaks_each_dialect() {
        let mut p = CdxProvider::new("https://vefsafn.is/cdx".to_string(), None);
        p.with_filters(ArchiveFilters::from_cli_lists(
            Some("20200101000000".to_string()),
            None,
            &s(&["200"]),
            &[],
            &[],
            &s(&["text/html"]),
        ));

        let pywb = p.query_base(CdxDialect::Pywb, "example.com");
        assert!(
            pywb.starts_with(
                "https://vefsafn.is/cdx?url=example.com/*&output=json&fl=url,timestamp,mime,status,digest&collapse=urlkey"
            ),
            "{pywb}"
        );
        assert!(pywb.contains("&from=20200101000000"), "{pywb}");
        assert!(pywb.contains("&filter=status:200"), "{pywb}");
        assert!(pywb.contains("&filter=!mime:text%2Fhtml"), "{pywb}");

        let classic = p.query_base(CdxDialect::Classic, "example.com");
        assert!(
            classic.starts_with(
                "https://vefsafn.is/cdx?url=example.com/*&fl=original,timestamp,mimetype,statuscode,digest&collapse=urlkey"
            ),
            "{classic}"
        );
        assert!(!classic.contains("output=json"), "{classic}");
        assert!(classic.contains("&filter=statuscode:200"), "{classic}");
        assert!(
            classic.contains("&filter=!mimetype:text%2Fhtml"),
            "{classic}"
        );

        p.with_subdomains(true);
        assert!(
            p.query_base(CdxDialect::Pywb, "example.com")
                .contains("?url=*.example.com/*&"),
            "--subs must widen the pattern"
        );
    }

    #[test]
    fn html_is_never_mistaken_for_rows() {
        assert!(looks_like_html("<!DOCTYPE html><html>…"));
        assert!(looks_like_html("\n  <html lang=\"en\">"));
        assert!(!looks_like_html("{\"url\": \"https://example.com/\"}"));
        assert!(!looks_like_html(
            "https://example.com/ 20240101000000 text/html 200 ABC"
        ));
        assert!(!looks_like_html(""));
        let err = reject_html(
            "<html>Session Verification</html>",
            "https://vefsafn.is/cdx",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("https://vefsafn.is/cdx"), "{err}");
        assert!(err.contains("bot-protection"), "{err}");
    }

    #[test]
    fn pywb_rows_parse_with_metadata_and_skip_junk() {
        let body = "{\"urlkey\":\"com,example)/\",\"url\":\"http://example.com/a\",\"timestamp\":\"20240101000000\",\"mime\":\"text/html\",\"status\":\"200\",\"digest\":\"ABC\"}\n\
                    \n\
                    not-json\n\
                    {\"url\":\"https://example.com/b\"}\n\
                    {\"timestamp\":\"20200101\"}\n\
                    {\"url\":\"ftp://example.com/skip\"}\n";
        let records = parse_pywb_rows(body);
        assert_eq!(
            records.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            vec!["http://example.com/a", "https://example.com/b"]
        );
        assert_eq!(records[0].meta.mime(), Some("text/html"));
        assert_eq!(records[0].meta.archive_status(), Some("200"));
        assert_eq!(records[0].meta.digest(), Some("ABC"));
        assert!(records[1].meta.is_empty());
    }

    #[tokio::test]
    async fn pywb_dialect_walks_show_num_pages_then_every_page() {
        let mut server = mockito::Server::new_async().await;
        let count = server
            .mock("GET", "/cdx")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                Matcher::UrlEncoded("output".into(), "json".into()),
                Matcher::UrlEncoded("fl".into(), PYWB_FIELDS.into()),
                Matcher::UrlEncoded("filter".into(), "status:200".into()),
                Matcher::UrlEncoded("showNumPages".into(), "true".into()),
            ]))
            .with_body("{\"pages\": 2, \"pageSize\": 5, \"blocks\": 10}")
            .expect(1)
            .create_async()
            .await;
        let page0 = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("page".into(), "0".into()))
            .with_body("{\"url\":\"https://example.com/a\",\"timestamp\":\"20200101000000\",\"status\":\"200\"}\n")
            .expect(1)
            .create_async()
            .await;
        let page1 = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("page".into(), "1".into()))
            .with_body("{\"url\":\"https://example.com/a\",\"timestamp\":\"20240101000000\",\"status\":\"200\"}\n{\"url\":\"https://example.com/b\"}\n")
            .expect(1)
            .create_async()
            .await;

        let mut p = provider(&server, Some(CdxDialect::Pywb));
        p.with_filters(ArchiveFilters::from_cli_lists(
            None,
            None,
            &s(&["200"]),
            &[],
            &[],
            &[],
        ));
        let records = p.fetch_urls("example.com").await.unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].url, "https://example.com/a");
        // Captures of one URL across pages fold into one record.
        assert_eq!(records[0].meta.first_seen(), Some("20200101000000"));
        assert_eq!(records[0].meta.last_seen(), Some("20240101000000"));
        assert_eq!(records[1].url, "https://example.com/b");
        count.assert();
        page0.assert();
        page1.assert();
    }

    #[tokio::test]
    async fn a_server_that_ignores_pagination_is_walked_as_one_page() {
        // vefsafn.is answers `showNumPages=true` with rows — and, measured
        // live, with the *unfiltered* rows: every other parameter is dropped
        // when the probe flag is present. So the probe body is discarded and
        // page 0 is fetched with the real query, exactly once.
        let mut server = mockito::Server::new_async().await;
        let probe = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("showNumPages".into(), "true".into()))
            .with_body("{\"url\":\"https://example.com/unfiltered\"}\n{\"url\":\"https://example.com/a\"}\n")
            .expect(1)
            .create_async()
            .await;
        let page = server
            .mock("GET", "/cdx")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "0".into()),
                Matcher::UrlEncoded("filter".into(), "status:200".into()),
            ]))
            .with_body("{\"url\":\"https://example.com/a\",\"status\":\"200\"}\n")
            .expect(1)
            .create_async()
            .await;

        let mut p = provider(&server, Some(CdxDialect::Pywb));
        p.with_filters(ArchiveFilters::from_cli_lists(
            None,
            None,
            &s(&["200"]),
            &[],
            &[],
            &[],
        ));
        let urls = urls_of(p.fetch_urls("example.com").await.unwrap());

        assert_eq!(urls, vec!["https://example.com/a"]);
        probe.assert();
        page.assert();
    }

    #[tokio::test]
    async fn classic_dialect_follows_the_resume_key() {
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("GET", "/cdx")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("url".into(), "*.example.com/*".into()),
                Matcher::UrlEncoded("fl".into(), CLASSIC_FIELDS.into()),
                Matcher::UrlEncoded("showResumeKey".into(), "true".into()),
                Matcher::UrlEncoded("filter".into(), "!statuscode:404".into()),
            ]))
            .with_body("https://example.com/a 20240101000000 text/html 200 AAA\n\nKEY1\n")
            .expect(1)
            .create_async()
            .await;
        let second = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("resumeKey".into(), "KEY1".into()))
            .with_body("https://sub.example.com/b 20240102000000 text/html 200 BBB\n")
            .expect(1)
            .create_async()
            .await;

        let mut p = provider(&server, Some(CdxDialect::Classic));
        p.with_subdomains(true);
        p.with_filters(ArchiveFilters::from_cli_lists(
            None,
            None,
            &[],
            &s(&["404"]),
            &[],
            &[],
        ));
        let records = p.fetch_urls("example.com").await.unwrap();

        assert_eq!(
            records.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            vec!["https://example.com/a", "https://sub.example.com/b"]
        );
        assert_eq!(records[1].meta.digest(), Some("BBB"));
        first.assert();
        second.assert();
    }

    #[tokio::test]
    async fn an_html_challenge_page_is_an_error_not_zero_urls() {
        let mut server = mockito::Server::new_async().await;
        let _challenge = server
            .mock("GET", "/cdx")
            .match_query(Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(
                "<!DOCTYPE html><html><head><title>Session Verification</title></head></html>",
            )
            .create_async()
            .await;

        let p = provider(&server, Some(CdxDialect::Pywb));
        let err = p.fetch_urls("example.com").await.unwrap_err().to_string();
        assert!(err.contains("HTML page instead of CDX rows"), "{err}");
        assert!(err.contains("/cdx"), "the endpoint must be named: {err}");

        // The classic walk refuses it just the same.
        let p = provider(&server, Some(CdxDialect::Classic));
        let err = p.fetch_urls("example.com").await.unwrap_err().to_string();
        assert!(err.contains("HTML page instead of CDX rows"), "{err}");
    }

    #[tokio::test]
    async fn a_challenge_page_mid_walk_keeps_the_pages_already_read() {
        let mut server = mockito::Server::new_async().await;
        let _count = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("showNumPages".into(), "true".into()))
            .with_body("{\"pages\": 2}")
            .create_async()
            .await;
        let _page0 = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("page".into(), "0".into()))
            .with_body("{\"url\":\"https://example.com/a\"}\n")
            .create_async()
            .await;
        let _page1 = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("page".into(), "1".into()))
            .with_body("<html>Session Verification</html>")
            .create_async()
            .await;

        let p = provider(&server, Some(CdxDialect::Pywb));
        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");
        let urls = urls_of(
            p.fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );
        assert_eq!(urls, vec!["https://example.com/a"]);
        assert!(
            reporter.is_partial(),
            "a page lost to a challenge is a partial result"
        );
    }

    #[tokio::test]
    async fn dialect_is_detected_from_one_probe_and_remembered() {
        let mut server = mockito::Server::new_async().await;
        // The probe: `url=<domain>&output=json&limit=1`, no wildcard.
        let probe = server
            .mock("GET", "/cdx")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("url".into(), "example.com".into()),
                Matcher::UrlEncoded("limit".into(), "1".into()),
            ]))
            .with_body("[[\"urlkey\",\"timestamp\"],[\"com,example)/\",\"20240101000000\"]]")
            .expect(1)
            .create_async()
            .await;
        // A classic answer ⇒ the real query is a resume-key walk.
        let walk = server
            .mock("GET", "/cdx")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("fl".into(), CLASSIC_FIELDS.into()),
                Matcher::UrlEncoded("showResumeKey".into(), "true".into()),
            ]))
            .with_body("https://example.com/a 20240101000000 text/html 200 AAA\n")
            .expect(2)
            .create_async()
            .await;

        let p = provider(&server, None);
        let urls = urls_of(p.fetch_urls("example.com").await.unwrap());
        assert_eq!(urls, vec!["https://example.com/a"]);

        // A clone (what the runner fetches through) reuses the answer.
        let clone = p.clone_box();
        let urls = urls_of(clone.fetch_urls("example.com").await.unwrap());
        assert_eq!(urls, vec!["https://example.com/a"]);

        probe.assert();
        walk.assert();
    }

    #[tokio::test]
    async fn detection_falls_back_to_pywb_when_the_probe_says_nothing() {
        let mut server = mockito::Server::new_async().await;
        // Empty probe body: a domain the archive has never seen.
        let _probe = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("limit".into(), "1".into()))
            .with_body("")
            .create_async()
            .await;
        let _count = server
            .mock("GET", "/cdx")
            .match_query(Matcher::UrlEncoded("showNumPages".into(), "true".into()))
            .with_body("{\"pages\": 1}")
            .create_async()
            .await;
        let walk = server
            .mock("GET", "/cdx")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("output".into(), "json".into()),
                Matcher::UrlEncoded("page".into(), "0".into()),
            ]))
            .with_body("{\"url\":\"https://example.com/x\"}\n")
            .expect(1)
            .create_async()
            .await;

        let p = provider(&server, None);
        let urls = urls_of(p.fetch_urls("example.com").await.unwrap());
        assert_eq!(urls, vec!["https://example.com/x"]);
        walk.assert();
    }

    #[tokio::test]
    async fn a_challenge_on_the_probe_is_reported_too() {
        let mut server = mockito::Server::new_async().await;
        let _probe = server
            .mock("GET", "/cdx")
            .match_query(Matcher::Any)
            .with_body("<html>Session Verification</html>")
            .create_async()
            .await;

        let p = provider(&server, None);
        let err = p.fetch_urls("example.com").await.unwrap_err().to_string();
        assert!(err.contains("HTML page instead of CDX rows"), "{err}");
    }

    #[test]
    fn probe_classification() {
        assert_eq!(
            CdxProvider::classify_probe("{\"url\": \"x\"}"),
            Some(CdxDialect::Pywb)
        );
        assert_eq!(
            CdxProvider::classify_probe("\n[[\"urlkey\"]]"),
            Some(CdxDialect::Classic)
        );
        assert_eq!(CdxProvider::classify_probe(""), None);
        assert_eq!(
            CdxProvider::classify_probe("com,example)/ 20240101000000 https://example.com/"),
            None
        );
    }

    #[test]
    fn network_settings_apply() {
        let mut p = CdxProvider::new("https://vefsafn.is/cdx".to_string(), None);
        p.with_timeout(45);
        p.with_insecure(true);
        p.with_random_agent(true);
        p.with_proxy(Some("http://proxy:8080".to_string()));
        p.with_proxy_auth(Some("user:pass".to_string()));
        p.with_rate_limit(Some(2.0));
        p.with_retries(7);

        let config = p.client_config();
        assert_eq!(config.timeout, 45);
        assert!(config.insecure);
        assert!(config.random_agent);
        assert_eq!(config.proxy.as_deref(), Some("http://proxy:8080"));
        assert_eq!(config.proxy_auth.as_deref(), Some("user:pass"));
        assert!(p.rate_limit.is_some());
        assert_eq!(p.retries, 7);
        assert_eq!(p.endpoint(), "https://vefsafn.is/cdx");
    }
}
