use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use super::filters::{ArchiveFilters, CdxDialect};
use super::Provider;
use crate::network::client::{get_with_retry, HttpClientConfig};
use crate::network::RateLimiter;
use crate::progress::ProgressReporter;

/// Hard ceiling on the number of CDX pages walked for one domain. Arquivo.pt
/// accepts `page=` but was measured to ignore it — `page=0` and `page=1` come
/// back byte-identical for both a five-row domain and a capped 100k-row one. We
/// stop as soon as a page contributes no new URLs (see the fetch loop), so this
/// ceiling is only a runaway backstop in case a future deployment does start
/// honouring the parameter.
const MAX_PAGES: usize = 1_000;

/// Rows to request per page. Arquivo.pt caps a `limit`-less response at 100,000
/// rows *silently*: the body simply stops, with no marker and no cursor to
/// resume from. Asking for that same bound explicitly is what makes the cap
/// detectable — a page that comes back holding exactly `ROW_LIMIT` rows was
/// truncated, and anything shorter is the complete result set.
///
/// Raising it does not help: `limit=300000` made the server stream so slowly
/// that a 300-second request delivered only 59k rows before timing out.
const ROW_LIMIT: usize = 100_000;

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
    /// Server-side CDX predicates (date range, status code, MIME type).
    filters: ArchiveFilters,
    #[cfg(test)]
    base_url: String,
    #[cfg(test)]
    row_limit: usize,
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
            filters: ArchiveFilters::default(),
            #[cfg(test)]
            base_url: "https://arquivo.pt".to_string(),
            #[cfg(test)]
            row_limit: ROW_LIMIT,
        }
    }

    /// Apply server-side CDX predicates (date range, status code, MIME type).
    /// Arquivo.pt serves a pywb-flavoured CDX API, so it shares Common Crawl's
    /// `status`/`mime` field names rather than the classic CDX ones.
    pub fn with_filters(&mut self, filters: ArchiveFilters) -> &mut Self {
        self.filters = filters;
        self
    }

    #[cfg(test)]
    pub fn with_base_url(&mut self, url: String) -> &mut Self {
        self.base_url = url;
        self
    }

    /// Shrink the per-page row bound so a test can exercise the truncated-page
    /// path without a mock body of a hundred thousand rows.
    #[cfg(test)]
    pub fn with_row_limit(&mut self, rows: usize) -> &mut Self {
        self.row_limit = rows;
        self
    }

    /// Rows requested per page. See [`ROW_LIMIT`].
    fn row_limit(&self) -> usize {
        #[cfg(test)]
        {
            self.row_limit
        }
        #[cfg(not(test))]
        {
            ROW_LIMIT
        }
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
    /// `fl=url` asks for just the captured URL, which is the only field
    /// [`parse_records`] reads. It matters at this scale: a full-record page of
    /// a large domain is ~25 MB where the same rows as `fl=url` are ~6 MB, and
    /// that whole body is buffered in memory before parsing.
    ///
    /// `collapse=urlkey` is sent because it is the correct request for a
    /// capture-level index — Arquivo returns one row per *capture*, so popular
    /// URLs repeat for thousands of rows — but the live server was measured to
    /// ignore it (`limit=3` returns three rows sharing one urlkey). Nothing here
    /// may assume the rows are collapsed.
    fn query_base(&self, domain: &str) -> String {
        let host = if self.include_subdomains {
            format!("*.{domain}")
        } else {
            domain.to_string()
        };
        let mut url = format!(
            "{}/wayback/cdx?url={host}/*&output=json&fl=url&collapse=urlkey",
            self.base_url()
        );
        url.push_str(&self.filters.query_params(CdxDialect::Pywb));
        url
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

            // Walk the `page=` cursor. Arquivo accepts `page` but was measured
            // to ignore it, so in practice this loop runs once — and that is the
            // point of the explicit `limit`: a page holding fewer rows than we
            // asked for is provably the complete result set, so we stop without
            // spending a second request re-downloading the identical body. Only
            // a page the server truncated is worth a follow-up, and if that
            // follow-up adds nothing then `page` is not advancing and the rest
            // of the result set is unreachable — a partial, not a clean, crawl.
            //
            // `seen` is the single source of truth: it dedups across pages and
            // its growth drives the no-progress stop condition.
            let row_limit = self.row_limit();
            let mut seen: HashSet<String> = HashSet::new();
            let mut page = 0usize;
            let mut truncated = false;

            loop {
                if page >= MAX_PAGES {
                    // Only reachable after a truncated page, i.e. with rows the
                    // server still has and we have not read.
                    truncated = true;
                    break;
                }

                let url = format!("{query_base}&limit={row_limit}&page={page}");

                if let Some(rl) = &limiter {
                    rl.acquire().await;
                }
                let text = match get_with_retry(&client, &url, self.retries).await {
                    Ok(text) => text,
                    Err(e) => {
                        // Best effort: a mid-walk failure shouldn't discard the
                        // pages we already pulled. Only a failure on the very
                        // first request (nothing collected) is fatal.
                        if seen.is_empty() {
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

                // Row count, not URL count: rows the server sent that we could
                // not parse, or that were duplicate captures of a URL we already
                // have, still count against `limit`. Only the raw row total says
                // whether the server truncated us.
                let rows = text.lines().filter(|l| !l.trim().is_empty()).count();
                let before = seen.len();
                seen.extend(parse_records(&text));

                if let Some(r) = &reporter {
                    r.detail(format!("{} URLs…", seen.len()));
                }

                // Short page ⇒ the server gave us everything it had for this
                // query. Done, and complete.
                if rows < row_limit {
                    break;
                }

                // The page was capped, so rows remain. Adding no new URLs means
                // `page` did not move us past them and never will.
                if seen.len() == before {
                    truncated = true;
                    break;
                }

                page += 1;
            }

            if truncated {
                if let Some(r) = &reporter {
                    r.mark_partial();
                }
            }

            let mut urls: Vec<String> = seen.into_iter().collect();
            urls.sort();

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
            "https://arquivo.pt/wayback/cdx?url=example.com/*&output=json&fl=url&collapse=urlkey"
        );
    }

    #[test]
    fn test_query_base_with_subdomains() {
        let mut provider = ArquivoProvider::new();
        provider.with_subdomains(true);
        assert_eq!(
            provider.query_base("example.com"),
            "https://arquivo.pt/wayback/cdx?url=*.example.com/*&output=json&fl=url&collapse=urlkey"
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
        // Page 0 carries results (with a duplicate to prove dedup) and comes
        // back short of the requested `limit`, so it is provably the whole
        // result set and no second page is requested.
        let page0 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("output".into(), "json".into()),
                mockito::Matcher::UrlEncoded("fl".into(), "url".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "urlkey".into()),
                mockito::Matcher::UrlEncoded("limit".into(), ROW_LIMIT.to_string()),
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
            .expect(0)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");
        let urls = provider
            .fetch_urls_with_progress("example.com", Some(reporter.clone()))
            .await
            .unwrap();

        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "http://example.com/page1");
        assert_eq!(urls[1], "http://example.com/page2");
        assert!(!reporter.is_partial());

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
        // Two rows per page is a full page here, so each page looks truncated
        // and the walk keeps going until a short one.
        provider.with_row_limit(2);

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
                "http://example.com/c".to_string(),
            ]
        );
        // Page 2 was short, so the walk reached the real end.
        assert!(!reporter.is_partial());
        page0.assert();
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_truncated_page_that_repeats_is_reported_partial() {
        // Arquivo ignores `page`: a truncated page 0 and page 1 come back with
        // the identical rows. The walk must stop after the first repeat instead
        // of looping forever — and, because the server told us (by filling the
        // page to the limit) that it still had rows we never reached, the result
        // must be flagged partial rather than passed off as a full crawl.
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
        provider.with_row_limit(2); // both pages come back full ⇒ truncated

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
        // Exactly two requests: page 0 (new rows) then page 1 (all duplicates).
        mock.assert();
    }

    #[tokio::test]
    async fn test_short_page_costs_only_one_request() {
        // `page` is ignored by the live server, so a follow-up page re-downloads
        // the identical body — up to ~25 MB on a large domain — for nothing. A
        // page shorter than the requested limit is provably complete, so there
        // is no follow-up to make.
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/a\"}\n{\"url\":\"http://example.com/b\"}\n")
            .expect(1)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());
        provider.with_row_limit(10); // 2 rows < 10 ⇒ that was everything

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");
        let urls = provider
            .fetch_urls_with_progress("example.com", Some(reporter.clone()))
            .await
            .unwrap();

        assert_eq!(urls.len(), 2);
        assert!(!reporter.is_partial());
        mock.assert();
    }

    #[tokio::test]
    async fn test_duplicate_captures_still_count_against_the_row_limit() {
        // Arquivo ignores `collapse=urlkey` and returns one row per capture, so
        // a page can be full of rows while adding few unique URLs. Truncation
        // must be judged on rows received, not on URLs collected, or a page of
        // repeats reads as "short" and the rest of the domain is dropped.
        let mut server = mockito::Server::new_async().await;
        let page0 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            // Three rows, one unique URL — a capture-level index doing its thing.
            .with_body(
                "{\"url\":\"http://example.com/a\"}\n\
                 {\"url\":\"http://example.com/a\"}\n\
                 {\"url\":\"http://example.com/a\"}\n",
            )
            .expect(1)
            .create_async()
            .await;
        let page1 = server
            .mock("GET", "/wayback/cdx")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_body("{\"url\":\"http://example.com/b\"}\n")
            .expect(1)
            .create_async()
            .await;

        let mut provider = ArquivoProvider::new();
        provider.with_base_url(server.url());
        provider.with_row_limit(3);

        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert_eq!(
            urls,
            vec![
                "http://example.com/a".to_string(),
                "http://example.com/b".to_string(),
            ]
        );
        page0.assert();
        page1.assert();
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
        provider.with_row_limit(2); // page 0 comes back full ⇒ page 1 is fetched

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
        // One row fills a page here, so page 0 looks truncated and page 1 is
        // fetched — which is what gives the limiter something to pace.
        provider.with_row_limit(1);
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

    #[test]
    fn test_query_base_uses_pywb_filter_fields() {
        // Arquivo.pt shares Common Crawl's exact-match pywb dialect. An
        // anchored regex here was observed to hang the server rather than
        // return an empty page, so the value must reach it verbatim.
        let mut provider = ArquivoProvider::new();
        provider.with_base_url("https://arquivo.pt".to_string());
        provider.with_filters(ArchiveFilters::from_cli_lists(
            None,
            None,
            &["200".to_string()],
            &["404".to_string(), "500".to_string()],
            &[],
            &[],
        ));

        let q = provider.query_base("example.com");
        assert!(q.contains("&filter=status:200"), "{q}");
        // Exclusions are AND-ed by the server, which is the wanted semantics,
        // so each one travels as its own parameter.
        assert!(q.contains("&filter=!status:404"), "{q}");
        assert!(q.contains("&filter=!status:500"), "{q}");
        assert!(q.contains("collapse=urlkey"), "{q}");
        assert!(!q.contains("statuscode"), "{q}");
    }
}
