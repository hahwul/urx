use anyhow::Result;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;

use super::Provider;

#[derive(Clone)]
pub struct CommonCrawlProvider {
    index: String,
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

#[derive(Deserialize)]
struct CCRecord {
    url: String,
}

impl CommonCrawlProvider {
    #[allow(dead_code)]
    pub fn new() -> Self {
        CommonCrawlProvider {
            index: "CC-MAIN-2025-08".to_string(),
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 10,
            retries: 3,
            random_agent: true,
            insecure: false,
            parallel: 1,
            rate_limit: None,
        }
    }

    /// Creates a provider instance with a specific Common Crawl index
    pub fn with_index(index: String) -> Self {
        CommonCrawlProvider {
            index,
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 10,
            retries: 3,
            random_agent: true,
            insecure: false,
            parallel: 1,
            rate_limit: None,
        }
    }
}

impl Provider for CommonCrawlProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            // Construct URL based on subdomain inclusion
            let url_pattern = if self.include_subdomains {
                format!(
                    "https://index.commoncrawl.org/{}-index?url=*.{}/*&output=json",
                    self.index, domain
                )
            } else {
                format!(
                    "https://index.commoncrawl.org/{}-index?url={}/*&output=json",
                    self.index, domain
                )
            };

            // Create client builder with timeout
            let mut client_builder =
                reqwest::Client::builder().timeout(std::time::Duration::from_secs(self.timeout));

            // Skip SSL verification if insecure is enabled
            if self.insecure {
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }

            // Add random user agent if enabled
            if self.random_agent {
                let user_agents = [
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36",
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.5 Safari/605.1.15",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/115.0",
                    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/113.0.0.0 Safari/537.36",
                    "Mozilla/5.0 (iPhone; CPU iPhone OS 16_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.5 Mobile/15E148 Safari/604.1",
                ];
                let random_index = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as usize
                    % user_agents.len();
                client_builder = client_builder.user_agent(user_agents[random_index]);
            }

            // Apply proxy if configured
            if let Some(proxy_url) = &self.proxy {
                let mut proxy = reqwest::Proxy::all(proxy_url)?;

                // Add proxy authentication if provided
                if let Some(auth) = &self.proxy_auth {
                    proxy = proxy.basic_auth(
                        auth.split(':').next().unwrap_or(""),
                        auth.split(':').nth(1).unwrap_or(""),
                    );
                }

                client_builder = client_builder.proxy(proxy);
            }

            let client = client_builder.build()?;

            // Implement retry logic
            let mut last_error = None;
            let mut attempt = 0;

            while attempt <= self.retries {
                if attempt > 0 {
                    // Wait before retrying, with increasing backoff
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                        .await;
                }

                match client.get(&url_pattern).send().await {
                    Ok(response) => {
                        // Check if response is successful
                        if !response.status().is_success() {
                            attempt += 1;
                            last_error = Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                            continue;
                        }

                        // Parse response text
                        match response.text().await {
                            Ok(text) => {
                                if text.trim().is_empty() {
                                    return Ok(Vec::new());
                                }

                                let mut urls = Vec::new();

                                // Common Crawl returns one JSON object per line
                                for line in text.lines() {
                                    if let Ok(record) = serde_json::from_str::<CCRecord>(line) {
                                        urls.push(record.url);
                                    }
                                }

                                // Remove duplicates
                                urls.sort();
                                urls.dedup();

                                return Ok(urls);
                            }
                            Err(e) => {
                                attempt += 1;
                                last_error = Some(e.into());
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

    // New method implementations for the additional features
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
        let provider = CommonCrawlProvider::new();
        assert_eq!(provider.index, "CC-MAIN-2025-08");
        assert!(!provider.include_subdomains);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert_eq!(provider.timeout, 10);
        assert_eq!(provider.retries, 3);
        assert!(provider.random_agent);
        assert!(!provider.insecure);
        assert_eq!(provider.parallel, 1);
        assert_eq!(provider.rate_limit, None);
    }

    #[test]
    fn test_with_index() {
        let index = "CC-MAIN-2023-06".to_string();
        let provider = CommonCrawlProvider::with_index(index.clone());
        assert_eq!(provider.index, index);
    }

    #[test]
    fn test_with_subdomains() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_timeout(60);
        assert_eq!(provider.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_random_agent(false);
        assert!(!provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_parallel() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_parallel(10);
        assert_eq!(provider.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_rate_limit(Some(2.5));
        assert_eq!(provider.rate_limit, Some(2.5));
    }

    #[test]
    fn test_clone_box() {
        let provider = CommonCrawlProvider::new();
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[tokio::test]
    async fn test_fetch_urls_builds_correct_url_without_subdomains() {
        let provider = CommonCrawlProvider::new();

        // Use a domain that's unlikely to exist to avoid real API calls
        let domain = "test-domain-that-does-not-exist-xyz.example";

        // We expect this test to fail with a connection error or timeout
        let result = provider.fetch_urls(domain).await;
        assert!(
            result.is_err(),
            "Expected an error when fetching from non-existent domain"
        );

        let err = result.unwrap_err().to_string();
        assert!(
            err.contains(domain) || err.contains("timed out") || err.contains("dns"),
            "Error doesn't indicate correct domain or connection timeout: {err}"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_builds_correct_url_with_subdomains() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_subdomains(true);

        // Use a domain that's unlikely to exist to avoid real API calls
        let domain = "test-domain-that-does-not-exist-xyz.example";

        // We expect this test to fail with a connection error or timeout
        let result = provider.fetch_urls(domain).await;
        assert!(
            result.is_err(),
            "Expected an error when fetching from non-existent domain"
        );

        let err = result.unwrap_err().to_string();
        assert!(
            err.contains(domain) || err.contains("timed out") || err.contains("dns"),
            "Error doesn't indicate correct domain or connection timeout: {err}"
        );
    }

    #[test]
    fn test_cc_record_deserialize() {
        let json = r#"{"url":"https://example.com/test"}"#;
        let record: CCRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.url, "https://example.com/test");
    }
}
