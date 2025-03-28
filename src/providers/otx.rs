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
    httpcode: i32,
    page_num: i32,
    full_size: i32,
    paged: bool,
}

const OTX_RESULTS_LIMIT: u32 = 200;

impl OTXProvider {
    pub fn new() -> Self {
        OTXProvider {
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            parallel: 1,
            rate_limit: None,
        }
    }

    fn format_url(&self, domain: &str, page: u32) -> String {
        let has_subdomain = domain.split('.').count() > 2;

        if !has_subdomain {
            format!(
                "https://otx.alienvault.com/api/v1/indicators/domain/{}/url_list?limit={}&page={}",
                domain, OTX_RESULTS_LIMIT, page
            )
        } else if has_subdomain && self.include_subdomains {
            // Extract the main domain
            let parts: Vec<&str> = domain.split('.').collect();
            let main_domain = if parts.len() >= 2 {
                parts[parts.len() - 2..].join(".")
            } else {
                domain.to_string()
            };

            format!(
                "https://otx.alienvault.com/api/v1/indicators/domain/{}/url_list?limit={}&page={}",
                main_domain, OTX_RESULTS_LIMIT, page
            )
        } else {
            format!(
                "https://otx.alienvault.com/api/v1/indicators/hostname/{}/url_list?limit={}&page={}",
                domain, OTX_RESULTS_LIMIT, page
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

                    // Don't sleep after the last attempt
                    if attempt < self.retries {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }

                let otx_result = match result {
                    Some(r) => r,
                    None => {
                        return Err(last_error.unwrap_or_else(|| {
                            anyhow::anyhow!("Failed to fetch OTX data after all retries")
                        }))
                    }
                };

                // Process the results
                for entry in otx_result.url_list {
                    if self.include_subdomains {
                        let has_subdomain = domain.split('.').count() > 2;
                        // Push the URL if we're not looking at a subdomain
                        // or if looking at a subdomain and the hostname contains our domain
                        if !has_subdomain
                            || entry
                                .hostname
                                .to_lowercase()
                                .contains(&domain.to_lowercase())
                        {
                            all_urls.push(entry.url);
                        }
                    } else if domain.to_lowercase() == entry.hostname.to_lowercase() {
                        all_urls.push(entry.url);
                    }
                }

                // Check if we should fetch the next page
                if !otx_result.has_next {
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

    fn with_parallel(&mut self, parallel: u32) {
        self.parallel = parallel;
    }

    fn with_rate_limit(&mut self, rate_limit: Option<f32>) {
        self.rate_limit = rate_limit;
    }
}
