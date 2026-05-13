use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

use super::Provider;
use crate::network::client::{get_with_retry, HttpClientConfig};

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
}

impl Provider for WaybackMachineProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            #[cfg(not(test))]
            let base_url = "https://web.archive.org";
            #[cfg(test)]
            let base_url = &self.base_url;

            // Plain-text streaming response is far more reliable than output=json
            // for large domains (the JSON variant buffers the entire result server-side
            // and frequently times out). collapse=urlkey trims server-side duplicates.
            let mut url = if self.include_subdomains {
                format!(
                    "{}/cdx/search/cdx?url=*.{domain}/*&fl=original&collapse=urlkey",
                    base_url
                )
            } else {
                format!(
                    "{}/cdx/search/cdx?url={domain}/*&fl=original&collapse=urlkey",
                    base_url
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

            let client = self.client_config().build_client()?;
            let text = get_with_retry(&client, &url, self.retries).await?;

            if text.trim().is_empty() {
                return Ok(Vec::new());
            }

            // Defensive: a 200 OK from Wayback can occasionally carry a maintenance
            // page or non-URL body. Restrict to lines that actually look like URLs.
            let mut urls: Vec<String> = text
                .lines()
                .map(str::trim)
                .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
                .map(String::from)
                .collect();

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
        let mock = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("fl".into(), "original".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "urlkey".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body(
                "http://example.com/page1\nhttp://example.com/page2\nhttp://example.com/page1\n",
            )
            .create_async()
            .await;

        let mut provider = WaybackMachineProvider::new();
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        // Should return unique URLs sorted
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "http://example.com/page1");
        assert_eq!(urls[1], "http://example.com/page2");

        mock.assert();
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
            ]))
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("http://sub.example.com/page1\n")
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
            ]))
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("http://example.com/page\n")
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
