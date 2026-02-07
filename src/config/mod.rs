use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Args;

/// Represents the application configuration loaded from a file
#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub provider: ProviderConfig,

    #[serde(default)]
    pub filter: FilterConfig,

    #[serde(default)]
    pub network: NetworkConfig,

    #[serde(default)]
    pub testing: TestingConfig,

    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct OutputConfig {
    pub output: Option<String>,
    pub format: Option<String>,
    pub merge_endpoint: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ProviderConfig {
    pub providers: Option<Vec<String>>,
    pub subs: Option<bool>,
    pub cc_index: Option<String>,
    pub vt_api_key: Option<String>,
    pub urlscan_api_key: Option<String>,
    pub include_robots: Option<bool>,
    pub include_sitemap: Option<bool>,
    pub exclude_robots: Option<bool>,
    pub exclude_sitemap: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FilterConfig {
    pub preset: Option<Vec<String>>,
    pub extensions: Option<Vec<String>>,
    pub exclude_extensions: Option<Vec<String>>,
    pub patterns: Option<Vec<String>>,
    pub exclude_patterns: Option<Vec<String>>,
    pub show_only_host: Option<bool>,
    pub show_only_path: Option<bool>,
    pub show_only_param: Option<bool>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct NetworkConfig {
    pub network_scope: Option<String>,
    pub proxy: Option<String>,
    pub proxy_auth: Option<String>,
    pub insecure: Option<bool>,
    pub random_agent: Option<bool>,
    pub timeout: Option<u64>,
    pub retries: Option<u32>,
    pub parallel: Option<u32>,
    pub rate_limit: Option<f32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct TestingConfig {
    pub check_status: Option<bool>,
    pub include_status: Option<Vec<String>>,
    pub exclude_status: Option<Vec<String>>,
    pub extract_links: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CacheConfig {
    pub incremental: Option<bool>,
    pub cache_type: Option<String>,
    pub cache_path: Option<String>,
    pub redis_url: Option<String>,
    pub cache_ttl: Option<u64>,
    pub no_cache: Option<bool>,
}

impl Config {
    /// Load configuration from a specific file path
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.as_ref().display()))?;

        Ok(config)
    }

    /// Get the default configuration file path
    /// - Linux/macOS: ~/.config/urx/config.toml
    /// - Windows: %AppData%\urx\config.toml
    ///
    /// If the directory doesn't exist, it will be created.
    /// If the file doesn't exist, an empty config.toml file will be created.
    pub fn default_path() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            if let Some(app_data) = env::var_os("APPDATA").map(PathBuf::from) {
                let config_dir = app_data.join("urx");
                let config_path = config_dir.join("config.toml");

                // Create directory if it doesn't exist
                if !config_dir.exists() {
                    if let Err(_) = fs::create_dir_all(&config_dir) {
                        return None;
                    }
                }

                // Create empty config file if it doesn't exist
                if !config_path.exists() {
                    if let Err(_) = fs::write(&config_path, "") {
                        return None;
                    }
                }

                return Some(config_path);
            }
        }

        #[cfg(not(windows))]
        {
            if let Some(home) = home_dir() {
                let config_dir = home.join(".config").join("urx");
                let config_path = config_dir.join("config.toml");

                // Create directory if it doesn't exist
                if !config_dir.exists() && fs::create_dir_all(&config_dir).is_err() {
                    return None;
                }

                // Create empty config file if it doesn't exist
                if !config_path.exists() && fs::write(&config_path, "").is_err() {
                    return None;
                }

                return Some(config_path);
            }
        }

        None
    }

    /// Load configuration based on command line arguments
    /// Priority: --config flag > default path > default values
    pub fn load(args: &Args) -> Self {
        // Try to load from --config flag first
        if let Some(path) = &args.config {
            if let Ok(config) = Self::from_file(path) {
                return config;
            }
        }

        // Then try default location
        if let Some(default_path) = Self::default_path() {
            if let Ok(config) = Self::from_file(default_path.clone()) {
                return config;
            }
        }

        // Otherwise use default values
        Config::default()
    }

    /// Apply configuration values to Args, respecting priority
    /// Command line arguments take precedence over config file values
    pub fn apply_to_args(self, args: &mut Args) {
        // Output options
        if args.output.is_none() {
            if let Some(output) = self.output.output {
                args.output = Some(PathBuf::from(output));
            }
        }

        if args.format == "plain" {
            if let Some(format) = self.output.format {
                args.format = format;
            }
        }

        if !args.merge_endpoint && self.output.merge_endpoint.unwrap_or(false) {
            args.merge_endpoint = true;
        }

        // Provider options
        if args.providers == vec!["wayback", "cc", "otx"] {
            if let Some(providers) = self.provider.providers {
                args.providers = providers;
            }
        }

        if !args.subs && self.provider.subs.unwrap_or(false) {
            args.subs = true;
        }

        if args.cc_index == "CC-MAIN-2025-13" {
            if let Some(cc_index) = self.provider.cc_index {
                args.cc_index = cc_index;
            }
        }

        if args.vt_api_key.is_empty() {
            if let Some(vt_api_key) = self.provider.vt_api_key {
                args.vt_api_key.push(vt_api_key);
            }
        }

        if args.urlscan_api_key.is_empty() {
            if let Some(urlscan_api_key) = self.provider.urlscan_api_key {
                args.urlscan_api_key.push(urlscan_api_key);
            }
        }

        // Handle robots.txt and sitemap.xml discovery options
        if !args.exclude_robots && self.provider.exclude_robots.unwrap_or(false) {
            args.exclude_robots = true;
        }

        if !args.exclude_sitemap && self.provider.exclude_sitemap.unwrap_or(false) {
            args.exclude_sitemap = true;
        }

        // Only apply include_* if exclude_* is not set (exclude takes precedence)
        if !args.exclude_robots && args.include_robots {
            if let Some(include_robots) = self.provider.include_robots {
                args.include_robots = include_robots;
            }
        }

        if !args.exclude_sitemap && args.include_sitemap {
            if let Some(include_sitemap) = self.provider.include_sitemap {
                args.include_sitemap = include_sitemap;
            }
        }

        // Filter options
        if args.preset.is_empty() {
            if let Some(preset) = self.filter.preset {
                args.preset = preset;
            }
        }

        if args.extensions.is_empty() {
            if let Some(extensions) = self.filter.extensions {
                args.extensions = extensions;
            }
        }

        if args.exclude_extensions.is_empty() {
            if let Some(exclude_extensions) = self.filter.exclude_extensions {
                args.exclude_extensions = exclude_extensions;
            }
        }

        if args.patterns.is_empty() {
            if let Some(patterns) = self.filter.patterns {
                args.patterns = patterns;
            }
        }

        if args.exclude_patterns.is_empty() {
            if let Some(exclude_patterns) = self.filter.exclude_patterns {
                args.exclude_patterns = exclude_patterns;
            }
        }

        if !args.show_only_host && self.filter.show_only_host.unwrap_or(false) {
            args.show_only_host = true;
        }

        if !args.show_only_path && self.filter.show_only_path.unwrap_or(false) {
            args.show_only_path = true;
        }

        if !args.show_only_param && self.filter.show_only_param.unwrap_or(false) {
            args.show_only_param = true;
        }

        if args.min_length.is_none() && self.filter.min_length.is_some() {
            args.min_length = self.filter.min_length;
        }

        if args.max_length.is_none() && self.filter.max_length.is_some() {
            args.max_length = self.filter.max_length;
        }

        // Network options
        if args.network_scope == "all" {
            if let Some(network_scope) = self.network.network_scope {
                args.network_scope = network_scope;
            }
        }

        if args.proxy.is_none() && self.network.proxy.is_some() {
            args.proxy = self.network.proxy;
        }

        if args.proxy_auth.is_none() && self.network.proxy_auth.is_some() {
            args.proxy_auth = self.network.proxy_auth;
        }

        if !args.insecure && self.network.insecure.unwrap_or(false) {
            args.insecure = true;
        }

        if !args.random_agent && self.network.random_agent.unwrap_or(false) {
            args.random_agent = true;
        }

        if args.timeout == 30 {
            if let Some(timeout) = self.network.timeout {
                args.timeout = timeout;
            }
        }

        if args.retries == 3 {
            if let Some(retries) = self.network.retries {
                args.retries = retries;
            }
        }

        if args.parallel.unwrap_or(5) == 5 && self.network.parallel.is_some() {
            args.parallel = self.network.parallel;
        }

        if args.rate_limit.is_none() && self.network.rate_limit.is_some() {
            args.rate_limit = self.network.rate_limit;
        }

        // Testing options
        if !args.check_status && self.testing.check_status.unwrap_or(false) {
            args.check_status = true;
        }

        if args.include_status.is_empty() {
            if let Some(include_status) = self.testing.include_status {
                args.include_status = include_status;
            }
        }

        if args.exclude_status.is_empty() {
            if let Some(exclude_status) = self.testing.exclude_status {
                args.exclude_status = exclude_status;
            }
        }

        if !args.extract_links && self.testing.extract_links.unwrap_or(false) {
            args.extract_links = true;
        }

        // Cache options
        if !args.incremental && self.cache.incremental.unwrap_or(false) {
            args.incremental = true;
        }

        if args.cache_type == "sqlite" {
            if let Some(cache_type) = self.cache.cache_type {
                args.cache_type = cache_type;
            }
        }

        if args.cache_path.is_none() {
            if let Some(cache_path) = self.cache.cache_path {
                args.cache_path = Some(PathBuf::from(cache_path));
            }
        }

        if args.redis_url.is_none() && self.cache.redis_url.is_some() {
            args.redis_url = self.cache.redis_url;
        }

        if args.cache_ttl == 86400 {
            if let Some(cache_ttl) = self.cache.cache_ttl {
                args.cache_ttl = cache_ttl;
            }
        }

        if !args.no_cache && self.cache.no_cache.unwrap_or(false) {
            args.no_cache = true;
        }
    }
}

