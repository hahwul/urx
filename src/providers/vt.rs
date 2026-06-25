use anyhow::Result;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;

use super::ApiKeyRotator;
use super::Provider;
use crate::network::client::HttpClientConfig;
use crate::network::RateLimiter;
use crate::progress::ProgressReporter;

/// Page size for the v3 `urls` relationship. VirusTotal caps this relationship
/// endpoint at 40 per page; larger values are silently clamped server-side.
const VT_PAGE_LIMIT: usize = 40;

/// Hard ceiling on pages followed for one domain, mirroring the other
/// paginating providers so a misbehaving `links.next` can't loop forever.
const VT_MAX_PAGES: usize = 10_000;

#[derive(Clone)]
pub struct VirusTotalProvider {
    api_key_rotator: ApiKeyRotator,
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

/// A page of the v3 `/domains/{domain}/urls` response. `data` holds the URL
/// objects; `links.next` is a complete URL for the following page (absent on
/// the last page). `Default` lets a 404 ("no such domain") resolve to an empty
/// page rather than an error.
#[derive(Debug, Deserialize, Default)]
struct VtUrlsResponse {
    #[serde(default)]
    data: Vec<VtUrlObject>,
    #[serde(default)]
    links: VtLinks,
}

#[derive(Debug, Deserialize, Default)]
struct VtLinks {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VtUrlObject {
    attributes: VtUrlAttributes,
}

#[derive(Debug, Deserialize)]
struct VtUrlAttributes {
    url: String,
}

impl VirusTotalProvider {
    #[allow(dead_code)]
    pub fn new(api_key: String) -> Self {
        if api_key.is_empty() {
            Self::new_with_keys(vec![])
        } else {
            Self::new_with_keys(vec![api_key])
        }
    }

    pub fn new_with_keys(api_keys: Vec<String>) -> Self {
        // Filter out empty keys
        let filtered_keys: Vec<String> = api_keys.into_iter().filter(|k| !k.is_empty()).collect();

        VirusTotalProvider {
            api_key_rotator: ApiKeyRotator::new(filtered_keys),
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            rate_limit: None,
            #[cfg(test)]
            base_url: "https://www.virustotal.com".to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(&mut self, url: String) -> &mut Self {
        self.base_url = url;
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

    /// First-page URL for a domain's v3 `urls` relationship.
    fn first_page_url(&self, domain: &str) -> String {
        let encoded = url::form_urlencoded::byte_serialize(domain.as_bytes()).collect::<String>();
        #[cfg(test)]
        {
            format!(
                "{}/api/v3/domains/{encoded}/urls?limit={VT_PAGE_LIMIT}",
                self.base_url
            )
        }
        #[cfg(not(test))]
        {
            format!(
                "https://www.virustotal.com/api/v3/domains/{encoded}/urls?limit={VT_PAGE_LIMIT}"
            )
        }
    }

    /// Fetch and parse a single page with retry/back-off and key rotation.
    ///
    /// A 404 (the domain has no VT object) resolves to an empty page rather
    /// than an error, matching the "no data" semantics of the other providers.
    async fn fetch_page(
        &self,
        client: &reqwest::Client,
        url: &str,
        limiter: Option<&RateLimiter>,
    ) -> Result<VtUrlsResponse> {
        let mut last_error = None;
        let mut attempt = 0;

        while attempt <= self.retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }

            // Rotate the key per attempt so a throttled/invalid key is retried
            // with a different one when several are configured. v3 carries the
            // key in the `x-apikey` header (v2 used an `apikey` query param).
            let api_key = self.api_key_rotator.next_key().unwrap_or_default();
            let mut req = client.get(url);
            if !api_key.is_empty() {
                req = req.header("x-apikey", &api_key);
            }

            if let Some(rl) = limiter {
                rl.acquire().await;
            }
            match req.send().await {
                Ok(response) => {
                    let status = response.status();
                    // 404 => no VT object for this domain; not an error.
                    if status.as_u16() == 404 {
                        return Ok(VtUrlsResponse::default());
                    }
                    if !status.is_success() {
                        // On a throttle, wait as long as the server asked.
                        if status.as_u16() == 429 {
                            if let Some(d) =
                                crate::network::client::retry_after_delay(response.headers())
                            {
                                tokio::time::sleep(d).await;
                            }
                        }
                        attempt += 1;
                        last_error = Some(anyhow::anyhow!("HTTP error: {status}"));
                        continue;
                    }
                    match response.json::<VtUrlsResponse>().await {
                        Ok(parsed) => return Ok(parsed),
                        Err(e) => {
                            attempt += 1;
                            last_error =
                                Some(anyhow::anyhow!("Failed to parse VirusTotal response: {e}"));
                            continue;
                        }
                    }
                }
                Err(e) => {
                    attempt += 1;
                    // Defensive hygiene: keep the request URL out of surfaced
                    // transport errors (the key is a header, not in the URL).
                    last_error = Some(e.without_url().into());
                    continue;
                }
            }
        }

        Err(anyhow::anyhow!(
            "Failed after {} attempts: {}",
            self.retries + 1,
            last_error.unwrap_or_else(|| anyhow::anyhow!("unknown error"))
        ))
    }
}

impl Provider for VirusTotalProvider {
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
            // Skip if no API keys are provided.
            if !self.api_key_rotator.has_keys() {
                return Ok(Vec::new());
            }

