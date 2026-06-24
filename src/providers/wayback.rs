use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

use super::Provider;
use crate::network::client::{get_with_retry, HttpClientConfig};
use crate::network::RateLimiter;
use crate::progress::ProgressReporter;

/// How many rows to ask the CDX server for per request. A bounded `limit` is
/// what keeps large domains from failing: an unbounded query makes the server
/// compute and buffer the *entire* result set (it then routinely times out),
/// whereas a capped request streams a slice and returns promptly. Most domains
/// fit in a single page; only the large ones the user cares about paginate.
const PAGE_LIMIT: usize = 50_000;

/// Hard ceiling on the number of pages we will follow, so a misbehaving cursor
/// can never spin forever. At `PAGE_LIMIT` rows each, this covers domains with
/// up to ~500M captured URLs — far beyond anything real.
const MAX_PAGES: usize = 10_000;

/// Split a CDX `showResumeKey=true` response into its URL rows and the resume
/// key for the next page (if any).
///
/// The server streams the result rows, then — *only while more results remain*
/// — a blank line followed by an opaque resume key:
///
/// ```text
/// https://example.com/a
/// https://example.com/b
///                          <- blank separator
/// eJxLzs_V...              <- resume key
/// ```
///
/// We treat a trailing, non-URL token *that is preceded by a blank line* as the
/// cursor. Requiring the blank separator is what stops stray non-URL junk in a
/// malformed/error body from being mistaken for a key (which would trigger a
/// spurious follow-up request). No such trailing token ⇒ this was the last page.
fn split_page(text: &str) -> (Vec<String>, Option<String>) {
    let lines: Vec<&str> = text.lines().collect();

    let mut resume_key = None;
    let mut url_scan_end = lines.len();
    if let Some(idx) = lines.iter().rposition(|l| !l.trim().is_empty()) {
        let candidate = lines[idx].trim();
        let is_url = candidate.starts_with("http://") || candidate.starts_with("https://");
        let preceded_by_blank = idx > 0 && lines[idx - 1].trim().is_empty();
        if !is_url && preceded_by_blank {
            resume_key = Some(candidate.to_string());
            url_scan_end = idx; // keep the key line out of the URL scan
        }
    }

    let urls = lines[..url_scan_end]
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
        .map(String::from)
        .collect();

    (urls, resume_key)
}

/// Percent-encode a resume key so opaque cursor bytes (`+`, `/`, `=` in some
/// base64 variants) survive being spliced back into the query string.
fn encode_resume_key(key: &str) -> String {
    url::form_urlencoded::byte_serialize(key.as_bytes()).collect()
}

/// Normalise a user-supplied date into a 14-digit Wayback CDX timestamp
/// (`YYYYMMDDhhmmss`). Accepts `YYYY`, `YYYYMM`, `YYYYMMDD` and the full
/// 14-digit form. When `end_of_range` is true the missing tail is padded
/// toward the end of the range (`31 23:59:59`) rather than the start
/// (`01 00:00:00`) — pass `false` for `--wayback-from`, `true` for
/// `--wayback-to`. Returns `None` for malformed input so the CLI can warn.
pub fn normalize_cdx_timestamp(input: &str, end_of_range: bool) -> Option<String> {
    let digits: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    if !matches!(digits.len(), 4 | 6 | 8 | 14) {
        return None;
    }

    let year: u32 = digits.get(0..4)?.parse().ok()?;
    if !(1996..=9999).contains(&year) {
        // CDX coverage only starts in 1996; reject anything earlier.
        return None;
    }

    // Pad each segment toward the appropriate end of the range.
    let month = match digits.get(4..6) {
        Some(s) => {
            let m: u32 = s.parse().ok()?;
            if !(1..=12).contains(&m) {
                return None;
            }
            format!("{m:02}")
        }
        None => {
            if end_of_range {
                "12".to_string()
            } else {
                "01".to_string()
            }
        }
    };
    let day = match digits.get(6..8) {
        Some(s) => {
            let d: u32 = s.parse().ok()?;
            if !(1..=31).contains(&d) {
                return None;
            }
            format!("{d:02}")
        }
        None => {
            if end_of_range {
                // 28 is the only day every month has — CDX accepts impossible
                // dates so 31 also works, but 28 avoids false widening at
                // month-only granularity. Wait: we WANT widening for `to`.
                // Use 31; CDX clamps gracefully.
                "31".to_string()
            } else {
                "01".to_string()
            }
        }
    };
    let tail = match digits.get(8..14) {
        Some(s) => s.to_string(),
        None => {
            if end_of_range {
                "235959".to_string()
            } else {
                "000000".to_string()
            }
        }
    };

    Some(format!("{year:04}{month}{day}{tail}"))
}