/// Helper function to get the home directory
fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from).or({
        #[cfg(windows)]
        {
            // On Windows, try USERPROFILE first, then HOMEDRIVE + HOMEPATH
            if let Some(profile) = env::var_os("USERPROFILE").map(PathBuf::from) {
                return Some(profile);
            }

            match (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
                (Some(drive), Some(path)) => {
                    let mut drive_path = PathBuf::from(drive);
                    drive_path.push(path);
                    Some(drive_path)
                }
                _ => None,
            }
        }

        #[cfg(not(windows))]
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_config_from_file() {
        // Create a temporary config file
        let config_content = r#"
            [output]
            output = "test-output.txt"
            format = "json"
            merge_endpoint = true

            [provider]
            providers = ["wayback", "cc"]
            subs = true
            cc_index = "CC-MAIN-2025-04"

            [filter]
            extensions = ["php", "js"]
            show_only_host = true
        "#;

        let temp_file = create_temp_config_file(config_content);

        // Load the config from the temp file
        let config = Config::from_file(temp_file.path()).unwrap();

        // Verify the loaded config values
        assert_eq!(config.output.output, Some("test-output.txt".to_string()));
        assert_eq!(config.output.format, Some("json".to_string()));
        assert_eq!(config.output.merge_endpoint, Some(true));

        assert_eq!(
            config.provider.providers,
            Some(vec!["wayback".to_string(), "cc".to_string()])
        );
        assert_eq!(config.provider.subs, Some(true));
        assert_eq!(
            config.provider.cc_index,
            Some("CC-MAIN-2025-04".to_string())
        );

        assert_eq!(
            config.filter.extensions,
            Some(vec!["php".to_string(), "js".to_string()])
        );
        assert_eq!(config.filter.show_only_host, Some(true));
    }

    #[test]
    fn test_default_config() {
        // Default config should have default values
        let config = Config::default();

        assert_eq!(config.output.output, None);
        assert_eq!(config.output.format, None);
        assert_eq!(config.output.merge_endpoint, None);

        assert_eq!(config.provider.providers, None);
        assert_eq!(config.provider.subs, None);
        assert_eq!(config.provider.cc_index, None);

        assert_eq!(config.filter.extensions, None);
        assert_eq!(config.filter.show_only_host, None);
    }

    #[test]
    fn test_apply_to_args() {
        // Create a config with some values
        let mut config = Config::default();
        config.output.output = Some("output.txt".to_string());
        config.output.format = Some("json".to_string());
        config.provider.providers = Some(vec!["cc".to_string()]);

        // Create default args
        let mut args = Args {
            domains: vec![],
            config: None,
            files: vec![],
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            normalize_url: false,
            providers: vec!["wayback".to_string(), "cc".to_string(), "otx".to_string()],
            subs: false,
            cc_index: "CC-MAIN-2025-13".to_string(),
            vt_api_key: vec![],
            urlscan_api_key: vec![],
            verbose: false,
            silent: false,
            no_progress: false,
            preset: vec![],
            extensions: vec![],
            exclude_extensions: vec![],
            patterns: vec![],
            exclude_patterns: vec![],
            show_only_host: false,
            show_only_path: false,
            show_only_param: false,
            min_length: None,
            max_length: None,
            strict: true,
            network_scope: "all".to_string(),
            proxy: None,
            proxy_auth: None,
            insecure: false,
            random_agent: false,
            timeout: 30,
            retries: 3,
            parallel: Some(5),
            rate_limit: None,
            check_status: false,
            include_status: vec![],
            exclude_status: vec![],
            extract_links: false,
            include_robots: true,
            include_sitemap: true,
            exclude_robots: false,
            exclude_sitemap: false,
            incremental: false,
            cache_type: "sqlite".to_string(),
            cache_path: None,
            redis_url: None,
            cache_ttl: 86400,
            no_cache: false,
        };
        assert_eq!(args.output, None);
        assert_eq!(args.format, "plain");
        assert_eq!(args.providers, vec!["wayback", "cc", "otx"]);

        // Apply config to args
        config.apply_to_args(&mut args);

        // Verify args were updated correctly
        assert_eq!(args.output, Some(PathBuf::from("output.txt")));
        assert_eq!(args.format, "json");
        assert_eq!(args.providers, vec!["cc"]);
    }
}
