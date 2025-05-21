use anyhow::Result;
use async_recursion::async_recursion;
use async_trait::async_trait;
use reqwest::Client;
use roxmltree::Document;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::providers::Provider;

#[derive(Clone)]
pub struct SitemapProvider {
    timeout: Duration,
    retries: u32,
    user_agent: Option<String>,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    insecure: bool,
}

impl SitemapProvider {
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

    #[async_recursion]
    async fn parse_sitemap(&self, client: &Client, sitemap_url: &str) -> Result<Vec<String>> {
        let resp = client.get(sitemap_url).send().await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let content = resp.text().await?;
        let mut urls = Vec::new();

        match Document::parse(&content) {
            Ok(doc) => {
                // Check if this is a sitemap index file
                let is_sitemap_index = doc.root_element().has_tag_name("sitemapindex");

                if is_sitemap_index {
                    // This is a sitemap index file, so we need to process each sitemap
                    for sitemap_node in doc.descendants().filter(|n| n.has_tag_name("sitemap")) {
                        if let Some(loc_node) =
                            sitemap_node.descendants().find(|n| n.has_tag_name("loc"))
                        {
                            if let Some(nested_sitemap_url) = loc_node.text() {
                                // Recursively fetch and parse nested sitemaps
                                // Box::pin the future to avoid infinitely sized futures
                                let nested_urls =
                                    Box::pin(self.parse_sitemap(client, nested_sitemap_url))
                                        .await?;
                                urls.extend(nested_urls);
                            }
                        }
                    }
                } else {
                    // This is a regular sitemap file
                    for url_node in doc.descendants().filter(|n| n.has_tag_name("url")) {
                        if let Some(loc_node) =
                            url_node.descendants().find(|n| n.has_tag_name("loc"))
                        {
                            if let Some(url) = loc_node.text() {
                                urls.push(url.to_string());
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // If XML parsing fails, try to handle it as a text file (some sitemaps are just lists of URLs)
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with("http") {
                        urls.push(line.to_string());
                    }
                }
            }
        }

        Ok(urls)
    }
}

#[async_trait]
impl Provider for SitemapProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let client = self.build_client()?;
            let mut urls = Vec::new();

            // Try common sitemap locations
            let sitemap_urls = vec![
                format!("https://{}/sitemap.xml", domain),
                format!("https://{}/sitemap_index.xml", domain),
                format!("https://{}/sitemap.txt", domain),
                format!("http://{}/sitemap.xml", domain),
                format!("http://{}/sitemap_index.xml", domain),
                format!("http://{}/sitemap.txt", domain),
            ];

            for sitemap_url in sitemap_urls {
                let resp = client.get(&sitemap_url).send().await;

                if let Ok(resp) = resp {
                    if resp.status().is_success() {
                        // Found a valid sitemap, parse it
                        let sitemap_urls = self.parse_sitemap(&client, &sitemap_url).await?;
                        urls.extend(sitemap_urls);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_provider() {
        let provider = SitemapProvider::new();
        assert_eq!(provider.timeout, Duration::from_secs(30));
        assert_eq!(provider.retries, 3);
        assert_eq!(provider.user_agent, None);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert!(!provider.insecure);
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = SitemapProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = SitemapProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = SitemapProvider::new();
        provider.with_timeout(60);
        assert_eq!(provider.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_with_retries() {
        let mut provider = SitemapProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = SitemapProvider::new();
        provider.with_random_agent(true);
        assert_eq!(
            provider.user_agent,
            Some("Mozilla/5.0 (compatible; URXBot/1.0)".to_string())
        );
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = SitemapProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_clone_box() {
        let provider = SitemapProvider::new();
        let _cloned = provider.clone_box();
        // Testing the existence of cloned object
    }

    #[test]
    fn test_build_client() {
        let provider = SitemapProvider::new();
        let client_result = provider.build_client();
        assert!(client_result.is_ok());
    }

    #[tokio::test]
    async fn test_sitemap_xml_parsing() {
        // Sample sitemap XML content for testing
        let sitemap_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/</loc>
    <lastmod>2023-01-01</lastmod>
    <changefreq>daily</changefreq>
    <priority>1.0</priority>
  </url>
  <url>
    <loc>https://example.com/about</loc>
    <lastmod>2023-01-02</lastmod>
    <changefreq>weekly</changefreq>
    <priority>0.8</priority>
  </url>
</urlset>"#;

        // Parse the sample sitemap
        let doc = Document::parse(sitemap_xml).unwrap();
        let mut urls = Vec::new();

        for url_node in doc.descendants().filter(|n| n.has_tag_name("url")) {
            if let Some(loc_node) = url_node.descendants().find(|n| n.has_tag_name("loc")) {
                if let Some(url) = loc_node.text() {
                    urls.push(url.to_string());
                }
            }
        }

        // Verify extracted URLs
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/".to_string()));
        assert!(urls.contains(&"https://example.com/about".to_string()));
    }

    #[tokio::test]
    async fn test_sitemap_index_parsing() {
        // Sample sitemap index XML content for testing
        let sitemap_index_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap>
    <loc>https://example.com/sitemap1.xml</loc>
    <lastmod>2023-01-01</lastmod>
  </sitemap>
  <sitemap>
    <loc>https://example.com/sitemap2.xml</loc>
    <lastmod>2023-01-02</lastmod>
  </sitemap>
</sitemapindex>"#;

        // Parse the sample sitemap index
        let doc = Document::parse(sitemap_index_xml).unwrap();
        let mut sitemap_urls = Vec::new();

        for sitemap_node in doc.descendants().filter(|n| n.has_tag_name("sitemap")) {
            if let Some(loc_node) = sitemap_node.descendants().find(|n| n.has_tag_name("loc")) {
                if let Some(url) = loc_node.text() {
                    sitemap_urls.push(url.to_string());
                }
            }
        }

        // Verify extracted sitemap URLs
        assert_eq!(sitemap_urls.len(), 2);
        assert!(sitemap_urls.contains(&"https://example.com/sitemap1.xml".to_string()));
        assert!(sitemap_urls.contains(&"https://example.com/sitemap2.xml".to_string()));
    }
}
