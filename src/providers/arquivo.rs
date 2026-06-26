use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use super::Provider;
use crate::network::client::{get_with_retry, HttpClientConfig};
use crate::network::RateLimiter;
use crate::progress::ProgressReporter;

/// Hard ceiling on the number of CDX pages walked for one domain. Arquivo.pt's
/// CDX server paginates large result sets into ZipNum blocks via `page=`, but a
/// domain small enough to fit in a single block ignores `page` and returns the
/// full set on every request. We stop as soon as a page contributes no new URLs
/// (see the fetch loop), so this ceiling is only a runaway backstop — at tens of
/// thousands of rows per page it covers far more URLs than any real domain has.
const MAX_PAGES: usize = 1_000;

/// One row of Arquivo.pt's CDX `output=json` response. Each line of the body is
/// a standalone JSON object (CDXJ / NDJSON); we only need the captured URL.
#[derive(Debug, Deserialize)]
struct ArquivoRecord {
    #[serde(default)]
    url: String,
}

/// Parse Arquivo's NDJSON CDX body into the captured URLs. Each non-empty line
/// is an independent JSON object, so a single malformed line (e.g. a stray
/// error message) is skipped rather than aborting the whole page. Rows without
/// a `url` are dropped.
fn parse_records(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            serde_json::from_str::<ArquivoRecord>(line)
                .ok()
                .map(|r| r.url)
                .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        })
        .collect()
}

#[derive(Clone)]
pub struct ArquivoProvider {
    include_subdomains: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    rate_limit: Option<RateLimiter>,
    #[cfg(test)]
    base_url: String,
}

impl ArquivoProvider {
    /// Creates a new ArquivoProvider with default settings.
    pub fn new() -> Self {
        ArquivoProvider {
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 60,
            retries: 3,
            random_agent: false,
            insecure: false,
            rate_limit: None,
            #[cfg(test)]
            base_url: "https://arquivo.pt".to_string(),
        }
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
            "https://arquivo.pt"
        }
    }

    /// Build the CDX query *without* the `page=` cursor. `output=json` streams
    /// one JSON object per line. A leading `*.` matches subdomains; a trailing
    /// `/*` matches the host and all of its paths — the same wildcard forms the
    /// Wayback provider uses, which Arquivo's CDX server honours as well.
    ///
    /// `collapse=urlkey` is essential, not just an optimisation: Arquivo returns
    /// one row per *capture*, and popular URLs accumulate thousands of captures
    /// (observed ~3.7× row inflation). Without collapsing, a single
    /// heavily-recaptured URL can fill an entire `page`, which the walk would
    /// see as "no new URLs" and stop early — silently under-collecting every URL
    /// that sorts after it. Collapsing adjacent duplicate urlkeys yields ~one row
    /// per unique URL per page, so a non-final page always carries new URLs.
    fn query_base(&self, domain: &str) -> String {
        if self.include_subdomains {
            format!(
                "{}/wayback/cdx?url=*.{domain}/*&output=json&collapse=urlkey",
                self.base_url()
            )
        } else {
            format!(
                "{}/wayback/cdx?url={domain}/*&output=json&collapse=urlkey",
                self.base_url()
            )
        }
    }
}

impl Provider for ArquivoProvider {
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
            let limiter = self.rate_limit.as_ref();

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            // Walk the `page=` cursor. Arquivo only paginates result sets that
            // span multiple ZipNum blocks; a domain that fits in a single block
            // ignores `page` and returns the full set on every request. So we
            // stop as soon as a page adds no new URLs (rather than waiting for an
            // empty page, which never comes for small domains). `seen` both
            // dedups across pages and drives that no-progress stop condition.
            let mut seen: HashSet<String> = HashSet::new();
            let mut urls: Vec<String> = Vec::new();
            let mut page = 0usize;

