use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

/// Common HTTP client configuration shared across providers and testers.
///
/// This struct centralizes the logic for building a `reqwest::Client` with
/// proxy, timeout, TLS, and User-Agent settings so that every provider and
/// tester does not have to duplicate the same builder code.
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Request timeout in seconds
    pub timeout: u64,
    /// Skip TLS certificate verification
    pub insecure: bool,
    /// Use a randomized User-Agent header
    pub random_agent: bool,
    /// Optional proxy URL (e.g. "http://proxy:8080")
    pub proxy: Option<String>,
    /// Optional proxy authentication in "username:password" format
    pub proxy_auth: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            timeout: 30,
            insecure: false,
            random_agent: false,
            proxy: None,
            proxy_auth: None,
        }
    }
}

impl HttpClientConfig {
    /// Build a `reqwest::Client` from this configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the proxy URL is invalid or the client fails to build.
    pub fn build_client(&self) -> Result<Client> {
        let mut builder = Client::builder().timeout(Duration::from_secs(self.timeout));

        if self.insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }

        if self.random_agent {
            let ua = crate::network::random_user_agent();
            builder = builder.user_agent(ua);
        }

        if let Some(proxy_url) = &self.proxy {
            let mut proxy = reqwest::Proxy::all(proxy_url)?;

            if let Some(auth) = &self.proxy_auth {
                let username = auth.split(':').next().unwrap_or("");
                let password = auth.split(':').nth(1).unwrap_or("");
                proxy = proxy.basic_auth(username, password);
            }

            builder = builder.proxy(proxy);
        }

        Ok(builder.build()?)
    }
}

/// Execute an HTTP GET request with retry and exponential back-off.
///
/// `max_retries` is the number of **additional** attempts after the first
/// failure (i.e. total attempts = 1 + max_retries).
///
/// On success the response body is returned as a `String`.
///
/// # Errors
///
/// Returns the last encountered error if all attempts are exhausted.
pub async fn get_with_retry(client: &Client, url: &str, max_retries: u32) -> Result<String> {
    let mut last_error: Option<anyhow::Error> = None;
    let mut attempt: u32 = 0;

    while attempt <= max_retries {
        if attempt > 0 {
            // Exponential back-off: 500ms, 1000ms, 1500ms, …
            tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
        }

        match client.get(url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    last_error = Some(anyhow::anyhow!("HTTP error: {}", response.status()));
                    attempt += 1;
                    continue;
                }

                match response.text().await {
                    Ok(text) => return Ok(text),
                    Err(e) => {
                        last_error = Some(e.into());
                        attempt += 1;
                        continue;
                    }
                }
            }
            Err(e) => {
                last_error = Some(e.into());
                attempt += 1;
                continue;
            }
        }
    }

    if let Some(e) = last_error {
        Err(anyhow::anyhow!(
            "Failed after {} attempts: {}",
            max_retries + 1,
            e
        ))
    } else {
        Err(anyhow::anyhow!("Failed after {} attempts", max_retries + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HttpClientConfig::default();
        assert_eq!(config.timeout, 30);
        assert!(!config.insecure);
        assert!(!config.random_agent);
        assert!(config.proxy.is_none());
        assert!(config.proxy_auth.is_none());
    }

    #[test]
    fn test_build_client_default() {
        let config = HttpClientConfig::default();
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_insecure() {
        let config = HttpClientConfig {
            insecure: true,
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_random_agent() {
        let config = HttpClientConfig {
            random_agent: true,
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_with_proxy() {
        let config = HttpClientConfig {
            proxy: Some("http://127.0.0.1:8080".to_string()),
            proxy_auth: Some("user:pass".to_string()),
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_with_proxy_no_auth() {
        let config = HttpClientConfig {
            proxy: Some("http://127.0.0.1:8080".to_string()),
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_with_custom_timeout() {
        let config = HttpClientConfig {
            timeout: 120,
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_all_options() {
        let config = HttpClientConfig {
            timeout: 60,
            insecure: true,
            random_agent: true,
            proxy: Some("http://127.0.0.1:8080".to_string()),
            proxy_auth: Some("admin:secret".to_string()),
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }
}
