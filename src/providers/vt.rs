use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::Provider;

#[derive(Clone)]
pub struct VirusTotalProvider {
    api_key: String,
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
    pub fn new(api_key: String) -> Self {
        VirusTotalProvider {
            api_key,
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
            // Skip if no API key is provided
            if self.api_key.is_empty() {
                return Ok(Vec::new());
            }

            // Use the url crate for encoding the domain
            let encoded_domain =
                url::form_urlencoded::byte_serialize(domain.as_bytes()).collect::<String>();
            let url = format!(
                "https://www.virustotal.com/vtapi/v2/domain/report?apikey={}&domain={}",
                self.api_key, encoded_domain
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
                let user_agents = [
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.82 Safari/537.36",
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Safari/605.1.15",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:86.0) Gecko/20100101 Firefox/86.0",
                    "Mozilla/5.0 (Linux; Android 10; SM-G973F) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Mobile Safari/537.36",
                    "Mozilla/5.0 (iPhone; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/87.0.4280.77 Mobile/15E148 Safari/604.1",
                ];
                let random_index = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as usize
                    % user_agents.len();
                client_builder = client_builder.user_agent(user_agents[random_index]);
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