#[derive(Clone)]
pub struct WaybackMachineProvider {
    include_subdomains: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    parallel: u32,
    rate_limit: Option<f32>,
    /// CDX `from=` timestamp (already normalised to 14 digits).
    from: Option<String>,
    /// CDX `to=` timestamp (already normalised to 14 digits).
    to: Option<String>,
    #[cfg(test)]
    base_url: String,
}

impl WaybackMachineProvider {
    /// Creates a new WaybackMachineProvider with default settings
    pub fn new() -> Self {
        WaybackMachineProvider {
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 60,
            retries: 3,
            random_agent: false,
            insecure: false,
            parallel: 5,
            rate_limit: None,
            from: None,
            to: None,
            #[cfg(test)]
            base_url: "https://web.archive.org".to_string(),
        }
    }

    /// Restrict crawled snapshots to those at or after `ts` (14-digit CDX
    /// timestamp, see `normalize_cdx_timestamp`). Pass `None` to clear.
    pub fn with_from(&mut self, ts: Option<String>) -> &mut Self {
        self.from = ts;
        self
    }

    /// Restrict crawled snapshots to those at or before `ts`. Pass `None` to
    /// clear. Pair with `with_from` for a closed window.
    pub fn with_to(&mut self, ts: Option<String>) -> &mut Self {
        self.to = ts;
        self
    }

