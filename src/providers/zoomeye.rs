use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::ApiKeyRotator;
use super::Provider;
use crate::network::client::HttpClientConfig;

#[derive(Clone)]
pub struct ZoomEyeProvider {
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
struct ZoomEyeResponse {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    data: Vec<ZoomEyeEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ZoomEyeEntry {
    #[serde(default)]
    url: String,
    #[serde(default)]
    ip: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Serialize)]
struct ZoomEyeRequest {
    qbase64: String,
    page: u32,
    pagesize: u32,
    sub_type: String,
}

impl ZoomEyeProvider {
    #[allow(dead_code)]
    pub fn new(api_key: String) -> Self {
        if api_key.is_empty() {
            Self::new_with_keys(vec![])
        } else {
            Self::new_with_keys(vec![api_key])
        }
    }

    pub fn new_with_keys(api_keys: Vec<String>) -> Self {
        let filtered_keys: Vec<String> = api_keys.into_iter().filter(|k| !k.is_empty()).collect();

        ZoomEyeProvider {
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
            base_url: "https://api.zoomeye.ai".to_string(),
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

    fn build_dork(&self, domain: &str) -> String {
        if self.include_subdomains {
            format!("site:*.{domain}")
        } else {
            format!("site:{domain}")
        }
    }
}

impl Provider for ZoomEyeProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            if !self.api_key_rotator.has_keys() {
                return Ok(Vec::new());
            }

            let api_key = self
                .api_key_rotator
                .next_key()
                .expect("Key rotator should have keys since has_keys() returned true");

            let dork = self.build_dork(domain);
            let qbase64 = STANDARD.encode(dork.as_bytes());

            #[cfg(test)]
            let api_url = format!("{}/v2/search", self.base_url);

            #[cfg(not(test))]
            let api_url = "https://api.zoomeye.ai/v2/search".to_string();

            let client = self.client_config().build_client()?;

            let mut all_urls: Vec<String> = Vec::new();
            let mut page: u32 = 1;
            let pagesize: u32 = 100;

            loop {
                let request_body = ZoomEyeRequest {
                    qbase64: qbase64.clone(),
                    page,
                    pagesize,
                    sub_type: "web".to_string(),
                };

                let mut last_error = None;
                let mut attempt = 0;
                let mut page_urls: Vec<String> = Vec::new();
                let mut total: u64 = 0;

                while attempt <= self.retries {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                            .await;
                    }

                    let req = client
                        .post(&api_url)
                        .header("API-KEY", &api_key)
                        .json(&request_body);

                    match req.send().await {
                        Ok(response) => {
                            if !response.status().is_success() {
                                attempt += 1;
                                last_error =
                                    Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                                continue;
                            }

                            match response.json::<ZoomEyeResponse>().await {
                                Ok(zoomeye_response) => {
                                    total = zoomeye_response.total;
                                    for entry in zoomeye_response.data {
                                        if !entry.url.is_empty() {
                                            page_urls.push(entry.url);
                                        }
                                    }
                                    last_error = None;
                                    break;
                                }
                                Err(e) => {
                                    attempt += 1;
                                    last_error = Some(anyhow::anyhow!(
                                        "Failed to parse ZoomEye response: {}",
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

                if let Some(e) = last_error {
                    return Err(anyhow::anyhow!(
                        "Failed after {} attempts: {}",
                        self.retries + 1,
                        e
                    ));
                }

                all_urls.extend(page_urls);

                // Check if there are more pages
                let fetched_so_far = (page as u64) * (pagesize as u64);
                if fetched_so_far >= total {
                    break;
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
        let api_key = "test_api_key".to_string();
        let provider = ZoomEyeProvider::new(api_key.clone());
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
        let provider = ZoomEyeProvider::new_with_keys(api_keys);

        assert!(provider.api_key_rotator.has_keys());
        assert_eq!(provider.api_key_rotator.key_count(), 3);

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
        );
    }

    #[test]
    fn test_new_provider_filters_empty_keys() {
        let api_keys = vec![
            "key1".to_string(),
            "".to_string(),
            "key2".to_string(),
            "".to_string(),
        ];
        let provider = ZoomEyeProvider::new_with_keys(api_keys);

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
        let provider = ZoomEyeProvider::new("".to_string());
        assert!(!provider.api_key_rotator.has_keys());
        assert_eq!(provider.api_key_rotator.key_count(), 0);
        assert_eq!(provider.api_key_rotator.current_key(), None);
    }

    #[test]
    fn test_build_dork() {
        let provider = ZoomEyeProvider::new("key".to_string());
        assert_eq!(provider.build_dork("example.com"), "site:example.com");
    }

    #[test]
    fn test_build_dork_with_subdomains() {
        let mut provider = ZoomEyeProvider::new("key".to_string());
        provider.with_subdomains(true);
        assert_eq!(provider.build_dork("example.com"), "site:*.example.com");
    }

    #[test]
    fn test_with_subdomains() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_timeout(60);
        assert_eq!(provider.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_random_agent(true);
        assert!(provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_parallel() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_parallel(10);
        assert_eq!(provider.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let provider = &mut ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_rate_limit(Some(2.5));
        assert_eq!(provider.rate_limit, Some(2.5));
    }

    #[test]
    fn test_clone_box() {
        let provider = ZoomEyeProvider::new("test_api_key".to_string());
        let _cloned = provider.clone_box();
    }

    #[test]
    fn test_zoomeye_response_deserialize() {
        let json = r#"{
            "code": 60000,
            "total": 2,
            "data": [
                {
                    "url": "https://example.com/page1",
                    "ip": "1.2.3.4",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Example Page 1"
                },
                {
                    "url": "https://example.com/page2",
                    "ip": "1.2.3.5",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Example Page 2"
                }
            ]
        }"#;

        let response: ZoomEyeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.code, 60000);
        assert_eq!(response.total, 2);
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].url, "https://example.com/page1");
        assert_eq!(response.data[1].url, "https://example.com/page2");
        assert_eq!(response.data[0].domain, "example.com");
        assert_eq!(response.data[0].port, 443);
    }

    #[test]
    fn test_zoomeye_response_empty_deserialize() {
        let json = r#"{"code": 60000, "total": 0, "data": []}"#;

        let response: ZoomEyeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.code, 60000);
        assert_eq!(response.total, 0);
        assert_eq!(response.data.len(), 0);
    }

    #[test]
    fn test_qbase64_encoding() {
        let dork = "site:example.com";
        let encoded = STANDARD.encode(dork.as_bytes());
        assert_eq!(encoded, "c2l0ZTpleGFtcGxlLmNvbQ==");
    }

    #[tokio::test]
    async fn test_fetch_urls_with_empty_api_key() {
        let provider = ZoomEyeProvider::new("".to_string());
        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_ok(), "Expected success with empty API key");
        let urls = result.unwrap();
        assert_eq!(urls.len(), 0, "Expected empty URLs list with empty API key");
    }

    #[tokio::test]
    async fn test_fetch_urls_with_mock() {
        let mut mock_server = mockito::Server::new_async().await;

        let mock_response = r#"{
            "code": 60000,
            "total": 2,
            "data": [
                {
                    "url": "https://example.com/page1",
                    "ip": "1.2.3.4",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 1"
                },
                {
                    "url": "https://example.com/page2",
                    "ip": "1.2.3.5",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 2"
                }
            ]
        }"#;

        let _m = mock_server
            .mock("POST", "/v2/search")
            .match_header("API-KEY", "test_api_key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_response)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_base_url(mock_server.url());

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "Expected success with mock API");

        let urls = result.unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/page1");
        assert_eq!(urls[1], "https://example.com/page2");
    }

    #[tokio::test]
    async fn test_fetch_urls_skips_empty_urls() {
        let mut mock_server = mockito::Server::new_async().await;

        let mock_response = r#"{
            "code": 60000,
            "total": 3,
            "data": [
                {
                    "url": "https://example.com/page1",
                    "ip": "1.2.3.4",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 1"
                },
                {
                    "url": "",
                    "ip": "1.2.3.5",
                    "domain": "example.com",
                    "port": 80,
                    "title": ""
                },
                {
                    "url": "https://example.com/page3",
                    "ip": "1.2.3.6",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 3"
                }
            ]
        }"#;

        let _m = mock_server
            .mock("POST", "/v2/search")
            .match_header("API-KEY", "test_api_key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_response)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new("test_api_key".to_string());
        provider.with_base_url(mock_server.url());

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok());

        let urls = result.unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/page1");
        assert_eq!(urls[1], "https://example.com/page3");
    }
}
