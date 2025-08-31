use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::ApiKeyRotator;
use super::Provider;

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
    parallel: u32,
    rate_limit: Option<f32>,
    #[cfg(test)]
    base_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VTUrl {
    url: String,
    // We could add scan_date parsing if needed in the future
    // scan_date: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VTResponse {
    #[serde(default)]
    detected_urls: Vec<VTUrl>,
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
            parallel: 1,
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
}

impl Provider for VirusTotalProvider {
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

            // Construct the URL - use base_url in test mode
            #[cfg(test)]
            let url = format!(
                "{}/vtapi/v2/domain/report?apikey={}&domain={}",
                self.base_url, api_key, encoded_domain
            );

            #[cfg(not(test))]
            let url = format!(
                "https://www.virustotal.com/vtapi/v2/domain/report?apikey={}&domain={}",
                api_key, encoded_domain
            );

            // Create client builder with proxy support
            let mut client_builder =
                reqwest::Client::builder().timeout(std::time::Duration::from_secs(self.timeout));

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

            // Implement retry logic
            let mut last_error = None;
            let mut attempt = 0;

            while attempt <= self.retries {
                if attempt > 0 {
                    // Wait before retrying, with increasing backoff
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                        .await;
                }

                match client.get(&url).send().await {
                    Ok(response) => {
                        // Check if response is successful
                        if !response.status().is_success() {
                            attempt += 1;
                            last_error = Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                            continue;
                        }

                        // Parse response
                        match response.json::<VTResponse>().await {
                            Ok(vt_response) => {
                                let mut urls = Vec::new();
                                for vt_url in vt_response.detected_urls {
                                    urls.push(vt_url.url);
                                }
                                return Ok(urls);
                            }
                            Err(e) => {
                                attempt += 1;
                                last_error = Some(anyhow::anyhow!(
                                    "Failed to parse VirusTotal response: {}",
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
        assert_eq!(provider.parallel, 1);
        assert_eq!(provider.rate_limit, None);
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
    fn test_with_parallel() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_parallel(10);
        assert_eq!(provider.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let provider = &mut VirusTotalProvider::new("test_api_key".to_string());
        provider.with_rate_limit(Some(2.5));
        assert_eq!(provider.rate_limit, Some(2.5));
    }

    #[test]
    fn test_clone_box() {
        let provider = VirusTotalProvider::new("test_api_key".to_string());
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[test]
    fn test_vt_response_deserialize() {
        let json = r#"{
            "detected_urls": [
                {"url": "https://example.com/page1"},
                {"url": "https://example.com/page2"}
            ]
        }"#;

        let response: VTResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.detected_urls.len(), 2);
        assert_eq!(response.detected_urls[0].url, "https://example.com/page1");
        assert_eq!(response.detected_urls[1].url, "https://example.com/page2");
    }

    #[test]
    fn test_vt_response_empty_deserialize() {
        let json = r#"{}"#;

        let response: VTResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.detected_urls.len(), 0);
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
        // Create a mock server - use new_async to avoid nested runtime issues
        let mut mock_server = mockito::Server::new_async().await;

        // Create a mock response
        let mock_response = r#"{
            "detected_urls": [
                {"url": "https://example.com/page1"},
                {"url": "https://example.com/page2"}
            ]
        }"#;

        // Setup the mock
        let _m = mock_server
            .mock("GET", "/vtapi/v2/domain/report")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("apikey".into(), "test_api_key".into()),
                mockito::Matcher::UrlEncoded("domain".into(), "example.com".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_response)
            .create();

        // Create the provider using mock server URL
        let mut provider = VirusTotalProvider::new("test_api_key".to_string());
        provider.with_base_url(mock_server.url());

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "Expected success with mock API");

        let urls = result.unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/page1");
        assert_eq!(urls[1], "https://example.com/page2");
    }
}