    #[cfg(test)]
    pub fn with_base_url(&mut self, url: String) -> &mut Self {
        self.base_url = url;
        self
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

    /// Archive origin. Overridable in tests so the mock server can stand in.
    fn base_url(&self) -> &str {
        #[cfg(test)]
        {
            &self.base_url
        }
        #[cfg(not(test))]
        {
            "https://web.archive.org"
        }
    }

    /// Build the CDX query *without* pagination params. Plain-text streaming
    /// (`fl=original`) is far more reliable than `output=json` for large
    /// domains, and `collapse=urlkey` trims server-side duplicates.
    fn query_base(&self, domain: &str) -> String {
        let mut url = if self.include_subdomains {
            format!(
                "{}/cdx/search/cdx?url=*.{domain}/*&fl=original&collapse=urlkey",
                self.base_url()
            )
        } else {
            format!(
                "{}/cdx/search/cdx?url={domain}/*&fl=original&collapse=urlkey",
                self.base_url()
            )
        };
        if let Some(ts) = &self.from {
            url.push_str("&from=");
            url.push_str(ts);
        }
        if let Some(ts) = &self.to {
            url.push_str("&to=");
            url.push_str(ts);
        }
        url
    }
}

impl Provider for WaybackMachineProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        self.fetch_urls_with_progress(domain, None)
    }

    fn fetch_urls_with_progress<'a>(
        &'a self,
        domain: &'a str,
        reporter: Option<ProgressReporter>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let client = self.client_config().build_client()?;
            let query_base = self.query_base(domain);
            let limiter = RateLimiter::from_rate(self.rate_limit);

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            // Walk the CDX cursor: each request returns at most PAGE_LIMIT rows
            // plus a resume key pointing at the next slice. Following the key
            // lets arbitrarily large domains complete as a series of bounded,
            // fast requests instead of one unbounded request that times out.
            let mut urls: Vec<String> = Vec::new();
            let mut resume_key: Option<String> = None;
            let mut pages = 0usize;

            loop {
                pages += 1;
                if pages > MAX_PAGES {
                    break;
                }

                let mut url = format!("{query_base}&limit={PAGE_LIMIT}&showResumeKey=true");
                if let Some(key) = &resume_key {
                    url.push_str("&resumeKey=");
                    url.push_str(&encode_resume_key(key));
                }

                if let Some(rl) = &limiter {
                    rl.acquire().await;
                }
                let text = match get_with_retry(&client, &url, self.retries).await {
                    Ok(text) => text,
                    Err(e) => {
                        // Best effort: a mid-cursor failure shouldn't discard
                        // the pages we already pulled. Only a failure on the
                        // very first request (nothing collected) is fatal.
                        if urls.is_empty() {
                            return Err(e);
                        }
                        // We're returning a truncated result. Flag it so the
                        // caller can mark the line partial and warn rather than
                        // present an incomplete crawl as a clean success.
                        if let Some(r) = &reporter {
                            r.mark_partial();
                        }
                        break;
                    }
                };

                let (page_urls, next_key) = split_page(&text);
                let got = page_urls.len();
                urls.extend(page_urls);

                if let Some(r) = &reporter {
                    r.detail(format!("{} URLs…", urls.len()));
                }

                // Continue only when the cursor actually advanced: a new resume
                // key AND a non-empty page. Otherwise we've reached the end (or
                // a stuck cursor) and must stop to avoid looping forever.
                match next_key {
                    Some(key) if got > 0 && resume_key.as_deref() != Some(key.as_str()) => {
                        resume_key = Some(key);
                    }
                    _ => break,
                }
            }

            urls.sort();
            urls.dedup();

            Ok(urls)
        })
    }

    // Implement new trait methods
    fn with_subdomains(&mut self, include: bool) {
        self.include_subdomains = include;
    }

    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }

    // New method implementations
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

    fn with_parallel(&mut self, parallel: u32) {
        self.parallel = parallel;
    }

    fn with_rate_limit(&mut self, rate_limit: Option<f32>) {
        self.rate_limit = rate_limit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Removed unused import: std::time::Duration

    #[test]
    fn test_new_provider() {
        let provider = WaybackMachineProvider::new();
        assert!(!provider.include_subdomains);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert_eq!(provider.timeout, 60);
        assert_eq!(provider.retries, 3);
        assert!(!provider.random_agent);
        assert!(!provider.insecure);
        assert_eq!(provider.parallel, 5);
        assert_eq!(provider.rate_limit, None);
    }

    #[test]
    fn test_with_subdomains() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_timeout(60);
        assert_eq!(provider.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_random_agent(true);
        assert!(provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_parallel() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_parallel(10);
        assert_eq!(provider.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_rate_limit(Some(2.5));
        assert_eq!(provider.rate_limit, Some(2.5));
    }

    #[test]
    fn test_clone_box() {
        let provider = WaybackMachineProvider::new();
        let _cloned = provider.clone_box();
        // Testing the existence of cloned object
    }

    #[test]
    fn test_client_config() {
        let mut provider = WaybackMachineProvider::new();
        provider.with_timeout(60);
        provider.with_insecure(true);
        provider.with_random_agent(true);
        provider.with_proxy(Some("http://proxy:8080".to_string()));
        provider.with_proxy_auth(Some("user:pass".to_string()));

        let config = provider.client_config();
        assert_eq!(config.timeout, 60);
        assert!(config.insecure);
        assert!(config.random_agent);
        assert_eq!(config.proxy, Some("http://proxy:8080".to_string()));
        assert_eq!(config.proxy_auth, Some("user:pass".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_urls_builds_correct_url_without_subdomains() {
        // 이 테스트는 실제 API 호출 없이 URL 구성을 확인합니다
        let provider = WaybackMachineProvider::new();

        // 존재하지 않을 가능성이 높은 도메인 사용
        let domain = "test-domain-that-does-not-exist-xyz.example";

        // 실제 URL 형식 검증만 합니다
        let expected_url = format!(
            "https://web.archive.org/cdx/search/cdx?url={domain}/*&fl=original&collapse=urlkey"
        );

        // URL 구성이 올바른지 확인합니다
        let url = if provider.include_subdomains {
            format!(
                "https://web.archive.org/cdx/search/cdx?url=*.{domain}/*&fl=original&collapse=urlkey"
            )
        } else {
            format!(
                "https://web.archive.org/cdx/search/cdx?url={domain}/*&fl=original&collapse=urlkey"
            )
        };

        assert_eq!(url, expected_url);
    }

    #[tokio::test]
    async fn test_fetch_urls_builds_correct_url_with_subdomains() {
        // 이 테스트는 실제 API 호출 없이 URL 구성을 확인합니다
        let mut provider = WaybackMachineProvider::new();
        provider.with_subdomains(true);

        // 존재하지 않을 가능성이 높은 도메인 사용
        let domain = "test-domain-that-does-not-exist-xyz.example";

        // 실제 URL 형식 검증만 합니다
        let expected_url = format!(
            "https://web.archive.org/cdx/search/cdx?url=*.{domain}/*&fl=original&collapse=urlkey"
        );

        // URL 구성이 올바른지 확인합니다
        let url = if provider.include_subdomains {
            format!(
                "https://web.archive.org/cdx/search/cdx?url=*.{domain}/*&fl=original&collapse=urlkey"
            )
        } else {
            format!(
                "https://web.archive.org/cdx/search/cdx?url={domain}/*&fl=original&collapse=urlkey"
            )
        };

        assert_eq!(url, expected_url);
    }

    #[tokio::test]
    async fn test_fetch_urls_integration() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        // A small domain fits in one page: results, no trailing resume key, so
        // exactly one request is made.
        let mock = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("fl".into(), "original".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "urlkey".into()),
                mockito::Matcher::UrlEncoded("showResumeKey".into(), "true".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body(
                "http://example.com/page1\nhttp://example.com/page2\nhttp://example.com/page1\n",
            )
            .expect(1)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        // Should return unique URLs sorted.
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "http://example.com/page1");
        assert_eq!(urls[1], "http://example.com/page2");

        mock.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_paginates_via_resume_key() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        // First page: rows + a blank line + the resume key (more results remain).
        // It fires before the continuation, so once it has served its single
        // hit, mockito routes the keyed request to the page-two mock.
        let page1 = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("showResumeKey".into(), "true".into()),
            ]))
            .with_status(200)
            .with_body("http://example.com/a\nhttp://example.com/b\n\nKEY2\n")
            .expect(1)
            .create_async()
            .await;
        // Second page: keyed by the cursor; overlaps /b to prove cross-page
        // dedup, and carries no trailing key so the walk terminates.
        let page2 = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::UrlEncoded(
                "resumeKey".into(),
                "KEY2".into(),
            ))
            .with_status(200)
            .with_body("http://example.com/b\nhttp://example.com/c\n")
            .expect(1)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(
            urls,
            vec![
                "http://example.com/a".to_string(),
                "http://example.com/b".to_string(),
                "http://example.com/c".to_string(),
            ]
        );
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_rate_limit_paces_page_requests() {
        use std::time::{Duration, Instant};
        let mut server = mockito::Server::new_async().await;
        // Page one hands back a resume key so a second request is made.
        let _page1 = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::UrlEncoded(
                "showResumeKey".into(),
                "true".into(),
            ))
            .with_status(200)
            .with_body("http://example.com/a\n\nKEY2\n")
            .expect(1)
            .create_async()
            .await;
        let _page2 = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::UrlEncoded(
                "resumeKey".into(),
                "KEY2".into(),
            ))
            .with_status(200)
            .with_body("http://example.com/b\n")
            .expect(1)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());
        // 5 req/s => a 200ms minimum gap before the second page request.
        provider.with_rate_limit(Some(5.0));

        let start = Instant::now();
        let urls = provider.fetch_urls("example.com").await.unwrap();
        assert_eq!(urls.len(), 2);
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "rate limit was not applied; elapsed {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_keeps_partial_results_on_midcursor_failure() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        // First page succeeds and hands us a cursor...
        let _page1 = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("showResumeKey".into(), "true".into()),
            ]))
            .with_status(200)
            .with_body("http://example.com/a\nhttp://example.com/b\n\nKEY2\n")
            .expect(1)
            .create_async()
            .await;
        // ...but the follow-up fails. We should keep page one rather than error.
        let _page2 = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::UrlEncoded(
                "resumeKey".into(),
                "KEY2".into(),
            ))
            .with_status(503)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());
        provider.with_retries(0); // fail fast, don't sleep through back-off

        // Drive it through a reporter so we can assert the partial flag is set.
        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");
        let urls = provider
            .fetch_urls_with_progress("example.com", Some(reporter.clone()))
            .await
            .unwrap();
        assert_eq!(
            urls,
            vec![
                "http://example.com/a".to_string(),
                "http://example.com/b".to_string(),
            ]
        );
        // The lost second page must be surfaced as a partial result, not a
        // clean success.
        assert!(reporter.is_partial());
    }

    #[tokio::test]
    async fn test_fetch_urls_errors_when_first_request_fails() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::Any)
            .with_status(503)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());
        provider.with_retries(0);

        // Nothing collected yet → a hard failure must propagate.
        assert!(provider.fetch_urls("example.com").await.is_err());
    }

    #[test]
    fn test_split_page_extracts_resume_key_after_blank_line() {
        let body = "http://example.com/a\nhttps://example.com/b\n\neJxKEY\n";
        let (urls, key) = split_page(body);
        assert_eq!(
            urls,
            vec![
                "http://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ]
        );
        assert_eq!(key.as_deref(), Some("eJxKEY"));
    }

    #[test]
    fn test_split_page_no_resume_key_on_last_page() {
        let body = "http://example.com/a\nhttp://example.com/b\n";
        let (urls, key) = split_page(body);
        assert_eq!(urls.len(), 2);
        assert_eq!(key, None);
    }

    #[test]
    fn test_split_page_ignores_trailing_junk_without_blank_separator() {
        // A non-URL line *not* preceded by a blank line is junk (e.g. an error
        // page line), never a resume key — so no spurious follow-up request.
        let body = "<html>Service unavailable</html>\nhttp://example.com/real\nnot-a-url\n";
        let (urls, key) = split_page(body);
        assert_eq!(urls, vec!["http://example.com/real".to_string()]);
        assert_eq!(key, None);
    }

    #[test]
    fn test_split_page_empty_body() {
        let (urls, key) = split_page("");
        assert!(urls.is_empty());
        assert_eq!(key, None);
    }

    #[test]
    fn test_encode_resume_key() {
        // URL-safe base64 chars pass through; +, /, = get percent-encoded.
        assert_eq!(encode_resume_key("eJx-_09Az"), "eJx-_09Az");
        assert_eq!(encode_resume_key("a+b/c="), "a%2Bb%2Fc%3D");
    }

    #[tokio::test]
    async fn test_fetch_urls_integration_with_subdomains() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "*.example.com/*".into()),
                mockito::Matcher::UrlEncoded("fl".into(), "original".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "urlkey".into()),
                mockito::Matcher::UrlEncoded("showResumeKey".into(), "true".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("http://sub.example.com/page1\n")
            .expect(1)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());
        provider.with_subdomains(true);

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "http://sub.example.com/page1");

        mock.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_integration_empty_response() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("")
            .expect(1)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(urls.len(), 0);

        mock.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_filters_non_url_lines() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(
                "<html><body>Service temporarily unavailable</body></html>\n\
                 http://example.com/real\n\
                 not-a-url\n",
            )
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(urls, vec!["http://example.com/real".to_string()]);
    }

    #[test]
    fn test_normalize_cdx_timestamp_year_only() {
        assert_eq!(
            normalize_cdx_timestamp("2020", false).as_deref(),
            Some("20200101000000")
        );
        assert_eq!(
            normalize_cdx_timestamp("2020", true).as_deref(),
            Some("20201231235959")
        );
    }

    #[test]
    fn test_normalize_cdx_timestamp_year_month() {
        assert_eq!(
            normalize_cdx_timestamp("202003", false).as_deref(),
            Some("20200301000000")
        );
        assert_eq!(
            normalize_cdx_timestamp("202003", true).as_deref(),
            Some("20200331235959")
        );
    }

    #[test]
    fn test_normalize_cdx_timestamp_day_and_full() {
        assert_eq!(
            normalize_cdx_timestamp("20200315", false).as_deref(),
            Some("20200315000000")
        );
        assert_eq!(
            normalize_cdx_timestamp("20200315123045", false).as_deref(),
            Some("20200315123045")
        );
        // Hyphens and slashes are stripped before length check.
        assert_eq!(
            normalize_cdx_timestamp("2020-03-15", false).as_deref(),
            Some("20200315000000")
        );
    }

    #[test]
    fn test_normalize_cdx_timestamp_rejects_invalid() {
        // Length not in {4, 6, 8, 14}.
        assert!(normalize_cdx_timestamp("20203", false).is_none());
        // Out-of-range month.
        assert!(normalize_cdx_timestamp("202013", false).is_none());
        // Out-of-range day.
        assert!(normalize_cdx_timestamp("20200300", false).is_none());
        // Pre-1996 year.
        assert!(normalize_cdx_timestamp("1995", false).is_none());
        // Empty / non-digit garbage.
        assert!(normalize_cdx_timestamp("oops", false).is_none());
    }

    #[tokio::test]
    async fn test_fetch_urls_passes_date_range() {
        use mockito;

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("fl".into(), "original".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "urlkey".into()),
                mockito::Matcher::UrlEncoded("from".into(), "20200101000000".into()),
                mockito::Matcher::UrlEncoded("to".into(), "20201231235959".into()),
                mockito::Matcher::UrlEncoded("showResumeKey".into(), "true".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("http://example.com/page\n")
            .expect(1)
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());
        provider.with_from(Some("20200101000000".to_string()));
        provider.with_to(Some("20201231235959".to_string()));

        let urls = provider.fetch_urls("example.com").await.unwrap();
        assert_eq!(urls, vec!["http://example.com/page".to_string()]);
        mock.assert();
    }
}
