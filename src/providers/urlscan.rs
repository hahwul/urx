use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::ApiKeyRotator;
use super::Provider;
use crate::network::client::HttpClientConfig;
use crate::network::RateLimiter;

#[derive(Clone)]
pub struct UrlscanProvider {
    api_key_rotator: ApiKeyRotator,
    include_subdomains: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    parallel: u32,
    rate_limit: Option<f32>,
    #[cfg(test)]
    base_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UrlscanResponse {
    status: Option<i32>,
    #[serde(default)]
    results: Vec<SearchResult>,
    #[serde(default)]
    has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    page: ArchivedPage,
    #[serde(default)]
    sort: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ArchivedPage {
    domain: String,
    #[serde(default)]
    #[serde(rename = "mimeType")]
    mime_type: String,
    url: String,
    #[serde(default)]
    status: String,
}

/// Hard ceiling on urlscan result pages walked for one domain (100 results
/// each), so a huge or misbehaving result set can't spin indefinitely.
const URLSCAN_MAX_PAGES: usize = 100;

/// Turn a result's `sort` array into the `search_after` cursor urlscan expects:
/// the array values rendered as a comma-separated string. Returns `None` when
/// the result carries no sort key (so we can't page further).
fn sort_to_search_after(sort: &[serde_json::Value]) -> Option<String> {
    if sort.is_empty() {
        return None;
    }
    let parts: Vec<String> = sort
        .iter()
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect();
    Some(parts.join(","))
}

impl UrlscanProvider {
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

        UrlscanProvider {
            api_key_rotator: ApiKeyRotator::new(filtered_keys),
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            parallel: 1,
            rate_limit: None,
            #[cfg(test)]
            base_url: "https://urlscan.io".to_string(),
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

    /// Fetch and parse a single search page with retry/back-off. Returns the
    /// parsed response or the last error after exhausting retries.
    async fn fetch_page(
        &self,
        client: &reqwest::Client,
        url: &str,
        api_key: &str,
        limiter: Option<&RateLimiter>,
    ) -> Result<UrlscanResponse> {
        let mut last_error = None;
        let mut attempt = 0;

        while attempt <= self.retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }

            let mut req = client.get(url);
            if !api_key.is_empty() {
                req = req.header("API-Key", api_key);
            }

            if let Some(rl) = limiter {
                rl.acquire().await;
            }
            match req.send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        attempt += 1;
                        last_error = Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                        continue;
                    }
                    match response.json::<UrlscanResponse>().await {
                        Ok(parsed) => return Ok(parsed),
                        Err(e) => {
                            attempt += 1;
                            last_error =
                                Some(anyhow::anyhow!("Failed to parse Urlscan response: {}", e));
                            continue;
                        }
                    }
                }
                Err(e) => {
                    attempt += 1;
                    last_error = Some(e.into());
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

impl Provider for UrlscanProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            // Skip if no API keys are provided
            if !self.api_key_rotator.has_keys() {
                return Ok(Vec::new());
            }

            // Get the next API key in rotation
            let api_key = self
                .api_key_rotator
                .next_key()
                .expect("Key rotator should have keys since has_keys() returned true");

            // Use the url crate for encoding the domain
            let encoded_domain =
                url::form_urlencoded::byte_serialize(domain.as_bytes()).collect::<String>();

            // Construct the base query - use base_url in test mode
            #[cfg(test)]
            let base_query = format!(
                "{}/api/v1/search/?q=domain:{}&size=100",
                self.base_url, encoded_domain
            );

            #[cfg(not(test))]
            let base_query =
                format!("https://urlscan.io/api/v1/search/?q=domain:{encoded_domain}&size=100");

            let client = self.client_config().build_client()?;
            let limiter = RateLimiter::from_rate(self.rate_limit);

            // urlscan returns at most 100 results per request and signals more
            // via `has_more`; the next page is requested by passing the last
            // result's `sort` values as `search_after`. The previous code never
            // paginated, silently capping every domain at 100 URLs.
            let mut all_urls = Vec::new();
            let mut search_after: Option<String> = None;
            let mut pages = 0;

            loop {
                pages += 1;
                if pages > URLSCAN_MAX_PAGES {
                    break;
                }

                let url = match &search_after {
                    Some(cursor) => format!("{base_query}&search_after={cursor}"),
                    None => base_query.clone(),
                };

                let response = match self
                    .fetch_page(&client, &url, &api_key, limiter.as_ref())
                    .await
                {
                    Ok(resp) => resp,
                    Err(e) => {
                        // A failure on the very first page is fatal; a later
                        // failure keeps the pages already collected.
                        if all_urls.is_empty() {
                            return Err(e);
                        }
                        break;
                    }
                };

                if response.results.is_empty() {
                    break;
                }

                // Capture the cursor before consuming the results.
                let next_cursor = response
                    .results
                    .last()
                    .and_then(|r| sort_to_search_after(&r.sort));

                let more = response.has_more;
                for result in response.results {
                    all_urls.push(result.page.url);
                }

                if !more {
                    break;
                }

                match next_cursor {
                    Some(cursor) => search_after = Some(cursor),
                    // No usable cursor — can't page further without risking an
                    // infinite loop re-requesting page one.
                    None => break,
                }
            }

            Ok(all_urls)
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

    #[test]
    fn test_new_provider() {
        let api_key = "test_api_key".to_string();
        let provider = UrlscanProvider::new(api_key.clone());
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
        assert_eq!(provider.parallel, 1);
        assert_eq!(provider.rate_limit, None);
    }

    #[test]
    fn test_new_provider_with_multiple_keys() {
        let api_keys = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let provider = UrlscanProvider::new_with_keys(api_keys.clone());

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
        let provider = UrlscanProvider::new_with_keys(api_keys);

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
        let provider = UrlscanProvider::new("".to_string());
        assert!(!provider.api_key_rotator.has_keys());
        assert_eq!(provider.api_key_rotator.key_count(), 0);
        assert_eq!(provider.api_key_rotator.current_key(), None);
    }

    #[test]
    fn test_with_subdomains() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_timeout(60);
        assert_eq!(provider.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_random_agent(true);
        assert!(provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_parallel() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_parallel(10);
        assert_eq!(provider.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let provider = &mut UrlscanProvider::new("test_api_key".to_string());
        provider.with_rate_limit(Some(2.5));
        assert_eq!(provider.rate_limit, Some(2.5));
    }

    #[test]
    fn test_clone_box() {
        let provider = UrlscanProvider::new("test_api_key".to_string());
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[test]
    fn test_urlscan_response_deserialize() {
        let json = r#"{
            "status": 200,
            "results": [
                {
                    "page": {
                        "domain": "example.com",
                        "mimeType": "text/html",
                        "url": "https://example.com/page1",
                        "status": "200"
                    },
                    "sort": ["2023-04-01", "example.com"]
                },
                {
                    "page": {
                        "domain": "example.com",
                        "mimeType": "application/javascript",
                        "url": "https://example.com/page2",
                        "status": "200"
                    },
                    "sort": ["2023-04-02", "example.com"]
                }
            ],
            "has_more": false
        }"#;

        let response: UrlscanResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.status, Some(200));
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].page.url, "https://example.com/page1");
        assert_eq!(response.results[1].page.url, "https://example.com/page2");
        assert_eq!(response.results[0].page.domain, "example.com");
        assert_eq!(response.results[0].page.mime_type, "text/html");
        assert!(!response.has_more);
    }

    #[test]
    fn test_urlscan_response_empty_deserialize() {
        let json = r#"{}"#;

        let response: UrlscanResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.status, None);
        assert_eq!(response.results.len(), 0);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_fetch_urls_with_empty_api_key() {
        let provider = UrlscanProvider::new("".to_string());
        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_ok(), "Expected success with empty API key");
        let urls = result.unwrap();
        assert_eq!(urls.len(), 0, "Expected empty URLs list with empty API key");
    }

