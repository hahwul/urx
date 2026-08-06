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
    pub stream: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ProviderConfig {
    pub providers: Option<Vec<String>>,
    pub subs: Option<bool>,
    pub cc_index: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub archive_status: Option<Vec<String>>,
    pub archive_exclude_status: Option<Vec<String>>,
    pub archive_mime: Option<Vec<String>>,
    pub archive_exclude_mime: Option<Vec<String>>,
    pub vt_api_key: Option<String>,
    pub urlscan_api_key: Option<String>,
    pub zoomeye_api_key: Option<String>,
    pub github_api_key: Option<String>,
    pub include_robots: Option<bool>,
    pub include_sitemap: Option<bool>,
    pub exclude_robots: Option<bool>,
    pub exclude_sitemap: Option<bool>,
}

/// Provider-config file: a small TOML that holds only API keys so the main
/// config (filter rules, output formatting, etc.) can be checked into source
/// control without leaking secrets. Comma-separated values rotate.
#[derive(Debug, Deserialize, Default)]
pub struct ProviderKeysConfig {
    pub vt_api_key: Option<String>,
    pub urlscan_api_key: Option<String>,
    pub zoomeye_api_key: Option<String>,
    pub github_api_key: Option<String>,
}

/// Split a comma-separated key list into individual keys, trimming each and
/// dropping blanks. Shared by both config layers so they agree on what
/// `"k1, k2"` means.
fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

