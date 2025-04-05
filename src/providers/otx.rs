use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::Provider;

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
}

#[derive(Debug, Serialize, Deserialize)]
struct OTXResult {
    has_next: bool,
    actual_size: i32,
    url_list: Vec<OTXUrlEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OTXUrlEntry {
    domain: String,
    url: String,
    hostname: String,
    #[serde(default)]
    httpcode: i32,
    #[serde(default)]
    page_num: i32,
    #[serde(default)]
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
        }
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
                "https://otx.alienvault.com/api/v1/indicators/domain/{}/url_list?limit={}&page={}",
                domain, OTX_RESULTS_LIMIT, page_number
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
                "https://otx.alienvault.com/api/v1/indicators/domain/{}/url_list?limit={}&page={}",
                main_domain, OTX_RESULTS_LIMIT, page_number
            )
        } else {
            // This is a subdomain and we don't want to include other subdomains
            format!(
                "https://otx.alienvault.com/api/v1/indicators/hostname/{}/url_list?limit={}&page={}",
                domain, OTX_RESULTS_LIMIT, page_number
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
                    let user_agents = [
                        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.82 Safari/537.36",
                        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Safari/605.1.15",
                        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:86.0) Gecko/20100101 Firefox/86.0",
                        "Mozilla/5.0 (Linux; Android 10; SM-G973F) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Mobile Safari/537.36",
                        "Mozilla/5.0 (iPhone; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/87.0.4280.77 Mobile/15E148 Safari/604.1",
                    ];
                    let random_index = rand::thread_rng().gen_range(0..user_agents.len());
                    let random_agent = user_agents[random_index];
                    client_builder = client_builder.user_agent(random_agent);
                }

                // Add proxy if configured
                if let Some(proxy_url) = &self.proxy {
                    let mut proxy = reqwest::Proxy::all(proxy_url)
                        .context(format!("Invalid proxy URL: {}", proxy_url))?;

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
                                match response.json::<OTXResult>().await {
                                    Ok(otx_result) => {
                                        result = Some(otx_result);
                                        break;
                                    }
                                    Err(e) => {
                                        last_error = Some(anyhow::anyhow!(
                                            "Failed to parse OTX response: {}",
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
                "https://otx.alienvault.com/api/v1/indicators/domain/example.com/url_list?limit={}&page=1",
                OTX_RESULTS_LIMIT
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
                "https://otx.alienvault.com/api/v1/indicators/hostname/sub.example.com/url_list?limit={}&page=1",
                OTX_RESULTS_LIMIT
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
                "https://otx.alienvault.com/api/v1/indicators/domain/example.com/url_list?limit={}&page=1",
                OTX_RESULTS_LIMIT
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
                "https://otx.alienvault.com/api/v1/indicators/domain/example.com/url_list?limit={}&page=3",
                OTX_RESULTS_LIMIT
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
}
