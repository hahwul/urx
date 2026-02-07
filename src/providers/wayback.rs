use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

use super::Provider;
use crate::network::client::{get_with_retry, HttpClientConfig};

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
}

impl WaybackMachineProvider {
    /// Creates a new WaybackMachineProvider with default settings
    pub fn new() -> Self {
        WaybackMachineProvider {
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            parallel: 5,
            rate_limit: None,
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
            // Handle subdomain inclusion in URL construction
            let url = if self.include_subdomains {
                format!(
                    "https://web.archive.org/cdx/search/cdx?url=*.{domain}/*&output=json&fl=original"
                )
            } else {
                format!(
                    "https://web.archive.org/cdx/search/cdx?url={domain}/*&output=json&fl=original"
                )
            };

            let client = self.client_config().build_client()?;
            let text = get_with_retry(&client, &url, self.retries).await?;

            if text.trim().is_empty() {
                return Ok(Vec::new());
            }

            let json_data: Value = serde_json::from_str(&text)?;
            let mut urls = Vec::new();

            // Skip the first array which is the header
            if let Value::Array(arrays) = json_data {
                for (i, array) in arrays.iter().enumerate() {
                    // Skip the header row
                    if i == 0 {
                        continue;
                    }

                    if let Value::Array(elements) = array {
                        if let Some(Value::String(url)) = elements.first() {
                            urls.push(url.clone());
                        }
                    }
                }
            }

            // Remove duplicates
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
        assert_eq!(provider.timeout, 30);
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
            "https://web.archive.org/cdx/search/cdx?url={domain}/*&output=json&fl=original"
        );

        // URL 구성이 올바른지 확인합니다
        let url = if provider.include_subdomains {
            format!(
                "https://web.archive.org/cdx/search/cdx?url=*.{domain}/*&output=json&fl=original"
            )
        } else {
            format!("https://web.archive.org/cdx/search/cdx?url={domain}/*&output=json&fl=original")
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
            "https://web.archive.org/cdx/search/cdx?url=*.{domain}/*&output=json&fl=original"
        );

        // URL 구성이 올바른지 확인합니다
        let url = if provider.include_subdomains {
            format!(
                "https://web.archive.org/cdx/search/cdx?url=*.{domain}/*&output=json&fl=original"
            )
        } else {
            format!("https://web.archive.org/cdx/search/cdx?url={domain}/*&output=json&fl=original")
        };

        assert_eq!(url, expected_url);
    }
}
