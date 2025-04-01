use anyhow::Result;
use rand::Rng;
use std::future::Future;
use std::pin::Pin;

use super::Tester;

/// HTTP status checker for URLs
#[derive(Clone)]
pub struct StatusChecker {
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    include_status: Option<Vec<String>>,
    exclude_status: Option<Vec<String>>,
}

impl StatusChecker {
    /// Creates a new StatusChecker with default settings
    pub fn new() -> Self {
        StatusChecker {
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            include_status: None,
            exclude_status: None,
        }
    }

    /// Sets the status codes to include in the results
    pub fn with_include_status(&mut self, status_codes: Option<Vec<String>>) {
        self.include_status = status_codes;
    }

    /// Sets the status codes to exclude from the results
    pub fn with_exclude_status(&mut self, status_codes: Option<Vec<String>>) {
        self.exclude_status = status_codes;
    }

    /// Checks if a status code matches a pattern
    /// Patterns can be exact (e.g., "200") or wildcard (e.g., "20x", "3xx")
    fn status_matches_pattern(&self, status_code: u16, pattern: &str) -> bool {
        if pattern.contains('x') || pattern.contains('X') {
            let status_str = status_code.to_string();
            let pattern = pattern.to_lowercase();

            if status_str.len() != pattern.len() {
                return false;
            }

            for (s, p) in status_str.chars().zip(pattern.chars()) {
                if p != 'x' && p != s {
                    return false;
                }
            }

            true
        } else {
            // Exact match
            if let Ok(pattern_code) = pattern.parse::<u16>() {
                status_code == pattern_code
            } else {
                false
            }
        }
    }

    /// Checks if a status code matches any pattern in the given patterns vector
    fn matches_any_pattern(&self, status_code: u16, patterns: &[String]) -> bool {
        if patterns.is_empty() {
            return false;
        }

        patterns.iter().any(|pattern| {
            // Split the pattern by commas and check if any subpattern matches
            pattern.split(',').any(|subpattern| {
                self.status_matches_pattern(status_code, subpattern.trim())
            })
        })
    }

    /// Checks if a status code should be included in the results
    /// Returns true if:
    /// - include_status is set and the status code matches any of the patterns
    /// - exclude_status is set and the status code doesn't match any of the patterns
    /// - neither filter is set
    ///   Prioritizes include_status over exclude_status if both are set
    ///   Supports comma-separated patterns like "200,30x,40x"
    fn should_include_status(&self, status_code: u16) -> bool {
        // If include_status is set, only include status codes that match
        if let Some(include_patterns) = &self.include_status {
            return self.matches_any_pattern(status_code, include_patterns);
        }

        // If exclude_status is set, exclude status codes that match
        if let Some(exclude_patterns) = &self.exclude_status {
            return !self.matches_any_pattern(status_code, exclude_patterns);
        }

        // If neither filter is set, include all status codes
        true
    }
}

impl Tester for StatusChecker {
    fn clone_box(&self) -> Box<dyn Tester> {
        Box::new(self.clone())
    }

    /// Tests a URL by sending an HTTP request and returning the status code
    /// If status filtering is enabled, only returns URLs that match the filter criteria
    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
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

