/// Shared network configuration settings for HTTP requests
///
/// This struct centralizes common HTTP request settings used throughout
/// the application to avoid code duplication between providers and testers.
/// Network scope specifying which components should use the network settings
#[derive(Clone, Debug, PartialEq)]
pub enum NetworkScope {
    /// Apply network settings to all components
    All,
    /// Apply network settings only to providers
    Providers,
    /// Apply network settings only to testers
    Testers,
}

impl Default for NetworkScope {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Clone, Debug)]
pub struct NetworkSettings {
    /// Proxy server URL (e.g., "<http://proxy.example.com:8080>")
    pub proxy: Option<String>,

    /// Proxy authentication in the format "username:password"
    pub proxy_auth: Option<String>,

    /// Request timeout in seconds
    pub timeout: u64,

    /// Number of retry attempts for failed requests
    pub retries: u32,

    /// Whether to use random User-Agent headers
    pub random_agent: bool,

    /// Whether to skip SSL certificate verification
    pub insecure: bool,

    /// Maximum number of parallel requests
    pub parallel: u32,

    /// Rate limit in requests per second
    pub rate_limit: Option<f32>,

    /// Whether to include subdomains in search
    pub include_subdomains: bool,

    /// Which components should use these network settings
    pub scope: NetworkScope,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            parallel: 5,
            rate_limit: None,
            include_subdomains: false,
            scope: NetworkScope::All,
        }
    }
}

impl NetworkSettings {
    /// Creates a new instance with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable subdomain inclusion
    pub fn with_subdomains(mut self, include: bool) -> Self {
        self.include_subdomains = include;
        self
    }

    /// Set the proxy server for HTTP requests
    pub fn with_proxy(mut self, proxy: Option<String>) -> Self {
        self.proxy = proxy;
        self
    }

    /// Set the proxy authentication credentials
    pub fn with_proxy_auth(mut self, auth: Option<String>) -> Self {
        self.proxy_auth = auth;
        self
    }

    /// Set the request timeout in seconds
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout = seconds;
        self
    }

    /// Set the number of retry attempts for failed requests
    pub fn with_retries(mut self, count: u32) -> Self {
        self.retries = count;
        self
    }

    /// Enable or disable the use of random User-Agent headers
    pub fn with_random_agent(mut self, enabled: bool) -> Self {
        self.random_agent = enabled;
        self
    }

    /// Enable or disable SSL certificate verification
    pub fn with_insecure(mut self, enabled: bool) -> Self {
        self.insecure = enabled;
        self
    }

    /// Set the number of parallel requests
    pub fn with_parallel(mut self, count: u32) -> Self {
        self.parallel = count;
        self
    }

    /// Set rate limiting to avoid being blocked
    pub fn with_rate_limit(mut self, requests_per_second: Option<f32>) -> Self {
        self.rate_limit = requests_per_second;
        self
    }

    /// Apply settings from command line arguments
    pub fn from_args(args: &crate::cli::Args) -> Self {
        let mut settings = NetworkSettings::new()
            .with_timeout(args.timeout)
            .with_retries(args.retries)
            .with_random_agent(args.random_agent)
            .with_insecure(args.insecure)
            .with_parallel(args.parallel.unwrap_or(5))
            .with_subdomains(args.subs);

        // Parse network scope from args
        let scope = match args.network_scope.to_lowercase().as_str() {
            "all" => NetworkScope::All,
            "providers" => NetworkScope::Providers,
            "testers" => NetworkScope::Testers,
            "providers,testers" | "testers,providers" => NetworkScope::All,
            _ => NetworkScope::All, // Default to All for invalid values
        };
        settings.scope = scope;

        if let Some(rate) = args.rate_limit {
            settings = settings.with_rate_limit(Some(rate));
        }

        if let Some(proxy) = &args.proxy {
            settings = settings.with_proxy(Some(proxy.clone()));

            if let Some(auth) = &args.proxy_auth {
                settings = settings.with_proxy_auth(Some(auth.clone()));
            }
        }

        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_scope_default() {
        let scope = NetworkScope::default();
        assert_eq!(scope, NetworkScope::All);
    }

    #[test]
    fn test_network_settings_default() {
        let settings = NetworkSettings::default();
        assert_eq!(settings.proxy, None);
        assert_eq!(settings.proxy_auth, None);
        assert_eq!(settings.timeout, 30);
        assert_eq!(settings.retries, 3);
        assert!(!settings.random_agent);
        assert!(!settings.insecure);
        assert_eq!(settings.parallel, 5);
        assert_eq!(settings.rate_limit, None);
        assert!(!settings.include_subdomains);
        assert_eq!(settings.scope, NetworkScope::All);
    }

    #[test]
    fn test_network_settings_new() {
        let settings = NetworkSettings::new();
        assert_eq!(settings.timeout, 30);
        assert_eq!(settings.retries, 3);
        assert_eq!(settings.parallel, 5);
    }

    #[test]
    fn test_with_subdomains() {
        let settings = NetworkSettings::new().with_subdomains(true);
        assert!(settings.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let proxy = "http://proxy.example.com:8080".to_string();
        let settings = NetworkSettings::new().with_proxy(Some(proxy.clone()));
        assert_eq!(settings.proxy, Some(proxy));
    }

    #[test]
    fn test_with_proxy_auth() {
        let auth = "username:password".to_string();
        let settings = NetworkSettings::new().with_proxy_auth(Some(auth.clone()));
        assert_eq!(settings.proxy_auth, Some(auth));
    }

    #[test]
    fn test_with_timeout() {
        let settings = NetworkSettings::new().with_timeout(60);
        assert_eq!(settings.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let settings = NetworkSettings::new().with_retries(5);
        assert_eq!(settings.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let settings = NetworkSettings::new().with_random_agent(true);
        assert!(settings.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let settings = NetworkSettings::new().with_insecure(true);
        assert!(settings.insecure);
    }

    #[test]
    fn test_with_parallel() {
        let settings = NetworkSettings::new().with_parallel(10);
        assert_eq!(settings.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let settings = NetworkSettings::new().with_rate_limit(Some(2.5));
        assert_eq!(settings.rate_limit, Some(2.5));
    }

    // Test chaining multiple settings
    #[test]
    fn test_chaining_settings() {
        let settings = NetworkSettings::new()
            .with_timeout(60)
            .with_retries(5)
            .with_random_agent(true)
            .with_insecure(true)
            .with_parallel(10)
            .with_rate_limit(Some(3.0))
            .with_subdomains(true)
            .with_proxy(Some("http://proxy.example.com:8080".to_string()))
            .with_proxy_auth(Some("user:pass".to_string()));

        assert_eq!(settings.timeout, 60);
        assert_eq!(settings.retries, 5);
        assert!(settings.random_agent);
        assert!(settings.insecure);
        assert_eq!(settings.parallel, 10);
        assert_eq!(settings.rate_limit, Some(3.0));
        assert!(settings.include_subdomains);
        assert_eq!(
            settings.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
        assert_eq!(settings.proxy_auth, Some("user:pass".to_string()));
    }
}
