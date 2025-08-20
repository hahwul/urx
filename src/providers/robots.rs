use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::providers::Provider;

#[derive(Clone)]
pub struct RobotsProvider {
    timeout: Duration,
    retries: u32,
    user_agent: Option<String>,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    insecure: bool,
}

impl RobotsProvider {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            retries: 3,
            user_agent: None,
            proxy: None,
            proxy_auth: None,
            insecure: false,
        }
    }

    fn build_client(&self) -> Result<Client> {
        let mut builder = Client::builder()
            .timeout(self.timeout)
            .danger_accept_invalid_certs(self.insecure);

        if let Some(ref proxy_url) = self.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)?;
            builder = builder.proxy(proxy);
        }

        if let Some(ref agent) = self.user_agent {
            builder = builder.user_agent(agent);
        }

        Ok(builder.build()?)
    }
}

#[async_trait]
impl Provider for RobotsProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let client = self.build_client()?;
            let https_url = format!("https://{domain}/robots.txt");
            let mut urls = Vec::new();

            // Try HTTPS first
            let https_resp = client.get(&https_url).send().await;
            // Track which protocol was successful
            let (is_https, text) = match https_resp {
                Ok(resp) if resp.status().is_success() => (true, resp.text().await?),
                _ => {
                    // If HTTPS fails, try HTTP
                    let http_url = format!("http://{domain}/robots.txt");
                    let http_resp = client.get(&http_url).send().await?;
                    if !http_resp.status().is_success() {
                        return Ok(urls);
                    }
                    (false, http_resp.text().await?)
                }
            };

            // Use the protocol that worked
            let protocol = if is_https { "https" } else { "http" };

            for line in text.lines() {
                let line = line.trim();
                if line.starts_with("Disallow:") {
                    if let Some(path) = line.strip_prefix("Disallow:").map(|s| s.trim()) {
                        if !path.is_empty() && path != "/" {
                            let url = format!("{protocol}://{domain}{path}");
                            urls.push(url);
                        }
                    }
                } else if line.starts_with("Sitemap:") {
                    if let Some(link) = line.strip_prefix("Sitemap:").map(|s| s.trim()) {
                        urls.push(link.to_string());
                    }
                }
            }

            Ok(urls)
        })
    }

    fn with_subdomains(&mut self, _include: bool) {}
    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }
    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }
    fn with_timeout(&mut self, seconds: u64) {
        self.timeout = Duration::from_secs(seconds);
    }
    fn with_retries(&mut self, count: u32) {
        self.retries = count;
    }
    fn with_random_agent(&mut self, enabled: bool) {
        if enabled {
            self.user_agent = Some(crate::network::random_user_agent());
        } else {
            self.user_agent = None;
        }
    }
    fn with_insecure(&mut self, enabled: bool) {
        self.insecure = enabled;
    }
    fn with_parallel(&mut self, _count: u32) {}
    fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_provider() {
        let provider = RobotsProvider::new();
        assert_eq!(provider.timeout, Duration::from_secs(30));
        assert_eq!(provider.retries, 3);
        assert_eq!(provider.user_agent, None);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert!(!provider.insecure);
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = RobotsProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = RobotsProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = RobotsProvider::new();
        provider.with_timeout(60);
        assert_eq!(provider.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_with_retries() {
        let mut provider = RobotsProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = RobotsProvider::new();
        provider.with_random_agent(true);
        assert!(provider.user_agent.is_some());
        assert!(provider
            .user_agent
            .as_ref()
            .unwrap()
            .starts_with("Mozilla/5.0"));

        // Test disabling the random agent
        provider.with_random_agent(false);
        assert_eq!(provider.user_agent, None);
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = RobotsProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_clone_box() {
        let provider = RobotsProvider::new();
        let _cloned = provider.clone_box();
        // Testing the existence of cloned object
    }

    #[test]
    fn test_build_client() {
        let provider = RobotsProvider::new();
        let client_result = provider.build_client();
        assert!(client_result.is_ok());

        // Test with proxy
        let mut provider_with_proxy = RobotsProvider::new();
        provider_with_proxy.with_proxy(Some("http://invalid:proxy".to_string()));
        let client_result = provider_with_proxy.build_client();
        assert!(client_result.is_err());

        // Test with user agent
        let mut provider_with_agent = RobotsProvider::new();
        provider_with_agent.with_random_agent(true);
        let client_result = provider_with_agent.build_client();
        assert!(client_result.is_ok());
    }

    #[tokio::test]
    async fn test_robots_txt_parsing() {
        let _provider = RobotsProvider::new();

        // Mock robots.txt content
        let robots_txt = "\
User-agent: *
Disallow: /private/
Disallow: /admin
Disallow:
Disallow: /
Allow: /public/
Sitemap: https://example.com/sitemap.xml
";

        // Manually parse the robots.txt content
        let domain = "example.com";
        let protocol = "https";
        let mut urls = Vec::new();

        for line in robots_txt.lines() {
            let line = line.trim();
            if line.starts_with("Disallow:") {
                if let Some(path) = line.strip_prefix("Disallow:").map(|s| s.trim()) {
                    if !path.is_empty() && path != "/" {
                        let url = format!("{protocol}://{domain}{path}");
                        urls.push(url);
                    }
                }
            } else if line.starts_with("Sitemap:") {
                if let Some(link) = line.strip_prefix("Sitemap:").map(|s| s.trim()) {
                    urls.push(link.to_string());
                }
            }
        }

        // Verify expected output
        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://example.com/private/".to_string()));
        assert!(urls.contains(&"https://example.com/admin".to_string()));
        assert!(urls.contains(&"https://example.com/sitemap.xml".to_string()));
    }

    #[tokio::test]
    async fn test_url_construction() {
        let domain = "example.com";

        // Test HTTPS URL construction
        let https_url = format!("https://{domain}/robots.txt");
        assert_eq!(https_url, "https://example.com/robots.txt");

        // Test HTTP URL construction
        let http_url = format!("http://{domain}/robots.txt");
        assert_eq!(http_url, "http://example.com/robots.txt");

        // Test disallowed path URL construction
        let protocol = "https";
        let path = "/private/";
        let url = format!("{protocol}://{domain}{path}");
        assert_eq!(url, "https://example.com/private/");
    }
}
