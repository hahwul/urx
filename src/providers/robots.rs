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
            let https_url = format!("https://{}/robots.txt", domain);
            let mut urls = Vec::new();

            // Try HTTPS first
            let https_resp = client.get(&https_url).send().await;
            // Track which protocol was successful
            let (is_https, text) = match https_resp {
                Ok(resp) if resp.status().is_success() => (true, resp.text().await?),
                _ => {
                    // If HTTPS fails, try HTTP
                    let http_url = format!("http://{}/robots.txt", domain);
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
                            let url = format!("{}://{}{}", protocol, domain, path);
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
            self.user_agent = Some("Mozilla/5.0 (compatible; URXBot/1.0)".to_string());
        }
    }
    fn with_insecure(&mut self, enabled: bool) {
        self.insecure = enabled;
    }
    fn with_parallel(&mut self, _count: u32) {}
    fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
}
