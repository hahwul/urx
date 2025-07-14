use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::Provider;

#[derive(Clone)]
pub struct UrlscanProvider {
    api_key: String,
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

impl UrlscanProvider {
    pub fn new(api_key: String) -> Self {
        UrlscanProvider {
            api_key,
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
            // Skip if no API key is provided
            if self.api_key.is_empty() {
                return Ok(Vec::new());
            }

            // Use the url crate for encoding the domain
            let encoded_domain =
                url::form_urlencoded::byte_serialize(domain.as_bytes()).collect::<String>();

            // Construct the URL - use base_url in test mode
            #[cfg(test)]
            let url = format!(
                "{}/api/v1/search/?q=domain:{}&size=100",
                self.base_url, encoded_domain
            );

            #[cfg(not(test))]
            let url =
                format!("https://urlscan.io/api/v1/search/?q=domain:{encoded_domain}&size=100");

            // Create client builder with proxy support
            let mut client_builder =
                reqwest::Client::builder().timeout(std::time::Duration::from_secs(self.timeout));

            // Skip SSL verification if insecure is enabled
            if self.insecure {
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }

            // Add random user agent if enabled
            if self.random_agent {
                let user_agents = [
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.82 Safari/537.36",
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Safari/605.1.15",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:86.0) Gecko/20100101 Firefox/86.0",
                    "Mozilla/5.0 (Linux; Android 10; SM-G973F) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Mobile Safari/537.36",
                    "Mozilla/5.0 (iPhone; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/87.0.4280.77 Mobile/15E148 Safari/604.1",
                ];
                let random_index = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as usize
                    % user_agents.len();
                client_builder = client_builder.user_agent(user_agents[random_index]);
            }

            // Add proxy if configured
            if let Some(proxy_url) = &self.proxy {
                let mut proxy = reqwest::Proxy::all(proxy_url)
                    .context(format!("Invalid proxy URL: {proxy_url}"))?;

                // Add proxy authentication if provided
                if let Some(auth) = &self.proxy_auth {
                    if let Some((username, password)) = auth.split_once(':') {
                        proxy = proxy.basic_auth(username, password);
                    }
                }

                client_builder = client_builder.proxy(proxy);
            }

            // Add API key as a header
            let headers = reqwest::header::HeaderMap::new();
            let client = client_builder
                .default_headers(headers)
                .build()
                .context("Failed to build HTTP client")?;

            // Implement retry logic
            let mut last_error = None;
            let mut attempt = 0;

            while attempt <= self.retries {
                if attempt > 0 {
                    // Wait before retrying, with increasing backoff
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                        .await;
                }

                // Create a new request with API key header
                let mut req = client.get(&url);
                if !self.api_key.is_empty() {
                    req = req.header("API-Key", &self.api_key);
                }

                match req.send().await {
                    Ok(response) => {
                        // Check if response is successful
                        if !response.status().is_success() {
                            attempt += 1;
                            last_error = Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                            continue;
                        }

                        // Parse response
                        match response.json::<UrlscanResponse>().await {
                            Ok(urlscan_response) => {
                                let mut urls = Vec::new();
                                for result in urlscan_response.results {
                                    urls.push(result.page.url);
                                }
                                return Ok(urls);
                            }
                            Err(e) => {
                                attempt += 1;
                                last_error = Some(anyhow::anyhow!(
                                    "Failed to parse Urlscan response: {}",
                                    e
                                ));
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

            // If we got here, all attempts failed
            if let Some(e) = last_error {
                Err(anyhow::anyhow!(
                    "Failed after {} attempts: {}",
                    self.retries + 1,
                    e
                ))
            } else {
                Err(anyhow::anyhow!(
                    "Failed after {} attempts",
                    self.retries + 1
                ))
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
        assert_eq!(provider.api_key, api_key);
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
