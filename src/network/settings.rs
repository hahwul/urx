/// Network scope specifying which components should use the network settings
#[derive(Clone, Debug, PartialEq, Default)]
pub enum NetworkScope {
    /// Apply network settings to all components
    #[default]
    All,
    /// Apply network settings only to providers
    Providers,
    /// Apply network settings only to testers
    Testers,
}

/// Shared network configuration settings for HTTP requests
///
/// This struct centralizes common HTTP request settings used throughout
/// the application to avoid code duplication between providers and testers.
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

    /// `-H` / `--cookie` / `--user-agent`, for the components that request
    /// URLs from the target. See [`crate::network::CustomHeaders`].
    pub headers: super::CustomHeaders,

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
            headers: super::CustomHeaders::default(),
            scope: NetworkScope::All,
        }
    }
}

impl NetworkSettings {
    /// Apply settings from command line arguments.
    ///
    /// # Errors
    ///
    /// Returns an error when `-H`, `--cookie` or `--user-agent` was given a
    /// value that is not a legal HTTP header. A malformed `-H` reads as "I
    /// sent an authenticated request" while sending an anonymous one, so it
    /// stops the run instead of being dropped.
    pub fn from_args(args: &crate::cli::Args) -> anyhow::Result<Self> {
        Ok(NetworkSettings {
            proxy: args.proxy.clone(),
            // Only meaningful alongside a proxy.
            proxy_auth: args.proxy.as_ref().and(args.proxy_auth.clone()),
            timeout: args.timeout.max(1),
            retries: args.retries,
            random_agent: args.random_agent,
            insecure: args.insecure,
            parallel: args.parallel.unwrap_or(5).max(1),
            rate_limit: args.rate_limit,
            include_subdomains: args.subs,
            headers: super::CustomHeaders::parse(
                &args.header,
                args.cookie.as_deref(),
                args.user_agent.as_deref(),
            )?,
            // `validate_network_scope` admits nothing else; anything it
            // doesn't name, including "providers,testers", means both.
            scope: match args.network_scope.to_lowercase().as_str() {
                "providers" => NetworkScope::Providers,
                "testers" => NetworkScope::Testers,
                _ => NetworkScope::All,
            },
        })
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
    fn test_from_args_basic() {
        use crate::cli::Args;
        use clap::Parser;

        let args = Args::parse_from(["urx", "example.com"]);
        let settings = NetworkSettings::from_args(&args).unwrap();

        assert_eq!(settings.timeout, 120); // Default timeout in args is 120
        assert_eq!(settings.retries, 2); // Default retries in args is 2
        assert!(!settings.random_agent);
        assert!(!settings.insecure);
        assert_eq!(settings.parallel, 5);
        assert_eq!(settings.rate_limit, None);
        assert!(!settings.include_subdomains);
        assert_eq!(settings.scope, NetworkScope::All);
    }

    #[test]
    fn test_from_args_with_proxy() {
        use crate::cli::Args;
        use clap::Parser;

        let args = Args::parse_from([
            "urx",
            "example.com",
            "--proxy",
            "http://proxy:8080",
            "--proxy-auth",
            "user:pass",
        ]);
        let settings = NetworkSettings::from_args(&args).unwrap();

        assert_eq!(settings.proxy, Some("http://proxy:8080".to_string()));
        assert_eq!(settings.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_from_args_with_network_options() {
        use crate::cli::Args;
        use clap::Parser;

        let args = Args::parse_from([
            "urx",
            "example.com",
            "--timeout",
            "60",
            "--retries",
            "5",
            "--random-agent",
            "--insecure",
            "--parallel",
            "10",
            "--rate-limit",
            "2.5",
            "--subs",
        ]);
        let settings = NetworkSettings::from_args(&args).unwrap();

        assert_eq!(settings.timeout, 60);
        assert_eq!(settings.retries, 5);
        assert!(settings.random_agent);
        assert!(settings.insecure);
        assert_eq!(settings.parallel, 10);
        assert_eq!(settings.rate_limit, Some(2.5));
        assert!(settings.include_subdomains);
    }

    #[test]
    fn test_from_args_clamps_zero_timeout_and_parallel() {
        use crate::cli::Args;
        use clap::Parser;

        let mut args = Args::parse_from(["urx", "example.com"]);
        args.timeout = 0;
        args.parallel = Some(0);

        let settings = NetworkSettings::from_args(&args).unwrap();

        assert_eq!(settings.timeout, 1);
        assert_eq!(settings.parallel, 1);
    }

    #[test]
    fn test_from_args_network_scope_providers() {
        use crate::cli::Args;
        use clap::Parser;

        let args = Args::parse_from(["urx", "example.com", "--network-scope", "providers"]);
        let settings = NetworkSettings::from_args(&args).unwrap();

        assert_eq!(settings.scope, NetworkScope::Providers);
    }

    #[test]
    fn test_from_args_network_scope_testers() {
        use crate::cli::Args;
        use clap::Parser;

        let args = Args::parse_from(["urx", "example.com", "--network-scope", "testers"]);
        let settings = NetworkSettings::from_args(&args).unwrap();

        assert_eq!(settings.scope, NetworkScope::Testers);
    }

    #[test]
    fn test_from_args_network_scope_providers_testers() {
        use crate::cli::Args;
        use clap::Parser;

        let args = Args::parse_from(["urx", "example.com", "--network-scope", "providers,testers"]);
        let settings = NetworkSettings::from_args(&args).unwrap();

        assert_eq!(settings.scope, NetworkScope::All);
    }

    #[test]
    fn test_from_args_parses_target_headers() {
        use crate::cli::Args;
        use clap::Parser;

        let args = Args::parse_from([
            "urx",
            "example.com",
            "-H",
            "X-Trace: urx",
            "--cookie",
            "session=abc",
            "--user-agent",
            "urx-test/1",
        ]);
        let settings = NetworkSettings::from_args(&args).unwrap();
        assert!(!settings.headers.is_empty());
        // X-Trace, Cookie, User-Agent.
        assert_eq!(settings.headers.len(), 3);
    }

    #[test]
    fn test_from_args_rejects_a_malformed_header() {
        use crate::cli::Args;
        use clap::Parser;

        // Silently dropping this would leave the user running an anonymous
        // scan while believing it was authenticated.
        let args = Args::parse_from(["urx", "example.com", "-H", "no-colon"]);
        assert!(NetworkSettings::from_args(&args).is_err());
    }
}
