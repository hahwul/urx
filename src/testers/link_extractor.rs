use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use rand::Rng;
use scraper::{Html, Selector};
use url::Url;

use super::Tester;

#[derive(Clone)]
pub struct LinkExtractor {
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
}

impl LinkExtractor {
    pub fn new() -> Self {
        LinkExtractor {
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
        }
    }
}

impl Tester for LinkExtractor {
    fn clone_box(&self) -> Box<dyn Tester> {
        Box::new(self.clone())
    }

    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            // Create client builder with proxy support
            let mut client_builder = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(self.timeout));
            
            // Add random user agent if enabled
            if self.random_agent {
                let user_agents = [
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.1.1 Safari/605.1.15",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:89.0) Gecko/20100101 Firefox/89.0",
                    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.114 Safari/537.36",
                    "Mozilla/5.0 (iPhone; CPU iPhone OS 14_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1",
                ];
                
                let mut rng = rand::thread_rng();
                let agent = user_agents[rng.gen_range(0..user_agents.len())];
                client_builder = client_builder.user_agent(agent);
            }
            
            // Add proxy if configured
            if let Some(proxy_url) = &self.proxy {
                let mut proxy = reqwest::Proxy::all(proxy_url)?;
                
                // Add proxy authentication if provided
                if let Some(auth) = &self.proxy_auth {
                    let parts: Vec<&str> = auth.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        proxy = proxy.basic_auth(parts[0], parts[1]);
                    }
                }
                
                client_builder = client_builder.proxy(proxy);
            }
            
            let client = client_builder.build()?;
            
            // Perform the request with retries
            let mut last_error = None;
            let mut links = Vec::new();
            
            for _ in 0..=self.retries {
                match client.get(url).send().await {
                    Ok(response) => {
                        // Get the base URL for resolving relative URLs
                        let base_url = match Url::parse(url) {
                            Ok(parsed_url) => parsed_url,
                            Err(_) => {
                                return Err(anyhow::anyhow!("Failed to parse URL: {}", url));
                            }
                        };
                        
                        // Get the HTML content
                        let html_content = response.text().await?;
                        let document = Html::parse_document(&html_content);
                        
                        // Select all <a> tags with href attributes
                        let selector = Selector::parse("a[href]").unwrap();
                        
                        // Extract and normalize links
                        for element in document.select(&selector) {
                            if let Some(href) = element.value().attr("href") {
                                // Resolve relative URLs to absolute URLs
                                if let Ok(absolute_url) = base_url.join(href) {
                                    links.push(absolute_url.to_string());
                                }
                            }
                        }
                        
                        // Return the list of links
                        return Ok(links);
                    },
                    Err(e) => {
                        last_error = Some(e);
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                }
            }
            
            // If we get here, all retries failed
            Err(anyhow::anyhow!("Failed to extract links from {}: {:?}", url, last_error))
        })
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
    
    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }
    
    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }
}