            let client = self.client_config().build_client()?;
            let limiter = self.rate_limit.as_ref();

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            // Walk the v3 cursor: each page returns up to VT_PAGE_LIMIT URL
            // objects plus a `links.next` pointing at the following page. The
            // deprecated v2 `domain/report` returned a single server-capped,
            // non-paginated slice that silently truncated large domains.
            let mut next_url = Some(self.first_page_url(domain));
            let mut urls = Vec::new();
            let mut pages = 0usize;

            while let Some(url) = next_url.take() {
                pages += 1;
                if pages > VT_MAX_PAGES {
                    break;
                }

                let page = match self.fetch_page(&client, &url, limiter).await {
                    Ok(page) => page,
                    Err(e) => {
                        // A failure on the first page (nothing collected) is
                        // fatal; a later failure keeps what we have and flags
                        // the result partial rather than presenting a truncated
                        // crawl as a clean success.
                        if urls.is_empty() {
                            return Err(e);
                        }
                        if let Some(r) = &reporter {
                            r.mark_partial();
                        }
                        break;
                    }
                };

                for obj in page.data {
                    urls.push(obj.attributes.url);
                }
                if let Some(r) = &reporter {
                    r.detail(format!("{} URLs…", urls.len()));
                }

                next_url = page.links.next;
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
        let api_key = "test_api_key".to_string();
        let provider = VirusTotalProvider::new(api_key.clone());
        assert!(provider.api_key_rotator.has_keys());
        assert_eq!(provider.api_key_rotator.key_count(), 1);
        assert_eq!(provider.api_key_rotator.current_key(), Some(api_key));
        assert!(!provider.include_subdomains);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert_eq!(provider.timeout, 30);
        assert_eq!(provider.retries, 3);
        assert!(!provider.random_agent);
        assert!(!provider.insecure);
        assert!(provider.rate_limit.is_none());
    }

    #[test]
    fn test_new_provider_with_multiple_keys() {
        let api_keys = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let provider = VirusTotalProvider::new_with_keys(api_keys.clone());

        assert!(provider.api_key_rotator.has_keys());
        assert_eq!(provider.api_key_rotator.key_count(), 3);

        // Test rotation
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key1".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key2".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key3".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key1".to_string())
        ); // Should wrap
    }

