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

        if args.format == "plain" && self.output.format.is_some() {
            args.format = self.output.format.unwrap();
        }

        if !args.merge_endpoint && self.output.merge_endpoint.unwrap_or(false) {
            args.merge_endpoint = true;
        }

        // Provider options
        if args.providers == vec!["wayback", "cc", "otx"] && self.provider.providers.is_some() {
            args.providers = self.provider.providers.unwrap();
        }

        if !args.subs && self.provider.subs.unwrap_or(false) {
            args.subs = true;
        }

        if args.cc_index == "CC-MAIN-2025-08" && self.provider.cc_index.is_some() {
            args.cc_index = self.provider.cc_index.unwrap();
        }

        if args.vt_api_key.is_none() && self.provider.vt_api_key.is_some() {
            args.vt_api_key = self.provider.vt_api_key;
        }

        if args.urlscan_api_key.is_none() && self.provider.urlscan_api_key.is_some() {
            args.urlscan_api_key = self.provider.urlscan_api_key;
        }

        // Filter options
        if args.preset.is_empty() && self.filter.preset.is_some() {
            args.preset = self.filter.preset.unwrap();
        }

        if args.extensions.is_empty() && self.filter.extensions.is_some() {
            args.extensions = self.filter.extensions.unwrap();
        }

        if args.exclude_extensions.is_empty() && self.filter.exclude_extensions.is_some() {
            args.exclude_extensions = self.filter.exclude_extensions.unwrap();
        }

        if args.patterns.is_empty() && self.filter.patterns.is_some() {
            args.patterns = self.filter.patterns.unwrap();
        }

        if args.exclude_patterns.is_empty() && self.filter.exclude_patterns.is_some() {
            args.exclude_patterns = self.filter.exclude_patterns.unwrap();
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
        if args.network_scope == "all" && self.network.network_scope.is_some() {
            args.network_scope = self.network.network_scope.unwrap();
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

        if args.timeout == 30 && self.network.timeout.is_some() {
            args.timeout = self.network.timeout.unwrap();
        }

        if args.retries == 3 && self.network.retries.is_some() {
            args.retries = self.network.retries.unwrap();
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

        if args.include_status.is_empty() && self.testing.include_status.is_some() {
            args.include_status = self.testing.include_status.unwrap();
        }

        if args.exclude_status.is_empty() && self.testing.exclude_status.is_some() {
            args.exclude_status = self.testing.exclude_status.unwrap();
        }

        if !args.extract_links && self.testing.extract_links.unwrap_or(false) {
            args.extract_links = true;
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
            config: None,
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            providers: vec!["wayback".to_string(), "cc".to_string(), "otx".to_string()],
            subs: false,
            cc_index: "CC-MAIN-2025-08".to_string(),
            vt_api_key: None,
            urlscan_api_key: None,
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
            domains: vec![],
            extract_links: false,
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
