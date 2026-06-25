use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::network::client::HttpClientConfig;
use crate::providers::Provider;

#[derive(Clone)]
pub struct RobotsProvider {
    timeout: Duration,
    retries: u32,
    random_agent: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    insecure: bool,
    #[cfg(test)]
    base_url: String,
    #[cfg(test)]
    base_url_http: String,
}

impl RobotsProvider {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            retries: 3,
            random_agent: false,
            proxy: None,
            proxy_auth: None,
            insecure: false,
            #[cfg(test)]
            base_url: String::new(),
            #[cfg(test)]
            base_url_http: String::new(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(&mut self, url: String) -> &mut Self {
        self.base_url = url;
        self
    }

    #[cfg(test)]
    pub fn with_http_base_url(&mut self, url: String) -> &mut Self {
        self.base_url_http = url;
        self
    }

    fn client_config(&self) -> HttpClientConfig {
        HttpClientConfig {
            timeout: self.timeout.as_secs(),
            insecure: self.insecure,
            random_agent: self.random_agent,
            proxy: self.proxy.clone(),
            proxy_auth: self.proxy_auth.clone(),
        }
    }

    /// Build the HTTP client via the shared config so it always sends a
    /// User-Agent (a UA-less request is rejected with 400 by some servers).
    fn build_client(&self) -> Result<Client> {
        self.client_config().build_client()
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

            #[cfg(not(test))]
            let https_url = format!("https://{domain}/robots.txt");

            #[cfg(test)]
            let https_url = if !self.base_url.is_empty() {
                format!("{}/robots.txt", self.base_url)
            } else {
                format!("https://{domain}/robots.txt")
            };

            let mut urls = Vec::new();

            // Try HTTPS first
            let https_resp = client.get(&https_url).send().await;
            // Track which protocol was successful
            let (is_https, text) = match https_resp {
                Ok(resp) if resp.status().is_success() => (true, resp.text().await?),
                _ => {
                    // If HTTPS fails, try HTTP
                    #[cfg(not(test))]
                    let http_url = format!("http://{domain}/robots.txt");

                    #[cfg(test)]
                    let http_url = if !self.base_url_http.is_empty() {
                        format!("{}/robots.txt", self.base_url_http)
                    } else if !self.base_url.is_empty() {
                        format!("{}/robots.txt", self.base_url)
                    } else {
                        format!("http://{domain}/robots.txt")
                    };

                    // robots.txt discovery is best-effort: a transport failure
                    // on the HTTP fallback means "no robots.txt", not a fatal
                    // error that should sink the whole provider.
                    let http_resp = match client.get(&http_url).send().await {
                        Ok(resp) => resp,
                        Err(_) => return Ok(urls),
                    };
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
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                // RFC 9309: field names are case-insensitive and may carry
                // surrounding whitespace (e.g. `Disallow :`). Split on the first
                // colon so `Sitemap: https://…` keeps its `https://` value.
                let Some((field, value)) = line.split_once(':') else {
                    continue;
                };
                // Take the first whitespace-delimited token of the value: paths
                // and URLs never contain spaces, so this drops any trailing
                // inline `# comment` and stray whitespace in one step.
                let value = value.split_whitespace().next().unwrap_or("");
                match field.trim().to_ascii_lowercase().as_str() {
                    "disallow" if !value.is_empty() && value != "/" => {
                        // Disallow entries can be match patterns, not literal
                        // paths: skip glob (`*`) patterns and strip a trailing
                        // `$` end-anchor so we don't emit unfetchable junk URLs.
                        if value.contains('*') {
                            continue;
                        }
                        let path = value.strip_suffix('$').unwrap_or(value);
                        if !path.is_empty() && path != "/" {
                            urls.push(format!("{protocol}://{domain}{path}"));
                        }
                    }
                    "sitemap" if !value.is_empty() => {
                        urls.push(value.to_string());
                    }
                    _ => {}
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
        self.random_agent = enabled;
    }
    fn with_insecure(&mut self, enabled: bool) {
        self.insecure = enabled;
    }
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
        assert!(!provider.random_agent);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert!(!provider.insecure);
        assert_eq!(provider.base_url, String::new());
        assert_eq!(provider.base_url_http, String::new());
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
        assert!(provider.random_agent);

        // Test disabling the random agent
        provider.with_random_agent(false);
        assert!(!provider.random_agent);
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
    async fn test_robots_directives_case_insensitive() {
        let mut server = mockito::Server::new_async().await;
        // Mixed/lower-case fields and a space before the colon — all RFC 9309
        // legal and all previously ignored by the case-sensitive parser.
        let robots = "user-agent: *\n\
                      disallow: /lower/\n\
                      DISALLOW: /upper\n\
                      Sitemap : https://example.com/sm.xml\n";
        let _m = server
            .mock("GET", "/robots.txt")
            .with_status(200)
            .with_body(robots)
            .create_async()
            .await;

        let mut provider = RobotsProvider::new();
        provider.with_base_url(server.url());
        let urls = provider.fetch_urls("example.com").await.unwrap();

        assert!(urls.contains(&"https://example.com/lower/".to_string()));
        assert!(urls.contains(&"https://example.com/upper".to_string()));
        assert!(urls.contains(&"https://example.com/sm.xml".to_string()));
    }

    #[tokio::test]
    async fn test_robots_skips_patterns_and_strips_comments() {
        let mut server = mockito::Server::new_async().await;
        let robots = "User-agent: *\n\
                      Disallow: /admin/*.php\n\
                      Disallow: /secret$\n\
                      Disallow: /good/\n\
                      Sitemap: https://example.com/sm.xml # main sitemap\n";
        let _m = server
            .mock("GET", "/robots.txt")
            .with_status(200)
            .with_body(robots)
            .create_async()
            .await;

        let mut provider = RobotsProvider::new();
        provider.with_base_url(server.url());
        let urls = provider.fetch_urls("example.com").await.unwrap();

        // Glob pattern is skipped entirely.
        assert!(!urls.iter().any(|u| u.contains('*')), "{urls:?}");
        // Trailing `$` anchor is stripped.
        assert!(urls.contains(&"https://example.com/secret".to_string()));
        assert!(urls.contains(&"https://example.com/good/".to_string()));
        // Inline comment is removed from the Sitemap value.
        assert!(urls.contains(&"https://example.com/sm.xml".to_string()));
        assert!(!urls.iter().any(|u| u.contains('#')), "{urls:?}");
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

    #[tokio::test]
    async fn test_fetch_urls_https_success() {
        let mut mock_server = mockito::Server::new_async().await;

        let robots_txt = "\
User-agent: *
Disallow: /private/
Sitemap: https://example.com/sitemap.xml
";

        let _m = mock_server
            .mock("GET", "/robots.txt")
            .with_status(200)
            .with_body(robots_txt)
            .create();

        let mut provider = RobotsProvider::new();
        provider.with_base_url(mock_server.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        // Protocol should be https because first request (simulated https) succeeded
        assert!(urls.contains(&"https://example.com/private/".to_string()));
        assert!(urls.contains(&"https://example.com/sitemap.xml".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_urls_http_fallback() {
        let mut mock_server_https = mockito::Server::new_async().await;
        let mut mock_server_http = mockito::Server::new_async().await;

        let robots_txt = "\
User-agent: *
Disallow: /private/
Sitemap: http://example.com/sitemap.xml
";

        // HTTPS fails
        let _m1 = mock_server_https
            .mock("GET", "/robots.txt")
            .with_status(500)
            .create();

        // HTTP succeeds
        let _m2 = mock_server_http
            .mock("GET", "/robots.txt")
            .with_status(200)
            .with_body(robots_txt)
            .create();

        let mut provider = RobotsProvider::new();
        provider.with_base_url(mock_server_https.url());
        provider.with_http_base_url(mock_server_http.url());

        let urls = provider.fetch_urls("example.com").await.unwrap();

        // Protocol should be http
        assert!(urls.contains(&"http://example.com/private/".to_string()));
        assert!(urls.contains(&"http://example.com/sitemap.xml".to_string()));
    }
}
