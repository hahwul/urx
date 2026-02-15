use anyhow::{Context, Result};

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::Provider;

// Helper function to deserialize null as default value for i32
fn deserialize_null_i32<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Clone)]
pub struct OTXProvider {
    include_subdomains: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    parallel: u32,
    rate_limit: Option<f32>,
    base_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OTXResult {
    #[serde(default)]
    has_next: bool,
    #[serde(default)]
    actual_size: i32,
    #[serde(default = "Vec::new")]
    url_list: Vec<OTXUrlEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OTXUrlEntry {
    #[serde(default = "String::new")]
    domain: String,
    #[serde(default = "String::new")]
    url: String,
    #[serde(default = "String::new")]
    hostname: String,
    #[serde(default, deserialize_with = "deserialize_null_i32")]
    httpcode: i32,
    #[serde(default, deserialize_with = "deserialize_null_i32")]
    page_num: i32,
    #[serde(default, deserialize_with = "deserialize_null_i32")]
    full_size: i32,
    #[serde(default)]
    paged: bool,
}

const OTX_RESULTS_LIMIT: u32 = 200;

impl OTXProvider {
    /// Creates a new OTXProvider with default settings
    pub fn new() -> Self {
        OTXProvider {
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            parallel: 1,
            rate_limit: None,
            base_url: "https://otx.alienvault.com".to_string(),
        }
    }

    #[cfg(test)]
    fn with_base_url(&mut self, url: String) {
        self.base_url = url;
    }

    /// Formats the OTX API URL based on the domain and page number
    ///
    /// This handles different endpoints for second-level domains and subdomains,
    /// and accounts for the include_subdomains setting.
    fn format_url(&self, domain: &str, page: u32) -> String {
        // AlienVault OTX API pages start at 1, not 0
        let page_number = page + 1;

        // We should always use domain endpoint for second-level domains like example.com
        // and hostname endpoint for subdomains like sub.example.com
        if domain.split('.').count() <= 2 {
            // This is a second-level domain like example.com
            format!(
                "{}/api/v1/indicators/domain/{domain}/url_list?limit={OTX_RESULTS_LIMIT}&page={page_number}",
                self.base_url
            )
        } else if self.include_subdomains {
            // This is a subdomain but we want to include all subdomains
            // Extract the main domain (e.g., "example.com" from "sub.example.com")
            let parts: Vec<&str> = domain.split('.').collect();
            let main_domain = if parts.len() >= 2 {
                parts[parts.len() - 2..].join(".")
            } else {
                domain.to_string()
            };

            format!(
                "{}/api/v1/indicators/domain/{main_domain}/url_list?limit={OTX_RESULTS_LIMIT}&page={page_number}",
                self.base_url
            )
        } else {
            // This is a subdomain and we don't want to include other subdomains
            format!(
                "{}/api/v1/indicators/hostname/{domain}/url_list?limit={OTX_RESULTS_LIMIT}&page={page_number}",
                self.base_url
            )
        }
    }
}

impl Provider for OTXProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let mut all_urls = Vec::new();
            let mut page = 0;

            loop {
                let url = self.format_url(domain, page);

                // Create client builder with proxy support
                let mut client_builder = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(self.timeout));

                // Skip SSL verification if insecure is enabled
                if self.insecure {
                    client_builder = client_builder.danger_accept_invalid_certs(true);
                }

                // Add random user agent if enabled
                if self.random_agent {
                    let ua = crate::network::random_user_agent();
                    client_builder = client_builder.user_agent(ua);
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

                let client = client_builder
                    .build()
                    .context("Failed to build HTTP client")?;

                // Retry logic
                let mut last_error = None;
                let mut result = None;

                for attempt in 0..=self.retries {
                    match client.get(&url).send().await {
                        Ok(response) => {
                            if response.status().is_success() {
                                match response.text().await {
                                    Ok(text) => {
                                        // Try to parse as OTXResult first
                                        let parse_result = serde_json::from_str::<OTXResult>(&text);

                                        if let Ok(otx_result) = parse_result {
                                            result = Some(otx_result);
                                            break;
                                        } else {
                                            // If that fails, try to parse as a JSON Value and extract the url_list
                                            match serde_json::from_str::<serde_json::Value>(&text) {
                                                Ok(json_value) => {
                                                    if let Some(url_list) =
                                                        json_value.get("url_list")
                                                    {
                                                        match serde_json::from_value::<
                                                            Vec<OTXUrlEntry>,
                                                        >(
                                                            url_list.clone()
                                                        ) {
                                                            Ok(entries) => {
                                                                // Create a new OTXResult with default values for other fields
                                                                let otx_result = OTXResult {
                                                                    has_next: json_value
                                                                        .get("has_next")
                                                                        .and_then(|v| v.as_bool())
                                                                        .unwrap_or(false),
                                                                    actual_size: json_value
                                                                        .get("actual_size")
                                                                        .and_then(|v| v.as_i64())
                                                                        .map(|v| v as i32)
                                                                        .unwrap_or(0),
                                                                    url_list: entries,
                                                                };
                                                                result = Some(otx_result);
                                                                break;
                                                            }
                                                            Err(e) => {
                                                                let preview = if text.len() > 100 {
                                                                    format!(
                                                                        "{}... (truncated)",
                                                                        &text[..100]
                                                                    )
                                                                } else {
                                                                    text.clone()
                                                                };

                                                                last_error = Some(anyhow::anyhow!(
                                                                    "Failed to parse url_list entries: {}. Response preview: {}",
                                                                    e, preview
                                                                ));
                                                            }
                                                        }
                                                    } else {
                                                        let preview = if text.len() > 100 {
                                                            format!(
                                                                "{}... (truncated)",
                                                                &text[..100]
                                                            )
                                                        } else {
                                                            text.clone()
                                                        };

                                                        last_error = Some(anyhow::anyhow!(
                                                            "Response is missing url_list field. Response preview: {}",
                                                            preview
                                                        ));
                                                    }
                                                }
                                                Err(e) => {
                                                    let preview = if text.len() > 100 {
                                                        format!("{}... (truncated)", &text[..100])
                                                    } else {
                                                        text.clone()
                                                    };

                                                    last_error = Some(anyhow::anyhow!(
                                                        "Failed to parse OTX response as JSON: {}. Response preview: {}",
                                                        e, preview
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        last_error = Some(anyhow::anyhow!(
                                            "Failed to get response text: {}",
                                            e
                                        ));
                                    }
                                }
                            } else {
                                last_error =
                                    Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                            }
                        }
                        Err(e) => {
                            last_error = Some(anyhow::anyhow!("Request error: {}", e));
                        }
                    }

                    if result.is_some() {
                        break;
                    }

                    if attempt < self.retries {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }

                if let Some(otx_result) = result {
                    all_urls.extend(otx_result.url_list.into_iter().map(|entry| entry.url));

                    // Check for next page
                    if !otx_result.has_next {
                        break;
                    }
                } else {
                    // If we couldn't get a result after all retries, return the error
                    return Err(last_error.unwrap_or_else(|| {
                        anyhow::anyhow!("Failed to fetch OTX data after all retries")
                    }));
                }

                page += 1;
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
        let provider = OTXProvider::new();
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
        let mut provider = OTXProvider::new();
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = OTXProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = OTXProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = OTXProvider::new();
        provider.with_timeout(60);
        assert_eq!(provider.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let mut provider = OTXProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = OTXProvider::new();
        provider.with_random_agent(true);
        assert!(provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = OTXProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_parallel() {
        let mut provider = OTXProvider::new();
        provider.with_parallel(10);
        assert_eq!(provider.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let mut provider = OTXProvider::new();
        provider.with_rate_limit(Some(2.5));
        assert_eq!(provider.rate_limit, Some(2.5));
    }

    #[test]
    fn test_clone_box() {
        let provider = OTXProvider::new();
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[test]
    fn test_format_url_second_level_domain() {
        let provider = OTXProvider::new();
        let url = provider.format_url("example.com", 0);
        assert_eq!(
            url,
            format!(
                "https://otx.alienvault.com/api/v1/indicators/domain/example.com/url_list?limit={OTX_RESULTS_LIMIT}&page=1"
            )
        );
    }

    #[test]
    fn test_format_url_subdomain_without_include_subdomains() {
        let provider = OTXProvider::new();
        let url = provider.format_url("sub.example.com", 0);
        assert_eq!(
            url,
            format!(
                "https://otx.alienvault.com/api/v1/indicators/hostname/sub.example.com/url_list?limit={OTX_RESULTS_LIMIT}&page=1"
            )
        );
    }

    #[test]
    fn test_format_url_subdomain_with_include_subdomains() {
        let mut provider = OTXProvider::new();
        provider.with_subdomains(true);
        let url = provider.format_url("sub.example.com", 0);
        assert_eq!(
            url,
            format!(
                "https://otx.alienvault.com/api/v1/indicators/domain/example.com/url_list?limit={OTX_RESULTS_LIMIT}&page=1"
            )
        );
    }

    #[test]
    fn test_format_url_with_pagination() {
        let provider = OTXProvider::new();
        let url = provider.format_url("example.com", 2);
        assert_eq!(
            url,
            format!(
                "https://otx.alienvault.com/api/v1/indicators/domain/example.com/url_list?limit={OTX_RESULTS_LIMIT}&page=3"
            )
        );
    }

    #[test]
    fn test_otx_result_deserialize() {
        let json = r#"{
            "has_next": true,
            "actual_size": 200,
            "url_list": [
                {
                    "domain": "example.com",
                    "url": "https://example.com/page1",
                    "hostname": "example.com",
                    "httpcode": 200,
                    "page_num": 1,
                    "full_size": 500,
                    "paged": true
                },
                {
                    "domain": "example.com",
                    "url": "https://example.com/page2",
                    "hostname": "example.com"
                }
            ]
        }"#;

        let result: OTXResult = serde_json::from_str(json).unwrap();
        assert!(result.has_next);
        assert_eq!(result.actual_size, 200);
        assert_eq!(result.url_list.len(), 2);
        assert_eq!(result.url_list[0].url, "https://example.com/page1");
        assert_eq!(result.url_list[0].httpcode, 200);
        assert_eq!(result.url_list[1].url, "https://example.com/page2");
        assert_eq!(result.url_list[1].httpcode, 0); // default value for missing field
    }

    #[tokio::test]
    async fn test_fetch_urls_with_nonexistent_domain() {
        let provider = OTXProvider::new();
        let domain = "test-domain-that-does-not-exist-xyz.example";

        let result = provider.fetch_urls(domain).await;
        assert!(
            result.is_err(),
            "Expected an error when fetching from non-existent domain"
        );

        // Error message should contain domain or connection-related error
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to fetch OTX data")
                || err.contains("timed out")
                || err.contains("HTTP error")
                || err.contains("connection")
                || err.contains("network"),
            "Unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_pagination() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        // Mock page 1 response
        let _m1 = server
            .mock(
                "GET",
                "/api/v1/indicators/domain/example.com/url_list?limit=200&page=1",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "has_next": true,
                "url_list": [
                    { "url": "http://example.com/1" }
                ]
            }"#,
            )
            .create();

        // Mock page 2 response
        let _m2 = server
            .mock(
                "GET",
                "/api/v1/indicators/domain/example.com/url_list?limit=200&page=2",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "has_next": false,
                "url_list": [
                    { "url": "http://example.com/2" }
                ]
            }"#,
            )
            .create();

        let mut provider = OTXProvider::new();
        provider.with_base_url(url);

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "Failed to fetch URLs: {:?}", result.err());
        let urls = result.unwrap();

        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"http://example.com/1".to_string()));
        assert!(urls.contains(&"http://example.com/2".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_urls_empty() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m1 = server
            .mock(
                "GET",
                "/api/v1/indicators/domain/example.com/url_list?limit=200&page=1",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "has_next": false,
                "url_list": []
            }"#,
            )
            .create();

        let mut provider = OTXProvider::new();
        provider.with_base_url(url);

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok());
        let urls = result.unwrap();

        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_urls_json_error() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        // Respond with malformed JSON
        let _m1 = server
            .mock(
                "GET",
                "/api/v1/indicators/domain/example.com/url_list?limit=200&page=1",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{ "invalid_json": true "#)
            .create();

        let mut provider = OTXProvider::new();
        provider.with_base_url(url);
        // Reduce retries to speed up test
        provider.with_retries(0);

        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // The implementation wraps the error, check for "Failed to parse"
        assert!(err.contains("Failed to parse") || err.contains("Failed to fetch OTX data"));
    }

    #[tokio::test]
    async fn test_fetch_urls_http_error() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m1 = server
            .mock(
                "GET",
                "/api/v1/indicators/domain/example.com/url_list?limit=200&page=1",
            )
            .with_status(500)
            .create();

        let mut provider = OTXProvider::new();
        provider.with_base_url(url);
        provider.with_retries(0);

        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // Check for HTTP error message
        assert!(err.contains("HTTP error") || err.contains("Failed to fetch OTX data"));
    }
}