    #[test]
    fn test_new_provider_filters_empty_keys() {
        let api_keys = vec![
            "key1".to_string(),
            "".to_string(),
            "key2".to_string(),
            "".to_string(),
        ];
        let provider = VirusTotalProvider::new_with_keys(api_keys);

        assert!(provider.api_key_rotator.has_keys());
        assert_eq!(provider.api_key_rotator.key_count(), 2);
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key1".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key2".to_string())
        );
    }

    #[test]
    fn test_new_provider_with_empty_key() {
        let provider = VirusTotalProvider::new("".to_string());
        assert!(!provider.api_key_rotator.has_keys());
        assert_eq!(provider.api_key_rotator.key_count(), 0);
        assert_eq!(provider.api_key_rotator.current_key(), None);
    }

    #[tokio::test]
    async fn test_transport_error_does_not_leak_api_key() {
        // The v3 key travels in the `x-apikey` header, but a transport-layer
        // failure (which reqwest renders with the full URL) must still not
        // surface it — and we strip the URL from the error for good measure.
        let mut provider = VirusTotalProvider::new_with_keys(vec!["SUPERSECRETKEY".to_string()]);
        // Port 1 reliably refuses the connection; keep the run fast.
        provider.with_base_url("http://127.0.0.1:1".to_string());
        provider.with_retries(0);
        provider.with_timeout(5);

        let err = provider
            .fetch_urls("example.com")
            .await
            .expect_err("connection to port 1 should fail");
        let msg = err.to_string();
        assert!(
            !msg.contains("SUPERSECRETKEY"),
            "API key leaked in error message: {msg}"
        );
    }

    #[test]
    fn test_with_subdomains() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_timeout(60);
        assert_eq!(provider.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_random_agent(true);
        assert!(provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_rate_limit() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_rate_limit(Some(2.5));
        assert!(provider.rate_limit.is_some());
    }

    #[test]
    fn test_clone_box() {
        let provider = VirusTotalProvider::new("test_api_key".to_string());
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[test]
    fn test_vt_response_deserialize() {
        // v3 shape: data[].attributes.url, with a links.next cursor.
        let json = r#"{
            "data": [
                {"type": "url", "id": "a", "attributes": {"url": "https://example.com/page1"}},
                {"type": "url", "id": "b", "attributes": {"url": "https://example.com/page2"}}
            ],
            "links": {"self": "https://x/self", "next": "https://x/next"},
            "meta": {"cursor": "CURSOR"}
        }"#;

        let response: VtUrlsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].attributes.url, "https://example.com/page1");
        assert_eq!(response.data[1].attributes.url, "https://example.com/page2");
        assert_eq!(response.links.next.as_deref(), Some("https://x/next"));
    }

    #[test]
    fn test_vt_response_empty_deserialize() {
        // No data and no `next` (last/empty page) must parse cleanly.
        let json = r#"{"data": [], "links": {"self": "https://x/self"}}"#;
        let response: VtUrlsResponse = serde_json::from_str(json).unwrap();
        assert!(response.data.is_empty());
        assert!(response.links.next.is_none());

        // A bare object also parses (all fields default).
        let response: VtUrlsResponse = serde_json::from_str("{}").unwrap();
        assert!(response.data.is_empty());
        assert!(response.links.next.is_none());
    }

    #[tokio::test]
    async fn test_fetch_urls_with_empty_api_key() {
        let provider = VirusTotalProvider::new("".to_string());
        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_ok(), "Expected success with empty API key");
        let urls = result.unwrap();
        assert_eq!(urls.len(), 0, "Expected empty URLs list with empty API key");
    }

    #[tokio::test]
    async fn test_fetch_urls_with_invalid_api_key() {
        let provider = VirusTotalProvider::new("invalid_key".to_string());
        // This test should fail with an HTTP error since the API key is invalid
        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_err(), "Expected error with invalid API key");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("HTTP error")
                || err.contains("Failed after")
                || err.contains("VirusTotal")
                || err.contains("parse"),
            "Unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_with_mock() {
        let mut server = mockito::Server::new_async().await;

        // v3: domain in the path, key in the x-apikey header, urls under
        // data[].attributes.url. A single page (no links.next) ends the walk.
        let m = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_header("x-apikey", "test_api_key")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "40".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "data": [
                        {"attributes": {"url": "https://example.com/page1"}},
                        {"attributes": {"url": "https://example.com/page2"}}
                    ],
                    "links": {"self": "x"}
                }"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new("test_api_key".to_string());
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();
        assert_eq!(
            urls,
            vec![
                "https://example.com/page1".to_string(),
                "https://example.com/page2".to_string(),
            ]
        );
        m.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_paginates_via_links_next() {
        let mut server = mockito::Server::new_async().await;

        // Page one hands back a complete `links.next` URL pointing at page two.
        let next = format!(
            "{}/api/v3/domains/example.com/urls?limit=40&cursor=PAGE2",
            server.url()
        );
        let page1 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Exact("limit=40".into()))
            .with_status(200)
            .with_body(format!(
                r#"{{"data": [{{"attributes": {{"url": "https://example.com/a"}}}}], "links": {{"next": "{next}"}}}}"#
            ))
            .expect(1)
            .create_async()
            .await;
        // Page two is keyed by the cursor and carries no `next`, so the walk ends.
        let page2 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::UrlEncoded(
                "cursor".into(),
                "PAGE2".into(),
            ))
            .with_status(200)
            .with_body(
                r#"{"data": [{"attributes": {"url": "https://example.com/b"}}], "links": {}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new("test_api_key".to_string());
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();
        assert_eq!(
            urls,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ]
        );
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_keeps_partial_on_midpage_failure() {
        let mut server = mockito::Server::new_async().await;

        let next = format!(
            "{}/api/v3/domains/example.com/urls?limit=40&cursor=PAGE2",
            server.url()
        );
        let _page1 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Exact("limit=40".into()))
            .with_status(200)
            .with_body(format!(
                r#"{{"data": [{{"attributes": {{"url": "https://example.com/a"}}}}], "links": {{"next": "{next}"}}}}"#
            ))
            .create_async()
            .await;
        // The follow-up page fails: keep page one and flag the result partial.
        let _page2 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::UrlEncoded(
                "cursor".into(),
                "PAGE2".into(),
            ))
            .with_status(503)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new("test_api_key".to_string());
        provider.with_base_url(server.url());
        provider.with_retries(0); // fail fast, no back-off sleeps

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "t · ");
        let urls = provider
            .fetch_urls_with_progress("example.com", Some(reporter.clone()))
            .await
            .unwrap();
        assert_eq!(urls, vec!["https://example.com/a".to_string()]);
        assert!(reporter.is_partial());
    }

    #[tokio::test]
    async fn test_fetch_urls_404_returns_empty() {
        // A domain with no VT object answers 404; treat it as "no data", not an
        // error, matching the other providers.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .with_body(r#"{"error": {"code": "NotFoundError"}}"#)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new("test_api_key".to_string());
        provider.with_base_url(server.url());
        provider.with_retries(0);

        let urls = provider.fetch_urls("example.com").await.unwrap();
        assert!(urls.is_empty());
    }
}
