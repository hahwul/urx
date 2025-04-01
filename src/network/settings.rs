/// Shared network configuration settings for HTTP requests
///
/// This struct centralizes common HTTP request settings used throughout
/// the application to avoid code duplication between providers and testers.
#[derive(Clone, Debug)]
pub struct NetworkSettings {
    /// Proxy server URL (e.g., "http://proxy.example.com:8080")
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
            .with_parallel(args.parallel)
            .with_subdomains(args.subs);

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