    #[tokio::test]
    async fn test_fetch_urls_paginates_with_search_after() {
        let mut server = mockito::Server::new_async().await;

        // Page one signals more results and carries a sort cursor.
        let page1 = server
            .mock("GET", "/api/v1/search/")
            .match_query(mockito::Matcher::Regex("size=100$".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"status":200,"has_more":true,"results":[{"page":{"domain":"example.com","url":"https://example.com/p1","status":"200"},"sort":[1700000000000,"abc"]}]}"#,
            )
            .expect(1)
            .create_async()
            .await;
        // Page two is requested via search_after=<cursor> and ends the walk.
        let page2 = server
            .mock("GET", "/api/v1/search/")
            .match_query(mockito::Matcher::Regex("search_after=1700000000000".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"status":200,"has_more":false,"results":[{"page":{"domain":"example.com","url":"https://example.com/p2","status":"200"},"sort":[1700000000001,"def"]}]}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut provider = UrlscanProvider::new("k".to_string());
        provider.with_base_url(server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();
        assert_eq!(
            urls,
            vec![
                "https://example.com/p1".to_string(),
                "https://example.com/p2".to_string(),
            ]
        );
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_with_mock() {
        // Create a mock server - use new_async to avoid nested runtime issues
        let mut mock_server = mockito::Server::new_async().await;

        // Create a mock response
        let mock_response = r#"{
            "status": 200,
            "results": [
                {
                    "page": {
                        "domain": "example.com",
                        "mimeType": "text/html",
                        "url": "https://example.com/page1",
                        "status": "200"
                    },
                    "sort": ["2023-04-01", "example.com"]
                },
                {
                    "page": {
                        "domain": "example.com",
                        "mimeType": "application/javascript",
                        "url": "https://example.com/page2",
                        "status": "200"
                    },
                    "sort": ["2023-04-02", "example.com"]
                }
            ],
            "has_more": false
        }"#;

        // Setup the mock
        let _m = mock_server
            .mock("GET", "/api/v1/search/")
            .match_query(mockito::Matcher::Regex(
                "q=domain:example.com&size=100".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("API-Key", "test_api_key")
            .with_body(mock_response)
            .create_async()
            .await;

        // Create the provider using mock server URL
        let mut provider = UrlscanProvider::new("test_api_key".to_string());
        provider.with_base_url(mock_server.url());

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "Expected success with mock API");

        let urls = result.unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/page1");
        assert_eq!(urls[1], "https://example.com/page2");
    }
}
