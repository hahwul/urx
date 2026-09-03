use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::{Args, CliProvided};

/// Keys a config section did not recognise.
///
/// Captured rather than dropped so a typo can be reported. serde ignores
/// unknown fields by default, which made a misspelled key — or worse, a
/// misspelled *section* like `[filters]` — completely inert: every setting
/// underneath it silently never applied.
pub type UnknownKeys = std::collections::BTreeMap<String, toml::Value>;

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

    #[serde(default)]
    pub notify: NotifyConfig,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
}

#[derive(Debug, Deserialize, Default)]
pub struct OutputConfig {
    pub output: Option<String>,
    pub format: Option<String>,
    pub merge_endpoint: Option<bool>,
    pub dedup_similar: Option<bool>,
    pub stream: Option<bool>,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
}

#[derive(Debug, Deserialize, Default)]
pub struct ProviderConfig {
    pub providers: Option<Vec<String>>,
    pub subs: Option<bool>,
    pub cc_index: Option<String>,
    pub cdx_endpoint: Option<Vec<String>>,
    pub cdx_dialect: Option<String>,
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
    pub bevigil_api_key: Option<String>,
    pub include_robots: Option<bool>,
    pub include_sitemap: Option<bool>,
    pub exclude_robots: Option<bool>,
    pub exclude_sitemap: Option<bool>,
    pub archived_discovery: Option<bool>,
    pub archived_discovery_limit: Option<usize>,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
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
    pub bevigil_api_key: Option<String>,
    /// Webhook URL(s) for `--notify`, comma-separated. Lives here as well as
    /// in `[notify].url` because the URL *is* the credential, and this file is
    /// the one meant to stay out of source control.
    pub notify_url: Option<String>,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
}

/// Render one unrecognised entry for the warning: a whole unknown table is
/// shown in section form (`[filters]`) so it's obvious the entire block is
/// inert, anything else as a plain key path.
fn describe_unknown(section: Option<&str>, key: &str, value: &toml::Value) -> String {
    match section {
        Some(section) => format!("{section}.{key}"),
        None if value.is_table() => format!("[{key}]"),
        None => key.to_string(),
    }
}

/// Warn once about every config key urx did not recognise.
///
/// Silence here is expensive: a config whose section is spelled `[filters]`
/// instead of `[filter]` parses cleanly and then does nothing at all, which
/// reads as "the filter matched everything".
fn warn_about_unknown_keys(unknown: &[String], source: &str, silent: bool) {
    if unknown.is_empty() || silent {
        return;
    }
    eprintln!(
        "Warning: ignoring unrecognised {} in {source}: {}. Check for a typo — settings under an unknown key have no effect.",
        if unknown.len() == 1 { "key" } else { "keys" },
        unknown.join(", ")
    );
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
    /// Every key in the provider-config file urx does not recognise, sorted.
    pub fn unknown_keys(&self) -> Vec<String> {
        self.unknown
            .iter()
            .map(|(k, v)| describe_unknown(None, k, v))
            .collect()
    }

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
    /// `supplied` carries the original CLI state captured BEFORE either
    /// config layer ran, so CLI input is preserved.
    pub fn apply_to_args(&self, args: &mut Args, supplied: CliSuppliedKeys) {
        warn_about_unknown_keys(
            &self.unknown_keys(),
            "the provider-config file",
            args.silent,
        );

        if !supplied.notify {
            if let Some(urls) = &self.notify_url {
                let urls = split_csv(urls);
                if !urls.is_empty() {
                    args.notify = urls;
                }
            }
        }

        if !supplied.vt {
            if let Some(keys) = &self.vt_api_key {
                args.vt_api_key = split_csv(keys);
            }
        }
        if !supplied.urlscan {
            if let Some(keys) = &self.urlscan_api_key {
                args.urlscan_api_key = split_csv(keys);
            }
        }
        if !supplied.zoomeye {
            if let Some(keys) = &self.zoomeye_api_key {
                args.zoomeye_api_key = split_csv(keys);
            }
        }
        if !supplied.github {
            if let Some(keys) = &self.github_api_key {
                args.github_api_key = split_csv(keys);
            }
        }
        if !supplied.bevigil {
            if let Some(keys) = &self.bevigil_api_key {
                args.bevigil_api_key = split_csv(keys);
            }
        }
    }
}

