use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

use super::Provider;

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
                    "https://web.archive.org/cdx/search/cdx?url=*.{}/*&output=json&fl=original",
                    domain
                )
            } else {
                format!(
                    "https://web.archive.org/cdx/search/cdx?url={}/*&output=json&fl=original",
                    domain
                )
            };

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

                match client.get(&url).send().await {
                    Ok(response) => {
                        // Check if response is successful
                        if !response.status().is_success() {
                            attempt += 1;
                            last_error = Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                            continue;
                        }

                        // Try to parse as JSON, but handle empty or invalid responses
                        match response.text().await {
                            Ok(text) => {
                                if text.trim().is_empty() {
                                    return Ok(Vec::new());
                                }

                                match serde_json::from_str::<Value>(&text) {
                                    Ok(json_data) => {
                                        let mut urls = Vec::new();

                                        // Skip the first array which is the header
                                        if let Value::Array(arrays) = json_data {
                                            for (i, array) in arrays.iter().enumerate() {
                                                // Skip the header row
                                                if i == 0 {
                                                    continue;
                                                }

                                                if let Value::Array(elements) = array {
                                                    if let Some(Value::String(url)) =
                                                        elements.first()
                                                    {
                                                        urls.push(url.clone());
                                                    }
                                                }
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