impl ProviderKeysConfig {
    /// Parse a provider-config TOML file from `path`. Returns the parsed
    /// struct or an error.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "Failed to read provider-config file: {}",
                path.as_ref().display()
            )
        })?;
        let parsed: ProviderKeysConfig = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse provider-config file: {}",
                path.as_ref().display()
            )
        })?;
        Ok(parsed)
    }

    /// Default lookup path mirrors the main config: $XDG_CONFIG_HOME/urx or
    /// %APPDATA%\urx. Returns None when neither exists; unlike `Config`, we
    /// do NOT auto-create the file because that would land an empty
    /// "credentials" path the user didn't ask for.
    pub fn default_path() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            if let Some(app_data) = env::var_os("APPDATA").map(PathBuf::from) {
                let p = app_data.join("urx").join("provider-config.toml");
                if p.exists() {
                    return Some(p);
                }
            }
        }
        #[cfg(not(windows))]
        {
            if let Some(home) = home_dir() {
                let p = home
                    .join(".config")
                    .join("urx")
                    .join("provider-config.toml");
                if p.exists() {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Load using the same precedence as the main config: --provider-config
    /// flag wins, then the default path. Returns an empty config when no file
    /// is found so callers can chain it freely.
    pub fn load(args: &Args) -> Result<Self> {
        if let Some(path) = &args.provider_config {
            return Self::from_file(path);
        }
        if let Some(path) = Self::default_path() {
            return Self::from_file(path);
        }
        Ok(ProviderKeysConfig::default())
    }

    /// Apply keys to args, but only for slots not already supplied via CLI
    /// (or env-via-CLI). The main `Config` runs first and may have filled
    /// these slots; this method then overwrites them when the provider-config
    /// has a value, so provider-config beats main config.
    ///
    /// `cli_supplied_*` flags carry the original CLI state captured BEFORE
    /// either config layer ran, so CLI input is preserved.
    pub fn apply_to_args(
        &self,
        args: &mut Args,
        cli_supplied_vt: bool,
        cli_supplied_urlscan: bool,
        cli_supplied_zoomeye: bool,
        cli_supplied_github: bool,
    ) {
        if !cli_supplied_vt {
            if let Some(keys) = &self.vt_api_key {
                args.vt_api_key = split_csv(keys);
            }
        }
        if !cli_supplied_urlscan {
            if let Some(keys) = &self.urlscan_api_key {
                args.urlscan_api_key = split_csv(keys);
            }
        }
        if !cli_supplied_zoomeye {
            if let Some(keys) = &self.zoomeye_api_key {
                args.zoomeye_api_key = split_csv(keys);
            }
        }
        if !cli_supplied_github {
            if let Some(keys) = &self.github_api_key {
                args.github_api_key = split_csv(keys);
            }
        }
    }
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

fn normalize_output_format(format: &str) -> Option<String> {
    match format.trim().to_ascii_lowercase().as_str() {
        "plain" => Some("plain".to_string()),
        "json" => Some("json".to_string()),
        "jsonl" => Some("jsonl".to_string()),
        "csv" => Some("csv".to_string()),
        _ => None,
    }
}

fn normalize_network_scope(scope: &str) -> Option<String> {
    match scope.trim().to_ascii_lowercase().as_str() {
        "all" => Some("all".to_string()),
        "providers" => Some("providers".to_string()),
        "testers" => Some("testers".to_string()),
        "providers,testers" | "testers,providers" => Some("providers,testers".to_string()),
        _ => None,
    }
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
    pub fn load(args: &Args) -> Result<Self> {
        // Try to load from --config flag first
        if let Some(path) = &args.config {
            return Self::from_file(path);
        }

        // Then try default location
        if let Some(default_path) = Self::default_path() {
            return Self::from_file(default_path);
        }

        // Otherwise use default values
        Ok(Config::default())
    }

    /// Apply configuration values to Args, respecting priority
    /// Command line arguments take precedence over config file values
    pub fn apply_to_args(self, args: &mut Args) {
        self.apply_output_config(args);
        self.apply_provider_config(args);
        self.apply_filter_config(args);
        self.apply_network_config(args);
        self.apply_testing_config(args);
        self.apply_cache_config(args);
    }

    fn apply_output_config(&self, args: &mut Args) {
        // Output options
        if args.output.is_none() {
            if let Some(output) = &self.output.output {
                args.output = Some(PathBuf::from(output));
            }
        }

        if args.format == "plain" {
            if let Some(format) = &self.output.format {
                if let Some(format) = normalize_output_format(format) {
                    args.format = format;
                } else if !args.silent {
                    eprintln!(
                        "Ignoring [output].format={format:?} in config: expected plain, json, jsonl, or csv"
                    );
                }
            }
        }

        if !args.merge_endpoint && self.output.merge_endpoint.unwrap_or(false) {
            args.merge_endpoint = true;
        }

        if !args.stream && self.output.stream.unwrap_or(false) {
            args.stream = true;
        }
    }

    fn apply_provider_config(&self, args: &mut Args) {
        // Provider options
        if args.providers == vec!["wayback", "cc", "otx"] {
            if let Some(providers) = &self.provider.providers {
                args.providers = providers.clone();
            }
        }

        if !args.subs && self.provider.subs.unwrap_or(false) {
            args.subs = true;
        }

        // Treat the default singleton list as "not user-supplied" so the file
        // value wins. Config file still accepts a single string; we split it
        // on commas so users can configure multi-index there too.
        let cc_default = vec!["latest".to_string()];
        if args.cc_index == cc_default {
            if let Some(cc_index) = &self.provider.cc_index {
                let split: Vec<String> = cc_index
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !split.is_empty() {
                    args.cc_index = split;
                }
            }
        }

        // Archive-side CDX predicates. Each applies only when the CLI left the
        // slot untouched, matching how every other provider option resolves.
        if args.from.is_none() && self.provider.from.is_some() {
            args.from = self.provider.from.clone();
        }

        if args.to.is_none() && self.provider.to.is_some() {
            args.to = self.provider.to.clone();
        }

        if args.archive_status.is_empty() {
            if let Some(v) = &self.provider.archive_status {
                args.archive_status = v.clone();
            }
        }

        if args.archive_exclude_status.is_empty() {
            if let Some(v) = &self.provider.archive_exclude_status {
                args.archive_exclude_status = v.clone();
            }
        }

        if args.archive_mime.is_empty() {
            if let Some(v) = &self.provider.archive_mime {
                args.archive_mime = v.clone();
            }
        }

        if args.archive_exclude_mime.is_empty() {
            if let Some(v) = &self.provider.archive_exclude_mime {
                args.archive_exclude_mime = v.clone();
            }
        }

        // API keys rotate when several are given, and every other source
        // separates them with commas: the env vars do, and so does the
        // provider-config file. The main config used to push the whole string as
        // a single key, so `vt_api_key = "k1,k2"` became one key literally named
        // "k1,k2" — which simply fails to authenticate, with no hint why.
        if args.vt_api_key.is_empty() {
            if let Some(vt_api_key) = &self.provider.vt_api_key {
                args.vt_api_key = split_csv(vt_api_key);
            }
        }

        if args.urlscan_api_key.is_empty() {
            if let Some(urlscan_api_key) = &self.provider.urlscan_api_key {
                args.urlscan_api_key = split_csv(urlscan_api_key);
            }
        }

        if args.zoomeye_api_key.is_empty() {
            if let Some(zoomeye_api_key) = &self.provider.zoomeye_api_key {
                args.zoomeye_api_key = split_csv(zoomeye_api_key);
            }
        }

        if args.github_api_key.is_empty() {
            if let Some(github_api_key) = &self.provider.github_api_key {
                args.github_api_key = split_csv(github_api_key);
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
    }

    fn apply_filter_config(&self, args: &mut Args) {
        // Filter options
        if args.preset.is_empty() {
            if let Some(preset) = &self.filter.preset {
                args.preset = preset.clone();
            }
        }

        if args.extensions.is_empty() {
            if let Some(extensions) = &self.filter.extensions {
                args.extensions = extensions.clone();
            }
        }

        if args.exclude_extensions.is_empty() {
            if let Some(exclude_extensions) = &self.filter.exclude_extensions {
                args.exclude_extensions = exclude_extensions.clone();
            }
        }

        if args.patterns.is_empty() {
            if let Some(patterns) = &self.filter.patterns {
                args.patterns = patterns.clone();
            }
        }

        if args.exclude_patterns.is_empty() {
            if let Some(exclude_patterns) = &self.filter.exclude_patterns {
                args.exclude_patterns = exclude_patterns.clone();
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
    }

    fn apply_network_config(&self, args: &mut Args) {
        // Network options
        if args.network_scope == "all" {
            if let Some(network_scope) = &self.network.network_scope {
                if let Some(network_scope) = normalize_network_scope(network_scope) {
                    args.network_scope = network_scope;
                } else if !args.silent {
                    eprintln!(
                        "Ignoring [network].network_scope={network_scope:?} in config: expected all, providers, testers, or providers,testers"
                    );
                }
            }
        }

        if args.proxy.is_none() && self.network.proxy.is_some() {
            args.proxy = self.network.proxy.clone();
        }

        if args.proxy_auth.is_none() && self.network.proxy_auth.is_some() {
            args.proxy_auth = self.network.proxy_auth.clone();
        }

        if !args.insecure && self.network.insecure.unwrap_or(false) {
            args.insecure = true;
        }

        if !args.random_agent && self.network.random_agent.unwrap_or(false) {
            args.random_agent = true;
        }

        if args.timeout == 120 {
            if let Some(timeout) = self.network.timeout {
                if timeout > 0 {
                    args.timeout = timeout;
                } else if !args.silent {
                    eprintln!(
                        "Ignoring [network].timeout=0 in config: value must be at least 1 second"
                    );
                }
            }
        }

        if args.retries == 2 {
            if let Some(retries) = self.network.retries {
                args.retries = retries;
            }
        }

        if args.parallel.unwrap_or(5) == 5 && self.network.parallel.is_some() {
            if let Some(parallel) = self.network.parallel {
                if parallel > 0 {
                    args.parallel = Some(parallel);
                } else if !args.silent {
                    eprintln!("Ignoring [network].parallel=0 in config: value must be at least 1");
                }
            }
        }

        if args.rate_limit.is_none() && self.network.rate_limit.is_some() {
            args.rate_limit = self.network.rate_limit;
        }
    }

    fn apply_testing_config(&self, args: &mut Args) {
        // Testing options
        if !args.check_status && self.testing.check_status.unwrap_or(false) {
            args.check_status = true;
        }

        if args.include_status.is_empty() {
            if let Some(include_status) = &self.testing.include_status {
                args.include_status = include_status.clone();
            }
        }

        if args.exclude_status.is_empty() {
            if let Some(exclude_status) = &self.testing.exclude_status {
                args.exclude_status = exclude_status.clone();
            }
        }

        if !args.extract_links && self.testing.extract_links.unwrap_or(false) {
            args.extract_links = true;
        }
    }

    fn apply_cache_config(&self, args: &mut Args) {
        // Cache options
        if !args.incremental && self.cache.incremental.unwrap_or(false) {
            args.incremental = true;
        }

        if args.cache_type == "sqlite" {
            if let Some(cache_type) = &self.cache.cache_type {
                // Mirrors how [output].format and [network].network_scope are
                // handled: name the bad value at the point it was read, rather
                // than failing later with an error that doesn't mention the
                // config file at all.
                match cache_type.trim().to_ascii_lowercase().as_str() {
                    valid @ ("sqlite" | "redis") => args.cache_type = valid.to_string(),
                    _ => {
                        if !args.silent {
                            eprintln!(
                                "Ignoring [cache].cache_type={cache_type:?} in config: expected sqlite or redis"
                            );
                        }
                    }
                }
            }
        }

        if args.cache_path.is_none() {
            if let Some(cache_path) = &self.cache.cache_path {
                args.cache_path = Some(PathBuf::from(cache_path));
            }
        }

        if args.redis_url.is_none() && self.cache.redis_url.is_some() {
            args.redis_url = self.cache.redis_url.clone();
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

#[cfg_attr(windows, allow(dead_code))]
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
    use clap::Parser;
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

        // Defaults come straight from clap, matching the sibling tests below;
        // an inline literal here was a fourth copy of the Args fixture.
        let mut args = Args::parse_from(["urx", "example.com"]);
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

    #[test]
    fn test_apply_to_args_ignores_invalid_network_values() {
        let mut config = Config::default();
        config.network.timeout = Some(0);
        config.network.parallel = Some(0);

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args);

        assert_eq!(args.timeout, 120);
        assert_eq!(args.parallel, Some(5));
    }

    #[test]
    fn test_apply_to_args_ignores_invalid_output_format_and_network_scope() {
        let mut config = Config::default();
        config.output.format = Some("yaml".to_string());
        config.network.network_scope = Some("providers only".to_string());

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args);

        assert_eq!(args.format, "plain");
        assert_eq!(args.network_scope, "all");
    }

    #[test]
    fn test_apply_to_args_normalizes_output_format_and_network_scope() {
        let mut config = Config::default();
        config.output.format = Some("JSON".to_string());
        config.network.network_scope = Some("TESTERS,PROVIDERS".to_string());

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args);

        assert_eq!(args.format, "json");
        assert_eq!(args.network_scope, "providers,testers");
    }

    #[test]
    fn test_provider_keys_config_parses_csv() -> Result<()> {
        let content = r#"
            vt_api_key = "key1, key2 ,key3"
            urlscan_api_key = "us1"
        "#;
        let file = create_temp_config_file(content);
        let cfg = ProviderKeysConfig::from_file(file.path())?;
        assert_eq!(cfg.vt_api_key.as_deref(), Some("key1, key2 ,key3"));
        assert_eq!(cfg.urlscan_api_key.as_deref(), Some("us1"));
        assert_eq!(cfg.zoomeye_api_key, None);
        Ok(())
    }

    #[test]
    fn test_config_load_returns_error_for_explicit_missing_file() {
        let args = Args::parse_from(["urx", "--config", "/definitely/missing.toml", "example.com"]);
        let err = Config::load(&args).unwrap_err();
        assert!(err.to_string().contains("Failed to read config file"));
    }

    #[test]
    fn test_provider_keys_load_returns_error_for_explicit_missing_file() {
        let args = Args::parse_from([
            "urx",
            "--provider-config",
            "/definitely/missing-provider.toml",
            "example.com",
        ]);
        let err = ProviderKeysConfig::load(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("Failed to read provider-config file"));
    }

    #[test]
    fn test_config_load_succeeds_without_explicit_file() -> Result<()> {
        let args = Args::parse_from(["urx", "example.com"]);
        let _cfg = Config::load(&args)?;
        Ok(())
    }

    #[test]
    fn test_main_config_api_keys_split_on_commas() {
        // Regression: the main config pushed the whole string as ONE key, so
        // `vt_api_key = "k1,k2"` produced a single key literally named "k1,k2"
        // that simply fails to authenticate. The env vars and the
        // provider-config file both split on commas; this layer now agrees.
        let mut config = Config::default();
        config.provider.vt_api_key = Some("k1, k2 , ,k3".to_string());
        config.provider.urlscan_api_key = Some("us1,us2".to_string());
        config.provider.zoomeye_api_key = Some("ze1".to_string());
        config.provider.github_api_key = Some("gh1,gh2".to_string());

        let mut args = <Args as clap::Parser>::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args);

        assert_eq!(args.vt_api_key, vec!["k1", "k2", "k3"]);
        assert_eq!(args.urlscan_api_key, vec!["us1", "us2"]);
        assert_eq!(args.zoomeye_api_key, vec!["ze1"]);
        assert_eq!(args.github_api_key, vec!["gh1", "gh2"]);
    }

    #[test]
    fn test_main_config_supplies_github_key() {
        // `--all-providers` documents keyed providers as activating from "flag,
        // env, or config file", but github had no config field at all — so the
        // only keyed provider added after that text was written could not be
        // configured from either config file.
        let toml_src = r#"
            [provider]
            github_api_key = "ghp_one,ghp_two"
        "#;
        let cfg: Config = toml::from_str(toml_src).expect("github_api_key must parse");
        assert_eq!(
            cfg.provider.github_api_key.as_deref(),
            Some("ghp_one,ghp_two")
        );

        let keys: ProviderKeysConfig =
            toml::from_str(r#"github_api_key = "ghp_three""#).expect("provider-config too");
        assert_eq!(keys.github_api_key.as_deref(), Some("ghp_three"));
    }

    #[test]
    fn test_provider_config_github_key_beats_main_config() {
        // Same precedence the other three keys follow.
        let mut config = Config::default();
        config.provider.github_api_key = Some("from-main".to_string());
        let mut args = <Args as clap::Parser>::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args);
        assert_eq!(args.github_api_key, vec!["from-main"]);

        let keys = ProviderKeysConfig {
            vt_api_key: None,
            urlscan_api_key: None,
            zoomeye_api_key: None,
            github_api_key: Some("from-provider-config".to_string()),
        };
        keys.apply_to_args(&mut args, false, false, false, false);
        assert_eq!(args.github_api_key, vec!["from-provider-config"]);

        // ...but a CLI-supplied key still wins.
        keys.apply_to_args(&mut args, false, false, false, true);
        assert_eq!(args.github_api_key, vec!["from-provider-config"]);
    }

    #[test]
    fn test_invalid_cache_type_in_config_is_reported_not_silently_taken() {
        // [output].format and [network].network_scope are both validated where
        // they're read; cache_type was not, so a typo surfaced much later as
        // "Unknown cache type" with no mention of the config file.
        let mut config = Config::default();
        config.cache.cache_type = Some("postgres".to_string());
        let mut args = <Args as clap::Parser>::parse_from(["urx", "--silent", "example.com"]);
        config.apply_to_args(&mut args);
        assert_eq!(args.cache_type, "sqlite", "invalid value must be ignored");

        // A valid value still applies, case-insensitively.
        let mut config = Config::default();
        config.cache.cache_type = Some("Redis".to_string());
        let mut args = <Args as clap::Parser>::parse_from(["urx", "--silent", "example.com"]);
        config.apply_to_args(&mut args);
        assert_eq!(args.cache_type, "redis");
    }

    #[test]
    fn test_provider_keys_load_succeeds_without_explicit_file() -> Result<()> {
        let args = Args::parse_from(["urx", "example.com"]);
        let _cfg = ProviderKeysConfig::load(&args)?;
        Ok(())
    }

    #[test]
    fn test_provider_keys_apply_to_args_respects_cli_supplied() {
        let cfg = ProviderKeysConfig {
            vt_api_key: Some("from-file".to_string()),
            urlscan_api_key: Some("us-from-file".to_string()),
            zoomeye_api_key: None,
            github_api_key: None,
        };
        let mut args = <Args as clap::Parser>::parse_from(["urx", "example.com"]);
        // Pretend the user supplied vt via CLI: provider-config should NOT
        // overwrite that.
        args.vt_api_key = vec!["cli-key".to_string()];

        cfg.apply_to_args(&mut args, true, false, false, false);

        assert_eq!(args.vt_api_key, vec!["cli-key".to_string()]);
        // urlscan was empty and not CLI-supplied -> file value applies and
        // is split on commas.
        assert_eq!(args.urlscan_api_key, vec!["us-from-file".to_string()]);
        // zoomeye not supplied anywhere -> stays empty.
        assert!(args.zoomeye_api_key.is_empty());
    }

    #[test]
    fn test_provider_keys_apply_to_args_splits_csv() {
        let cfg = ProviderKeysConfig {
            vt_api_key: Some("k1, k2 , ,k3".to_string()),
            urlscan_api_key: None,
            zoomeye_api_key: None,
            github_api_key: None,
        };
        let mut args = <Args as clap::Parser>::parse_from(["urx", "example.com"]);
        cfg.apply_to_args(&mut args, false, false, false, false);
        assert_eq!(args.vt_api_key, vec!["k1", "k2", "k3"]);
    }

    #[test]
    fn test_archive_filters_load_from_config_file() {
        let content = r#"
            [provider]
            from = "2020"
            to = "2021"
            archive_status = ["200"]
            archive_exclude_status = ["404", "500"]
            archive_mime = ["application/json"]
            archive_exclude_mime = ["text/html"]
        "#;
        let file = create_temp_config_file(content);
        let config = Config::from_file(file.path()).unwrap();

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args);

        assert_eq!(args.from.as_deref(), Some("2020"));
        assert_eq!(args.to.as_deref(), Some("2021"));
        assert_eq!(args.archive_status, vec!["200"]);
        assert_eq!(args.archive_exclude_status, vec!["404", "500"]);
        assert_eq!(args.archive_mime, vec!["application/json"]);
        assert_eq!(args.archive_exclude_mime, vec!["text/html"]);
    }

    #[test]
    fn test_cli_archive_filters_beat_config_file() {
        let content = r#"
            [provider]
            from = "2020"
            archive_status = ["200"]
        "#;
        let file = create_temp_config_file(content);
        let config = Config::from_file(file.path()).unwrap();

        let mut args = Args::parse_from([
            "urx",
            "--from",
            "2015",
            "--archive-status",
            "404",
            "example.com",
        ]);
        config.apply_to_args(&mut args);

        assert_eq!(args.from.as_deref(), Some("2015"));
        assert_eq!(args.archive_status, vec!["404"]);
    }
}