            loop {
                if page >= MAX_PAGES {
                    break;
                }

                let url = format!("{query_base}&page={page}");

                if let Some(rl) = &limiter {
                    rl.acquire().await;
                }
                let text = match get_with_retry(&client, &url, self.retries).await {
                    Ok(text) => text,
                    Err(e) => {
                        // Best effort: a mid-walk failure shouldn't discard the
                        // pages we already pulled. Only a failure on the very
                        // first request (nothing collected) is fatal.
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

                let mut new_in_page = 0usize;
                for u in parse_records(&text) {
                    if seen.insert(u.clone()) {
                        urls.push(u);
                        new_in_page += 1;
                    }
                }

                if let Some(r) = &reporter {
                    r.detail(format!("{} URLs…", urls.len()));
                }

                // No new URLs ⇒ either this was the last page, or the server is
                // ignoring `page` and re-serving the same rows. Either way, stop.
                if new_in_page == 0 {
                    break;
                }

                page += 1;
            }

            urls.sort();
            urls.dedup();

            Ok(urls)
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

    #[test]
    fn test_new_provider() {
        let provider = ArquivoProvider::new();
        assert!(!provider.include_subdomains);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert_eq!(provider.timeout, 60);
        assert_eq!(provider.retries, 3);
        assert!(!provider.random_agent);
        assert!(!provider.insecure);
        assert!(provider.rate_limit.is_none());
    }

    #[test]
    fn test_with_subdomains() {
        let mut provider = ArquivoProvider::new();
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = ArquivoProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = ArquivoProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = ArquivoProvider::new();
        provider.with_timeout(30);
        assert_eq!(provider.timeout, 30);
    }

    #[test]
    fn test_with_retries() {
        let mut provider = ArquivoProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = ArquivoProvider::new();
        provider.with_random_agent(true);
        assert!(provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = ArquivoProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_rate_limit() {
        let mut provider = ArquivoProvider::new();
        provider.with_rate_limit(Some(2.5));
        assert!(provider.rate_limit.is_some());
    }

    #[test]
    fn test_clone_box() {
        let provider = ArquivoProvider::new();
        let _cloned = provider.clone_box();
    }

    #[test]
    fn test_client_config() {
        let mut provider = ArquivoProvider::new();
        provider.with_timeout(45);
        provider.with_insecure(true);
        provider.with_random_agent(true);
        provider.with_proxy(Some("http://proxy:8080".to_string()));
        provider.with_proxy_auth(Some("user:pass".to_string()));

        let config = provider.client_config();
        assert_eq!(config.timeout, 45);
        assert!(config.insecure);
        assert!(config.random_agent);
        assert_eq!(config.proxy, Some("http://proxy:8080".to_string()));
        assert_eq!(config.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_query_base_without_subdomains() {
        let provider = ArquivoProvider::new();
        assert_eq!(
            provider.query_base("example.com"),
            "https://arquivo.pt/wayback/cdx?url=example.com/*&output=json&collapse=urlkey"
        );
    }

    #[test]
    fn test_query_base_with_subdomains() {
        let mut provider = ArquivoProvider::new();
        provider.with_subdomains(true);
        assert_eq!(
            provider.query_base("example.com"),
            "https://arquivo.pt/wayback/cdx?url=*.example.com/*&output=json&collapse=urlkey"
        );
    }

    #[test]
    fn test_parse_records_extracts_urls_and_skips_junk() {
        let body =
            "{\"urlkey\":\"com,example)/\",\"url\":\"http://example.com/a\",\"status\":\"200\"}\n\
                    \n\
                    not-json-just-an-error-line\n\
                    {\"url\":\"https://example.com/b\"}\n\
                    {\"timestamp\":\"20200101\"}\n\
                    {\"url\":\"ftp://example.com/skip\"}\n";
        let urls = parse_records(body);
        assert_eq!(
            urls,
            vec![
                "http://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_integration() {
        let mut server = mockito::Server::new_async().await;
        // Page 0 carries results (with a duplicate to prove dedup); page 1 is
        // empty, which terminates the walk.
        let page0 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("output".into(), "json".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "urlkey".into()),
                mockito::Matcher::UrlEncoded("page".into(), "0".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "text/x-ndjson")
            .with_body(
                "{\"url\":\"http://example.com/page1\"}\n\
                 {\"url\":\"http://example.com/page2\"}\n\
                 {\"url\":\"http://example.com/page1\"}\n",
            )
            .expect(1)
            .create_async()
            .await;
        let page1 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_body("")
            .expect(1)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "http://example.com/page1");
        assert_eq!(urls[1], "http://example.com/page2");

        page0.assert();
        page1.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_integration_with_subdomains() {
        let mut server = mockito::Server::new_async().await;
        let page0 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "*.example.com/*".into()),
                mockito::Matcher::UrlEncoded("output".into(), "json".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "urlkey".into()),
                mockito::Matcher::UrlEncoded("page".into(), "0".into()),
            ]))
            .with_status(200)
            .with_body("{\"url\":\"http://sub.example.com/page1\"}\n")
            .expect(1)
            .create_async()
            .await;
        let _page1 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_body("")
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());
        provider.with_subdomains(true);

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(urls, vec!["http://sub.example.com/page1".to_string()]);
        page0.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_paginates_across_pages() {
        let mut server = mockito::Server::new_async().await;
        // Page 0 and page 1 overlap on /b to prove cross-page dedup; page 2 is
        // empty so the walk terminates.
        let page0 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/a\"}\n{\"url\":\"http://example.com/b\"}\n")
            .expect(1)
            .create_async()
            .await;
        let page1 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/b\"}\n{\"url\":\"http://example.com/c\"}\n")
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "2".into()))
            .with_status(200)
            .with_body("")
            .expect(1)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
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
        page0.assert();
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_stops_when_page_param_ignored() {
        // A small domain fits in one ZipNum block, so Arquivo ignores `page` and
        // re-serves the same rows. The walk must stop after the first repeat
        // (page 1 adds no new URLs) instead of looping forever.
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/a\"}\n{\"url\":\"http://example.com/b\"}\n")
            .expect(2)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(
            urls,
            vec![
                "http://example.com/a".to_string(),
                "http://example.com/b".to_string(),
            ]
        );
        // Exactly two requests: page 0 (new rows) then page 1 (all duplicates).
        mock.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_integration_empty_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("")
            .expect(1)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(urls.len(), 0);
        mock.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_errors_when_first_request_fails() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::Any)
            .with_status(503)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());
        provider.with_retries(0);

        // Nothing collected yet → a hard failure must propagate.
        assert!(provider.fetch_urls("example.com").await.is_err());
    }

    #[tokio::test]
    async fn test_fetch_urls_keeps_partial_results_on_midwalk_failure() {
        let mut server = mockito::Server::new_async().await;
        // Page 0 succeeds...
        let _page0 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/a\"}\n{\"url\":\"http://example.com/b\"}\n")
            .expect(1)
            .create_async()
            .await;
        // ...but the follow-up page fails. We should keep page 0 rather than err.
        let _page1 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(503)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());
        provider.with_retries(0); // fail fast, don't sleep through back-off

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
        assert!(reporter.is_partial());
    }

    #[tokio::test]
    async fn test_rate_limit_paces_page_requests() {
        use std::time::{Duration, Instant};
        let mut server = mockito::Server::new_async().await;
        // Page 0 has new rows so a second request (page 1) is made.
        let _page0 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/a\"}\n")
            .expect(1)
            .create_async()
            .await;
        let _page1 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/b\"}\n")
            .expect(1)
            .create_async()
            .await;
        // Page 2 empty → terminate.
        let _page2 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "2".into()))
            .with_status(200)
            .with_body("")
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());
        // 5 req/s ⇒ a 200ms minimum gap between page requests.
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
}