/// Which API-key (and webhook) slots the CLI or environment already filled,
/// captured before either config layer runs. [`ProviderKeysConfig::apply_to_args`]
/// only overwrites a slot when its flag here is `false` — otherwise CLI/env
/// input would be silently replaced by the provider-config file.
///
/// A named struct instead of six positional bools: the six-argument form hit
/// clippy's `too_many_arguments` the moment BeVigil support added a sixth key,
/// and positional bools are error-prone at the call site regardless.
#[derive(Debug, Clone, Copy, Default)]
pub struct CliSuppliedKeys {
    pub vt: bool,
    pub urlscan: bool,
    pub zoomeye: bool,
    pub github: bool,
    pub bevigil: bool,
    pub notify: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct FilterConfig {
    pub preset: Option<Vec<String>>,
    pub extensions: Option<Vec<String>>,
    pub exclude_extensions: Option<Vec<String>>,
    pub patterns: Option<Vec<String>>,
    pub exclude_patterns: Option<Vec<String>>,
    pub match_regex: Option<Vec<String>>,
    pub filter_regex: Option<Vec<String>>,
    pub show_only_host: Option<bool>,
    pub show_only_path: Option<bool>,
    pub show_only_param: Option<bool>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
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

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
}

#[derive(Debug, Deserialize, Default)]
pub struct TestingConfig {
    pub check_status: Option<bool>,
    pub include_status: Option<Vec<String>>,
    pub exclude_status: Option<Vec<String>>,
    pub extract_links: Option<bool>,
    pub extract_js_endpoints: Option<bool>,
    pub max_js_files: Option<usize>,
    pub archive_body: Option<bool>,
    pub archive_body_limit: Option<usize>,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
}

#[derive(Debug, Deserialize, Default)]
pub struct CacheConfig {
    pub incremental: Option<bool>,
    pub cache_type: Option<String>,
    pub cache_path: Option<String>,
    pub redis_url: Option<String>,
    pub cache_ttl: Option<u64>,
    pub no_cache: Option<bool>,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
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

/// A single string or a list of them, so `url = "..."` and `url = ["..."]`
/// both read naturally.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct NotifyConfig {
    /// Webhook URL or list of URLs (`--notify`).
    pub url: Option<OneOrMany>,
    /// `always`, `new`, or `never` (`--notify-on`).
    pub on: Option<String>,
    /// `slack`, `discord`, or `json` (`--notify-format`).
    pub format: Option<String>,

    /// Anything in this section urx does not know about. See [`UnknownKeys`].
    #[serde(flatten)]
    pub unknown: UnknownKeys,
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

    /// Every key in the file urx does not recognise, as `section.key` — or
    /// `[section]` for a whole unknown table. Sorted, so the warning is stable.
    pub fn unknown_keys(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .unknown
            .iter()
            .map(|(k, v)| describe_unknown(None, k, v))
            .collect();
        for (section, unknown) in [
            ("output", &self.output.unknown),
            ("provider", &self.provider.unknown),
            ("filter", &self.filter.unknown),
            ("network", &self.network.unknown),
            ("testing", &self.testing.unknown),
            ("cache", &self.cache.unknown),
            ("notify", &self.notify.unknown),
        ] {
            out.extend(
                unknown
                    .iter()
                    .map(|(k, v)| describe_unknown(Some(section), k, v)),
            );
        }
        out.sort();
        out
    }

    /// Apply configuration values to Args, respecting priority.
    ///
    /// `provided` names the options the user typed on the command line. Those
    /// always win: a flag whose value happens to equal the clap default (say
    /// an explicit `--format plain`) is still an explicit choice, and used to
    /// be silently replaced by the config file's value.
    pub fn apply_to_args(self, args: &mut Args, provided: &CliProvided) {
        warn_about_unknown_keys(&self.unknown_keys(), "the config file", args.silent);

        self.apply_output_config(args, provided);
        self.apply_provider_config(args, provided);
        self.apply_filter_config(args);
        self.apply_network_config(args, provided);
        self.apply_testing_config(args, provided);
        self.apply_cache_config(args, provided);
        self.apply_notify_config(args, provided);
    }