            for _ in 0..=self.retries {
                match client.get(url).send().await {
                    Ok(response) => {
                        let status = response.status();
                        let status_code = status.as_u16();

                        // Check if this status code should be included in results
                        if !self.should_include_status(status_code) {
                            return Ok(vec![]); // Return empty vec if filtered out
                        }

                        let status_text = format!(
                            "{} {}",
                            status_code,
                            status.canonical_reason().unwrap_or("")
                        );
                        return Ok(vec![format!("{} - {}", url, status_text)]);
                    }
                    Err(e) => {
                        last_error = Some(e);
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                }
            }

            // If we get here, all retries failed
            Err(anyhow::anyhow!(
                "Failed to check status for {}: {:?}",
                url,
                last_error
            ))
        })
    }

    /// Sets the request timeout in seconds
    fn with_timeout(&mut self, seconds: u64) {
        self.timeout = seconds;
    }

    /// Sets the number of retry attempts for failed requests
    fn with_retries(&mut self, count: u32) {
        self.retries = count;
    }

    /// Enables or disables the use of random User-Agent headers
    fn with_random_agent(&mut self, enabled: bool) {
        self.random_agent = enabled;
    }

    /// Enables or disables SSL certificate verification
    fn with_insecure(&mut self, enabled: bool) {
        self.insecure = enabled;
    }

    /// Sets the proxy server for HTTP requests
    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    /// Sets the proxy authentication credentials (username:password)
    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_matches_pattern() {
        let checker = StatusChecker::new();
        
        // Exact match test
        assert!(checker.status_matches_pattern(200, "200"));
        assert!(!checker.status_matches_pattern(200, "404"));
        
        // Wildcard match test
        assert!(checker.status_matches_pattern(200, "2xx"));
        assert!(checker.status_matches_pattern(200, "20x"));
        assert!(checker.status_matches_pattern(201, "20x"));
        assert!(checker.status_matches_pattern(404, "4xx"));
        assert!(!checker.status_matches_pattern(200, "3xx"));
        assert!(!checker.status_matches_pattern(200, "4xx"));
        
        // Case insensitivity test
        assert!(checker.status_matches_pattern(200, "2XX"));
        assert!(checker.status_matches_pattern(404, "4XX"));
    }

    #[test]
    fn test_matches_any_pattern() {
        let checker = StatusChecker::new();
        
        // Single pattern test
        assert!(checker.matches_any_pattern(200, &vec!["200".to_string()]));
        assert!(!checker.matches_any_pattern(404, &vec!["200".to_string()]));
        
        // Multiple pattern test
        assert!(checker.matches_any_pattern(200, &vec!["200".to_string(), "404".to_string()]));
        assert!(checker.matches_any_pattern(404, &vec!["200".to_string(), "404".to_string()]));
        assert!(!checker.matches_any_pattern(301, &vec!["200".to_string(), "404".to_string()]));
        
        // Wildcard pattern test
        assert!(checker.matches_any_pattern(200, &vec!["2xx".to_string()]));
        assert!(checker.matches_any_pattern(404, &vec!["2xx".to_string(), "4xx".to_string()]));
        
        // Comma-separated pattern test
        assert!(checker.matches_any_pattern(200, &vec!["200,404".to_string()]));
        assert!(checker.matches_any_pattern(404, &vec!["200,404".to_string()]));
        assert!(checker.matches_any_pattern(200, &vec!["2xx,404".to_string()]));
        assert!(!checker.matches_any_pattern(301, &vec!["200,404".to_string()]));
    }

    #[test]
    fn test_should_include_status() {
        let mut checker = StatusChecker::new();
        
        // Include all status codes when no filters are set
        assert!(checker.should_include_status(200));
        assert!(checker.should_include_status(404));
        assert!(checker.should_include_status(500));
        
        // include_status filter test
        checker.with_include_status(Some(vec!["200".to_string(), "3xx".to_string()]));
        assert!(checker.should_include_status(200));
        assert!(checker.should_include_status(301));
        assert!(!checker.should_include_status(404));
        assert!(!checker.should_include_status(500));
        
        // exclude_status filter test
        checker.with_include_status(None);
        checker.with_exclude_status(Some(vec!["4xx".to_string(), "500".to_string()]));
        assert!(checker.should_include_status(200));
        assert!(checker.should_include_status(301));
        assert!(!checker.should_include_status(404));
        assert!(!checker.should_include_status(500));
        
        // include_status has higher priority than exclude_status
        checker.with_include_status(Some(vec!["200".to_string()]));
        checker.with_exclude_status(Some(vec!["2xx".to_string()]));
        assert!(checker.should_include_status(200));
        assert!(!checker.should_include_status(201));
    }
}