    fn apply_output_config(&self, args: &mut Args, provided: &CliProvided) {
        // Output options
        if args.output.is_none() {
            if let Some(output) = &self.output.output {
                args.output = Some(PathBuf::from(output));
            }
        }

        if !provided.has("format") {
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

        if !args.dedup_similar && self.output.dedup_similar.unwrap_or(false) {
            args.dedup_similar = true;
        }

        if !args.stream && self.output.stream.unwrap_or(false) {
            args.stream = true;
        }
    }

    fn apply_provider_config(&self, args: &mut Args, provided: &CliProvided) {
        // Provider options
        if !provided.has("providers") {
            if let Some(providers) = &self.provider.providers {
                args.providers = providers.clone();
            }
        }

        if !args.subs && self.provider.subs.unwrap_or(false) {
            args.subs = true;
        }

        // Config file still accepts a single string; we split it on commas so
        // users can configure multi-index there too.
        if !provided.has("cc_index") {
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

        // Extra CDX index servers, and the dialect they speak.
        if args.cdx_endpoint.is_empty() {
            if let Some(endpoints) = &self.provider.cdx_endpoint {
                args.cdx_endpoint = endpoints
                    .iter()
                    .map(|e| e.trim().to_string())
                    .filter(|e| !e.is_empty())
                    .collect();
            }
        }

        // An empty string is the documented "unset" spelling, same as `from`.
        if args.cdx_dialect.is_none() {
            if let Some(dialect) = self.provider.cdx_dialect.as_deref().map(str::trim) {
                if !dialect.is_empty() {
                    args.cdx_dialect = Some(dialect.to_string());
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

        if args.bevigil_api_key.is_empty() {
            if let Some(bevigil_api_key) = &self.provider.bevigil_api_key {
                args.bevigil_api_key = split_csv(bevigil_api_key);
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

        if !args.archived_discovery && self.provider.archived_discovery.unwrap_or(false) {
            args.archived_discovery = true;
        }

        if !provided.has("archived_discovery_limit") {
            if let Some(limit) = self.provider.archived_discovery_limit {
                args.archived_discovery_limit = limit;
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

        if args.match_regex.is_empty() {
            if let Some(match_regex) = &self.filter.match_regex {
                args.match_regex = match_regex.clone();
            }
        }

        if args.filter_regex.is_empty() {
            if let Some(filter_regex) = &self.filter.filter_regex {
                args.filter_regex = filter_regex.clone();
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

    fn apply_network_config(&self, args: &mut Args, provided: &CliProvided) {
        // Network options
        if !provided.has("network_scope") {
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

        if !provided.has("timeout") {
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

        if !provided.has("retries") {
            if let Some(retries) = self.network.retries {
                args.retries = retries;
            }
        }

        if !provided.has("parallel") {
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

    fn apply_testing_config(&self, args: &mut Args, provided: &CliProvided) {
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

        if !args.extract_js_endpoints && self.testing.extract_js_endpoints.unwrap_or(false) {
            args.extract_js_endpoints = true;
        }

        if !provided.has("max_js_files") {
            if let Some(max) = self.testing.max_js_files {
                args.max_js_files = max;
            }
        }

        if !args.archive_body && self.testing.archive_body.unwrap_or(false) {
            args.archive_body = true;
        }

        if !provided.has("archive_body_limit") {
            if let Some(limit) = self.testing.archive_body_limit {
                args.archive_body_limit = limit;
            }
        }
    }

    fn apply_cache_config(&self, args: &mut Args, provided: &CliProvided) {
        // Cache options
        if !args.incremental && self.cache.incremental.unwrap_or(false) {
            args.incremental = true;
        }

        if !provided.has("cache_type") {
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

        if !provided.has("cache_ttl") {
            if let Some(cache_ttl) = self.cache.cache_ttl {
                args.cache_ttl = cache_ttl;
            }
        }

        if !args.no_cache && self.cache.no_cache.unwrap_or(false) {
            args.no_cache = true;
        }
    }

    fn apply_notify_config(&self, args: &mut Args, provided: &CliProvided) {
        // The URL list is filled from the CLI *or* URX_NOTIFY_URL before the
        // config layers run, and the two are indistinguishable afterwards —
        // so "still empty" is the test, not `provided.has`.
        if args.notify.is_empty() {
            if let Some(urls) = &self.notify.url {
                let urls: Vec<String> = urls
                    .clone()
                    .into_vec()
                    .into_iter()
                    .map(|u| u.trim().to_string())
                    .filter(|u| !u.is_empty())
                    .collect();
                if !urls.is_empty() {
                    args.notify = urls;
                }
            }
        }

        if !provided.has("notify_on") {
            if let Some(on) = &self.notify.on {
                match <crate::notify::NotifyOn as clap::ValueEnum>::from_str(on.trim(), true) {
                    Ok(value) => args.notify_on = value,
                    Err(_) => {
                        if !args.silent {
                            eprintln!(
                                "Ignoring [notify].on={on:?} in config: expected always, new, or never"
                            );
                        }
                    }
                }
            }
        }

        if !provided.has("notify_format") {
            if let Some(format) = &self.notify.format {
                match <crate::notify::NotifyFormat as clap::ValueEnum>::from_str(
                    format.trim(),
                    true,
                ) {
                    Ok(value) => args.notify_format = value,
                    Err(_) => {
                        if !args.silent {
                            eprintln!(
                                "Ignoring [notify].format={format:?} in config: expected slack, discord, or json"
                            );
                        }
                    }
                }
            }
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
    use crate::cli::parse_args_from;
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
        config.apply_to_args(&mut args, &CliProvided::default());

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
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.timeout, 120);
        assert_eq!(args.parallel, Some(5));
    }

    #[test]
    fn test_apply_to_args_ignores_invalid_output_format_and_network_scope() {
        let mut config = Config::default();
        config.output.format = Some("yaml".to_string());
        config.network.network_scope = Some("providers only".to_string());

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.format, "plain");
        assert_eq!(args.network_scope, "all");
    }

    #[test]
    fn test_apply_to_args_normalizes_output_format_and_network_scope() {
        let mut config = Config::default();
        config.output.format = Some("JSON".to_string());
        config.network.network_scope = Some("TESTERS,PROVIDERS".to_string());

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());

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
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.vt_api_key, vec!["k1", "k2", "k3"]);
        assert_eq!(args.urlscan_api_key, vec!["us1", "us2"]);
        assert_eq!(args.zoomeye_api_key, vec!["ze1"]);
        assert_eq!(args.github_api_key, vec!["gh1", "gh2"]);
    }

    #[test]
    fn test_config_supplies_cdx_endpoints_and_dialect() {
        let toml_src = r#"
            [provider]
            cdx_endpoint = ["https://vefsafn.is/cdx", " http://localhost:8080/cdx ", ""]
            cdx_dialect = "classic"
        "#;
        let config: Config = toml::from_str(toml_src).unwrap();
        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());
        assert_eq!(
            args.cdx_endpoint,
            vec!["https://vefsafn.is/cdx", "http://localhost:8080/cdx"]
        );
        assert_eq!(args.cdx_dialect.as_deref(), Some("classic"));

        // The CLI wins over the file.
        let config: Config = toml::from_str(toml_src).unwrap();
        let mut args = Args::parse_from([
            "urx",
            "--cdx-endpoint",
            "https://example.org/cdx",
            "--cdx-dialect",
            "pywb",
            "example.com",
        ]);
        config.apply_to_args(&mut args, &CliProvided::default());
        assert_eq!(args.cdx_endpoint, vec!["https://example.org/cdx"]);
        assert_eq!(args.cdx_dialect.as_deref(), Some("pywb"));

        // An empty dialect string, as in the documented template, is unset.
        let config: Config = toml::from_str("[provider]\ncdx_dialect = \"\"").unwrap();
        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());
        assert!(args.cdx_dialect.is_none());
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
        config.apply_to_args(&mut args, &CliProvided::default());
        assert_eq!(args.github_api_key, vec!["from-main"]);

        let keys = ProviderKeysConfig {
            vt_api_key: None,
            urlscan_api_key: None,
            zoomeye_api_key: None,
            github_api_key: Some("from-provider-config".to_string()),
            bevigil_api_key: None,
            notify_url: None,
            unknown: Default::default(),
        };
        keys.apply_to_args(&mut args, CliSuppliedKeys::default());
        assert_eq!(args.github_api_key, vec!["from-provider-config"]);

        // ...but a CLI-supplied key still wins.
        keys.apply_to_args(
            &mut args,
            CliSuppliedKeys {
                github: true,
                ..Default::default()
            },
        );
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
        config.apply_to_args(&mut args, &CliProvided::default());
        assert_eq!(args.cache_type, "sqlite", "invalid value must be ignored");

        // A valid value still applies, case-insensitively.
        let mut config = Config::default();
        config.cache.cache_type = Some("Redis".to_string());
        let mut args = <Args as clap::Parser>::parse_from(["urx", "--silent", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());
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
            bevigil_api_key: None,
            notify_url: None,
            unknown: Default::default(),
        };
        let mut args = <Args as clap::Parser>::parse_from(["urx", "example.com"]);
        // Pretend the user supplied vt via CLI: provider-config should NOT
        // overwrite that.
        args.vt_api_key = vec!["cli-key".to_string()];

        cfg.apply_to_args(
            &mut args,
            CliSuppliedKeys {
                vt: true,
                ..Default::default()
            },
        );

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
            bevigil_api_key: None,
            notify_url: None,
            unknown: Default::default(),
        };
        let mut args = <Args as clap::Parser>::parse_from(["urx", "example.com"]);
        cfg.apply_to_args(&mut args, CliSuppliedKeys::default());
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
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.from.as_deref(), Some("2020"));
        assert_eq!(args.to.as_deref(), Some("2021"));
        assert_eq!(args.archive_status, vec!["200"]);
        assert_eq!(args.archive_exclude_status, vec!["404", "500"]);
        assert_eq!(args.archive_mime, vec!["application/json"]);
        assert_eq!(args.archive_exclude_mime, vec!["text/html"]);
    }

    #[test]
    fn test_explicit_cli_flag_wins_even_when_it_equals_the_default() {
        // Regression: precedence used to be decided by "does this field still
        // equal its clap default?", which cannot see the difference between
        // `--format plain` and no `--format` at all. Every option whose default
        // a user might legitimately type back was therefore silently
        // overridden by the config file — `urx --format plain` printed JSON.
        let content = r#"
            [output]
            format = "json"

            [provider]
            providers = ["arquivo"]
            cc_index = "CC-MAIN-2020-05"

            [network]
            network_scope = "testers"
            timeout = 30
            retries = 9
            parallel = 2

            [cache]
            cache_type = "redis"
            cache_ttl = 60
        "#;
        let file = create_temp_config_file(content);

        let argv = [
            "urx",
            "--format",
            "plain",
            "--providers",
            "wayback,cc,otx",
            "--cc-index",
            "latest",
            "--network-scope",
            "all",
            "--timeout",
            "120",
            "--retries",
            "2",
            "--parallel",
            "5",
            "--cache-type",
            "sqlite",
            "--cache-ttl",
            "86400",
            "example.com",
        ];
        let (mut args, provided) = crate::cli::parse_args_from(argv);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);

        assert_eq!(args.format, "plain");
        assert_eq!(args.providers, vec!["wayback", "cc", "otx"]);
        assert_eq!(args.cc_index, vec!["latest"]);
        assert_eq!(args.network_scope, "all");
        assert_eq!(args.timeout, 120);
        assert_eq!(args.retries, 2);
        assert_eq!(args.parallel, Some(5));
        assert_eq!(args.cache_type, "sqlite");
        assert_eq!(args.cache_ttl, 86400);
    }

    #[test]
    fn test_config_still_applies_when_the_flag_was_not_supplied() {
        // The other half of the contract: without the flag, the file wins.
        let content = r#"
            [output]
            format = "json"

            [provider]
            providers = ["arquivo"]
            cc_index = "CC-MAIN-2020-05"

            [network]
            network_scope = "testers"
            timeout = 30
            retries = 9
            parallel = 2

            [cache]
            cache_type = "redis"
            cache_ttl = 60
        "#;
        let file = create_temp_config_file(content);

        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);

        assert_eq!(args.format, "json");
        assert_eq!(args.providers, vec!["arquivo"]);
        assert_eq!(args.cc_index, vec!["CC-MAIN-2020-05"]);
        assert_eq!(args.network_scope, "testers");
        assert_eq!(args.timeout, 30);
        assert_eq!(args.retries, 9);
        assert_eq!(args.parallel, Some(2));
        assert_eq!(args.cache_type, "redis");
        assert_eq!(args.cache_ttl, 60);
    }

    #[test]
    fn test_unknown_config_keys_and_sections_are_reported() {
        // Regression: serde drops unknown fields in silence, so a misspelled
        // key — or a whole misspelled section like `[filters]` — parsed
        // cleanly and then did absolutely nothing. Users read the resulting
        // unfiltered output as "the filter matched everything".
        let content = r#"
            stray = 1

            [output]
            fromat = "json"

            [filters]
            extensions = ["js"]

            [provider]
            provdiers = ["wayback"]

            [network]
            rate_limit = 5
        "#;
        let config = Config::from_file(create_temp_config_file(content).path()).unwrap();

        assert_eq!(
            config.unknown_keys(),
            vec![
                "[filters]".to_string(),
                "output.fromat".to_string(),
                "provider.provdiers".to_string(),
                "stray".to_string(),
            ]
        );
        // ...and capturing them must not disturb ordinary parsing, including
        // the integer-to-float widening TOML does for `rate_limit`.
        assert_eq!(config.network.rate_limit, Some(5.0));
    }

    #[test]
    fn test_known_config_keys_are_not_reported_as_unknown() {
        let content = r#"
            [output]
            output = "out.txt"
            format = "json"
            merge_endpoint = true
            stream = false

            [provider]
            providers = ["wayback"]
            subs = true
            cc_index = "latest"
            from = "2020"
            to = "2021"
            vt_api_key = "k"
            include_robots = false
            archived_discovery = true
            archived_discovery_limit = 20

            [filter]
            preset = ["no-images"]
            extensions = ["js"]
            min_length = 5
            max_length = 500

            [network]
            network_scope = "all"
            proxy = "http://p:1"
            insecure = true
            timeout = 10
            retries = 1
            parallel = 2
            rate_limit = 1.5

            [testing]
            check_status = true
            include_status = ["200"]
            extract_js_endpoints = true
            max_js_files = 42
            archive_body = true
            archive_body_limit = 100

            [cache]
            incremental = true
            cache_type = "sqlite"
            cache_path = "/tmp/x.db"
            cache_ttl = 10
            no_cache = false
        "#;
        let config = Config::from_file(create_temp_config_file(content).path()).unwrap();
        assert!(
            config.unknown_keys().is_empty(),
            "{:?}",
            config.unknown_keys()
        );
    }

    #[test]
    fn test_unknown_provider_config_keys_are_reported() {
        let content = r#"
            vt_api_key = "abc"
            vt_apikey = "typo"

            [provider]
            zoomeye_api_key = "z"
        "#;
        let cfg = ProviderKeysConfig::from_file(create_temp_config_file(content).path()).unwrap();
        assert_eq!(cfg.vt_api_key.as_deref(), Some("abc"));
        let mut unknown = cfg.unknown_keys();
        unknown.sort();
        assert_eq!(
            unknown,
            vec!["[provider]".to_string(), "vt_apikey".to_string()]
        );
    }

    #[test]
    fn test_js_endpoint_options_apply_from_config_unless_given_on_the_cli() {
        let content = r#"
            [testing]
            extract_js_endpoints = true
            max_js_files = 42
        "#;
        let file = create_temp_config_file(content);

        // Nothing on the CLI: both keys come from the file.
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &provided);
        assert!(args.extract_js_endpoints);
        assert_eq!(args.max_js_files, 42);

        // An explicit --max-js-files wins even when it equals the default.
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) =
            crate::cli::parse_args_from(["urx", "--max-js-files", "500", "example.com"]);
        config.apply_to_args(&mut args, &provided);
        assert_eq!(args.max_js_files, 500);
    }

    #[test]
    fn test_archive_body_settings_load_from_config_and_yield_to_the_cli() {
        let content = r#"
            [testing]
            archive_body = true
            archive_body_limit = 42
        "#;
        let file = create_temp_config_file(content);

        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);
        assert!(args.archive_body);
        assert_eq!(args.archive_body_limit, 42);

        // An explicit limit on the command line wins even when it equals the
        // clap default.
        let (mut args, provided) =
            crate::cli::parse_args_from(["urx", "--archive-body-limit", "500", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);
        assert_eq!(args.archive_body_limit, 500);
    }

    #[test]
    fn test_archived_discovery_settings_load_from_config_and_yield_to_the_cli() {
        let content = r#"
            [provider]
            archived_discovery = true
            archived_discovery_limit = 7
        "#;
        let file = create_temp_config_file(content);

        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);
        assert!(args.archived_discovery);
        assert_eq!(args.archived_discovery_limit, 7);

        let (mut args, provided) =
            crate::cli::parse_args_from(["urx", "--archived-discovery-limit", "50", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);
        assert_eq!(args.archived_discovery_limit, 50);
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
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.from.as_deref(), Some("2015"));
        assert_eq!(args.archive_status, vec!["404"]);
    }

    #[test]
    fn test_notify_section_loads_and_applies() {
        use crate::notify::{NotifyFormat, NotifyOn};

        let content = r#"
            [notify]
            url = ["https://hooks.example/a", " https://hooks.example/b "]
            on = "Always"
            format = "discord"
        "#;
        let file = create_temp_config_file(content);
        let config = Config::from_file(file.path()).unwrap();
        assert!(
            config.unknown_keys().is_empty(),
            "{:?}",
            config.unknown_keys()
        );

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(
            args.notify,
            vec!["https://hooks.example/a", "https://hooks.example/b"]
        );
        assert_eq!(args.notify_on, NotifyOn::Always);
        assert_eq!(args.notify_format, NotifyFormat::Discord);
    }

    #[test]
    fn test_notify_url_accepts_a_single_string() {
        let content = r#"
            [notify]
            url = "https://hooks.example/one"
        "#;
        let file = create_temp_config_file(content);
        let config = Config::from_file(file.path()).unwrap();

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());
        assert_eq!(args.notify, vec!["https://hooks.example/one"]);
    }

    #[test]
    fn test_notify_cli_beats_config() {
        use crate::notify::{NotifyFormat, NotifyOn};

        let content = r#"
            [notify]
            url = "https://hooks.example/from-config"
            on = "always"
            format = "slack"
        "#;
        let file = create_temp_config_file(content);
        let config = Config::from_file(file.path()).unwrap();

        // Explicit flags — including ones that spell the clap default back —
        // survive the config layer.
        let (mut args, provided) = parse_args_from([
            "urx",
            "--notify",
            "https://hooks.example/from-cli",
            "--notify-on",
            "new",
            "--notify-format",
            "json",
            "example.com",
        ]);
        config.apply_to_args(&mut args, &provided);

        assert_eq!(args.notify, vec!["https://hooks.example/from-cli"]);
        assert_eq!(args.notify_on, NotifyOn::New);
        assert_eq!(args.notify_format, NotifyFormat::Json);
    }

    #[test]
    fn test_notify_invalid_values_are_ignored_not_fatal() {
        use crate::notify::{NotifyFormat, NotifyOn};

        let mut config = Config::default();
        config.notify.on = Some("sometimes".to_string());
        config.notify.format = Some("teams".to_string());

        let mut args = Args::parse_from(["urx", "--silent", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.notify_on, NotifyOn::New);
        assert_eq!(args.notify_format, NotifyFormat::Json);
    }

    #[test]
    fn test_notify_unknown_key_is_reported() {
        let content = r#"
            [notify]
            urls = "https://hooks.example/typo"
        "#;
        let file = create_temp_config_file(content);
        let config = Config::from_file(file.path()).unwrap();
        assert_eq!(config.unknown_keys(), vec!["notify.urls"]);
    }

    #[test]
    fn test_provider_config_notify_url_beats_main_config_but_not_cli() {
        let mut main = Config::default();
        main.notify.url = Some(OneOrMany::One("https://hooks.example/main".to_string()));

        let keys = ProviderKeysConfig {
            vt_api_key: None,
            urlscan_api_key: None,
            zoomeye_api_key: None,
            github_api_key: None,
            bevigil_api_key: None,
            notify_url: Some("https://hooks.example/p1, https://hooks.example/p2".to_string()),
            unknown: Default::default(),
        };

        // Nothing on the CLI: provider-config overrides the main config.
        let mut args = Args::parse_from(["urx", "example.com"]);
        main.apply_to_args(&mut args, &CliProvided::default());
        assert_eq!(args.notify, vec!["https://hooks.example/main"]);
        keys.apply_to_args(&mut args, CliSuppliedKeys::default());
        assert_eq!(
            args.notify,
            vec!["https://hooks.example/p1", "https://hooks.example/p2"]
        );

        // CLI (or env) supplied: provider-config yields.
        let mut args = Args::parse_from([
            "urx",
            "--notify",
            "https://hooks.example/cli",
            "example.com",
        ]);
        keys.apply_to_args(
            &mut args,
            CliSuppliedKeys {
                notify: true,
                ..Default::default()
            },
        );
        assert_eq!(args.notify, vec!["https://hooks.example/cli"]);
    }
}
