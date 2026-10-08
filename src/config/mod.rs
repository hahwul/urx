use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app::keys::{api_keys_mut, KEYED_PROVIDER_IDS};
use crate::cli::{Args, CliProvided};
use crate::utils::split_csv;

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

    /// Default lookup path mirrors the main config: ~/.config/urx
    /// (`$HOME/.config`; `$XDG_CONFIG_HOME` is not consulted) or
    /// %APPDATA%\urx. Returns None when neither exists; unlike `Config`, we
    /// do NOT auto-create the file because that would land an empty
    /// "credentials" path the user didn't ask for.
    pub fn default_path() -> Option<PathBuf> {
        Some(urx_config_dir()?.join("provider-config.toml")).filter(|p| p.exists())
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

        let configured = [
            &self.vt_api_key,
            &self.urlscan_api_key,
            &self.zoomeye_api_key,
            &self.github_api_key,
            &self.bevigil_api_key,
        ];
        for (id, keys) in KEYED_PROVIDER_IDS.into_iter().zip(configured) {
            if supplied.api_keys.contains(&id) {
                continue;
            }
            // An empty placeholder (`vt_api_key = ""`) is no key, not an
            // instruction to clear the one the main config just set.
            let keys = keys.as_deref().map(split_csv).unwrap_or_default();
            if !keys.is_empty() {
                *api_keys_mut(args, id) = keys;
            }
        }
    }
}

/// Which API-key (and webhook) slots the CLI or environment already filled,
/// captured before either config layer runs. [`ProviderKeysConfig::apply_to_args`]
/// only overwrites a slot when its flag here is `false` — otherwise CLI/env
/// input would be silently replaced by the provider-config file.
///
#[derive(Debug, Clone, Default)]
pub struct CliSuppliedKeys {
    /// Keyed provider ids whose `--<id>-api-key` slot is already filled.
    pub api_keys: Vec<&'static str>,
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

    // --- result-filters ---
    pub scope_file: Option<Vec<std::path::PathBuf>>,
    pub meta_first_seen_after: Option<String>,
    pub meta_first_seen_before: Option<String>,
    pub meta_last_seen_after: Option<String>,
    pub meta_last_seen_before: Option<String>,
    pub meta_mime: Option<Vec<String>>,
    pub meta_exclude_mime: Option<Vec<String>>,
    pub meta_status: Option<Vec<String>>,
    pub meta_exclude_status: Option<Vec<String>>,
    // --- end result-filters ---
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
    /// `-H`: `header = ["Name: value", ...]`, or a single string.
    pub header: Option<OneOrMany>,
    pub cookie: Option<String>,
    pub user_agent: Option<String>,
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
    pub archive_body_dir: Option<PathBuf>,
    // --- spec-expansion ---
    pub expand_specs: Option<bool>,
    pub max_spec_files: Option<usize>,

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

impl Config {
    /// Load configuration from a specific file path
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.as_ref().display()))?;

        let output_views = [
            ("show_only_host", config.filter.show_only_host),
            ("show_only_path", config.filter.show_only_path),
            ("show_only_param", config.filter.show_only_param),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.unwrap_or(false).then_some(name))
        .collect::<Vec<_>>();
        if output_views.len() > 1 {
            anyhow::bail!(
                "Conflicting [filter] output views in {}: {} are mutually exclusive",
                path.as_ref().display(),
                output_views.join(", ")
            );
        }

        Ok(config)
    }

    /// Get the default configuration file path
    /// - Linux/macOS: ~/.config/urx/config.toml
    /// - Windows: %AppData%\urx\config.toml
    ///
    /// If the directory doesn't exist, it will be created.
    /// If the file doesn't exist, an empty config.toml file will be created.
    pub fn default_path() -> Option<PathBuf> {
        let config_dir = urx_config_dir()?;
        let config_path = config_dir.join("config.toml");

        // Create directory if it doesn't exist
        if !config_dir.exists() && fs::create_dir_all(&config_dir).is_err() {
            return None;
        }

        // Create empty config file if it doesn't exist
        if !config_path.exists() && fs::write(&config_path, "").is_err() {
            return None;
        }

        Some(config_path)
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
        let (o, p, f, n) = (&self.output, &self.provider, &self.filter, &self.network);
        let (t, c) = (&self.testing, &self.cache);

        // [output]
        args.output.fill(&o.output.as_ref().map(PathBuf::from));

        if !provided.has("format") {
            let expected = "plain, json, jsonl, csv, or wordlist";
            if let Some(v) = parse_enum(&o.format, "[output].format", expected, args.silent) {
                args.format = v;
            }
        }

        args.merge_endpoint.fill(&o.merge_endpoint);
        args.dedup_similar.fill(&o.dedup_similar);
        args.stream.fill(&o.stream);

        // [provider]
        fill_untyped(provided, "providers", &mut args.providers, &p.providers);
        args.subs.fill(&p.subs);

        // Config file still accepts a single string; we split it on commas so
        // users can configure multi-index there too.
        if !provided.has("cc_index") {
            if let Some(cc_index) = &p.cc_index {
                let split = split_csv(cc_index);
                if !split.is_empty() {
                    args.cc_index = split;
                }
            }
        }

        // Extra CDX index servers, and the dialect they speak.
        if args.cdx_endpoint.is_empty() {
            if let Some(endpoints) = &p.cdx_endpoint {
                args.cdx_endpoint = endpoints
                    .iter()
                    .map(|e| e.trim().to_string())
                    .filter(|e| !e.is_empty())
                    .collect();
            }
        }

        // An empty string is the documented "unset" spelling, same as `from`.
        if args.cdx_dialect.is_none() {
            let dialect = p.cdx_dialect.clone().filter(|d| !d.trim().is_empty());
            let expected = "classic or pywb";
            args.cdx_dialect =
                parse_enum(&dialect, "[provider].cdx_dialect", expected, args.silent);
        }

        // Archive-side CDX predicates.
        args.from.fill(&p.from);
        args.to.fill(&p.to);
        args.archive_status.fill(&p.archive_status);
        args.archive_exclude_status.fill(&p.archive_exclude_status);
        args.archive_mime.fill(&p.archive_mime);
        args.archive_exclude_mime.fill(&p.archive_exclude_mime);

        // API keys rotate when several are given, and every other source
        // separates them with commas: the env vars do, and so does the
        // provider-config file. The main config used to push the whole string as
        // a single key, so `vt_api_key = "k1,k2"` became one key literally named
        // "k1,k2" — which simply fails to authenticate, with no hint why.
        let configured = [
            &p.vt_api_key,
            &p.urlscan_api_key,
            &p.zoomeye_api_key,
            &p.github_api_key,
            &p.bevigil_api_key,
        ];
        for (id, keys) in KEYED_PROVIDER_IDS.into_iter().zip(configured) {
            api_keys_mut(args, id).fill(&keys.as_deref().map(split_csv));
        }

        // Explicit CLI choices take precedence over config, while exclusion
        // wins when both include and exclude are enabled at the same layer.
        if !provided.has("include_robots")
            && !provided.has("exclude_robots")
            && !args.exclude_robots
            && p.exclude_robots.unwrap_or(false)
        {
            args.exclude_robots = true;
        }

        if !provided.has("include_sitemap")
            && !provided.has("exclude_sitemap")
            && !args.exclude_sitemap
            && p.exclude_sitemap.unwrap_or(false)
        {
            args.exclude_sitemap = true;
        }

        // Only apply include_* if exclude_* is not set (exclude takes precedence)
        if !provided.has("include_robots") && !args.exclude_robots && args.include_robots {
            if let Some(include_robots) = p.include_robots {
                args.include_robots = include_robots;
            }
        }

        if !provided.has("include_sitemap") && !args.exclude_sitemap && args.include_sitemap {
            if let Some(include_sitemap) = p.include_sitemap {
                args.include_sitemap = include_sitemap;
            }
        }

        args.archived_discovery.fill(&p.archived_discovery);
        fill_untyped(
            provided,
            "archived_discovery_limit",
            &mut args.archived_discovery_limit,
            &p.archived_discovery_limit,
        );

        // [filter]
        args.preset.fill(&f.preset);
        args.extensions.fill(&f.extensions);
        args.exclude_extensions.fill(&f.exclude_extensions);
        args.patterns.fill(&f.patterns);
        args.exclude_patterns.fill(&f.exclude_patterns);
        args.match_regex.fill(&f.match_regex);
        args.filter_regex.fill(&f.filter_regex);

        // These flags select one output view. A view explicitly chosen on the
        // CLI must replace the configured view as a whole; otherwise a config
        // `show_only_host = true` silently strips the query before `--params`
        // can inventory it.
        let cli_selected_output_view = [
            "show_only_host",
            "show_only_path",
            "show_only_param",
            "params",
            "params_by_endpoint",
            "fuzz_placeholder",
        ]
        .iter()
        .any(|id| provided.has(id));
        if !cli_selected_output_view {
            args.show_only_host.fill(&f.show_only_host);
            args.show_only_path.fill(&f.show_only_path);
            args.show_only_param.fill(&f.show_only_param);
        }

        args.min_length.fill(&f.min_length);
        args.max_length.fill(&f.max_length);

        // --- result-filters ---
        args.scope_file.fill(&f.scope_file);
        args.meta_first_seen_after.fill(&f.meta_first_seen_after);
        args.meta_first_seen_before.fill(&f.meta_first_seen_before);
        args.meta_last_seen_after.fill(&f.meta_last_seen_after);
        args.meta_last_seen_before.fill(&f.meta_last_seen_before);
        args.meta_mime.fill(&f.meta_mime);
        args.meta_exclude_mime.fill(&f.meta_exclude_mime);
        args.meta_status.fill(&f.meta_status);
        args.meta_exclude_status.fill(&f.meta_exclude_status);
        // --- end result-filters ---

        // [network]
        if !provided.has("network_scope") {
            let (key, expected) = (
                "[network].network_scope",
                "all, providers, testers, or providers,testers",
            );
            if let Some(v) = parse_enum(&n.network_scope, key, expected, args.silent) {
                args.network_scope = v;
            }
        }

        args.proxy.fill(&n.proxy);
        args.proxy_auth.fill(&n.proxy_auth);
        args.insecure.fill(&n.insecure);
        args.random_agent.fill(&n.random_agent);

        // Headers are additive nowhere: a config file that sets them is a
        // default, and any -H on the command line replaces the set wholesale,
        // so a run can always be made anonymous again without editing a file.
        args.header.fill(&n.header.clone().map(OneOrMany::into_vec));
        args.cookie.fill(&n.cookie);
        args.user_agent.fill(&n.user_agent);

        if !provided.has("timeout") {
            if let Some(timeout) = n.timeout {
                if timeout > 0 {
                    args.timeout = timeout;
                } else if !args.silent {
                    eprintln!(
                        "Ignoring [network].timeout=0 in config: value must be at least 1 second"
                    );
                }
            }
        }

        fill_untyped(provided, "retries", &mut args.retries, &n.retries);

        if !provided.has("parallel") {
            if let Some(parallel) = n.parallel {
                if parallel > 0 {
                    args.parallel = parallel;
                } else if !args.silent {
                    eprintln!("Ignoring [network].parallel=0 in config: value must be at least 1");
                }
            }
        }

        args.rate_limit.fill(&n.rate_limit);

        // [testing]
        args.check_status.fill(&t.check_status);
        args.include_status.fill(&t.include_status);
        args.exclude_status.fill(&t.exclude_status);
        args.extract_links.fill(&t.extract_links);
        args.extract_js_endpoints.fill(&t.extract_js_endpoints);
        fill_untyped(
            provided,
            "max_js_files",
            &mut args.max_js_files,
            &t.max_js_files,
        );
        args.archive_body.fill(&t.archive_body);
        fill_untyped(
            provided,
            "archive_body_limit",
            &mut args.archive_body_limit,
            &t.archive_body_limit,
        );
        args.archive_body_dir.fill(&t.archive_body_dir);
        // --- spec-expansion ---
        args.expand_specs.fill(&t.expand_specs);
        fill_untyped(
            provided,
            "max_spec_files",
            &mut args.max_spec_files,
            &t.max_spec_files,
        );

        // [cache]
        args.incremental.fill(&c.incremental);

        if !provided.has("cache_type") {
            let expected = "sqlite or redis";
            if let Some(v) = parse_enum(&c.cache_type, "[cache].cache_type", expected, args.silent)
            {
                args.cache_type = v;
            }
        }

        args.cache_path
            .fill(&c.cache_path.as_ref().map(PathBuf::from));
        args.redis_url.fill(&c.redis_url);
        fill_untyped(provided, "cache_ttl", &mut args.cache_ttl, &c.cache_ttl);
        args.no_cache.fill(&c.no_cache);

        // [notify]
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

        let notify = &self.notify;
        if !provided.has("notify_on") {
            let expected = "always, new, or never";
            if let Some(v) = parse_enum(&notify.on, "[notify].on", expected, args.silent) {
                args.notify_on = v;
            }
        }

        if !provided.has("notify_format") {
            let expected = "slack, discord, or json";
            if let Some(v) = parse_enum(&notify.format, "[notify].format", expected, args.silent) {
                args.notify_format = v;
            }
        }
    }
}

/// Parse a configured value of a `--flag` enum the way clap would, ignoring
/// case. A bad value is named at the point it was read — with the config key
/// it came from — and ignored, rather than failing later with an error that
/// doesn't mention the config file at all.
fn parse_enum<T: ValueEnum>(
    raw: &Option<String>,
    key: &str,
    expected: &str,
    silent: bool,
) -> Option<T> {
    let raw = raw.as_ref()?;
    let parsed = T::from_str(raw.trim(), true).ok();
    if parsed.is_none() && !silent {
        eprintln!("Ignoring {key}={raw:?} in config: expected {expected}");
    }
    parsed
}

/// Fill a CLI slot from the config file only when the CLI left it empty,
/// unset or false — so CLI input always wins.
trait Fill<C> {
    fn fill(&mut self, configured: &C);
}

impl<T: Clone> Fill<Option<Vec<T>>> for Vec<T> {
    fn fill(&mut self, configured: &Option<Vec<T>>) {
        if let (true, Some(values)) = (self.is_empty(), configured) {
            self.clone_from(values);
        }
    }
}

impl<T: Clone> Fill<Option<T>> for Option<T> {
    fn fill(&mut self, configured: &Option<T>) {
        if self.is_none() {
            self.clone_from(configured);
        }
    }
}

impl Fill<Option<bool>> for bool {
    fn fill(&mut self, configured: &Option<bool>) {
        *self |= configured.unwrap_or(false);
    }
}

/// Fill a clap-defaulted slot, whose default can't be told from "unset", unless
/// `id` was typed on the command line.
fn fill_untyped<T: Clone>(provided: &CliProvided, id: &str, slot: &mut T, configured: &Option<T>) {
    if let (false, Some(value)) = (provided.has(id), configured) {
        slot.clone_from(value);
    }
}

/// urx's config directory: `$HOME/.config/urx` (`$XDG_CONFIG_HOME` is not
/// consulted), or `%APPDATA%\urx` on Windows.
fn urx_config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"));
    base.map(|base| base.join("urx"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::parse_args_from;
    use crate::{cache::CacheType, network::NetworkScope, output::Format, providers::CdxDialect};
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
        assert_eq!(args.format, Format::Plain);
        assert_eq!(args.providers, vec!["wayback", "cc", "otx"]);

        // Apply config to args
        config.apply_to_args(&mut args, &CliProvided::default());

        // Verify args were updated correctly
        assert_eq!(args.output, Some(PathBuf::from("output.txt")));
        assert_eq!(args.format, Format::Json);
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
        assert_eq!(args.parallel, 5);
    }

    #[test]
    fn test_apply_to_args_ignores_invalid_output_format_and_network_scope() {
        let mut config = Config::default();
        config.output.format = Some("yaml".to_string());
        config.network.network_scope = Some("providers only".to_string());

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.format, Format::Plain);
        assert_eq!(args.network_scope, NetworkScope::All);
    }

    #[test]
    fn test_apply_to_args_normalizes_output_format_and_network_scope() {
        let mut config = Config::default();
        config.output.format = Some("JSON".to_string());
        config.network.network_scope = Some("TESTERS,PROVIDERS".to_string());

        let mut args = Args::parse_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(args.format, Format::Json);
        assert_eq!(args.network_scope, NetworkScope::All);
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
        assert_eq!(args.cdx_dialect, Some(CdxDialect::Classic));

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
        assert_eq!(args.cdx_dialect, Some(CdxDialect::Pywb));

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
                api_keys: vec!["github"],
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
        assert_eq!(
            args.cache_type,
            CacheType::Sqlite,
            "invalid value must be ignored"
        );

        // A valid value still applies, case-insensitively.
        let mut config = Config::default();
        config.cache.cache_type = Some("Redis".to_string());
        let mut args = <Args as clap::Parser>::parse_from(["urx", "--silent", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());
        assert_eq!(args.cache_type, CacheType::Redis);
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
                api_keys: vec!["vt"],
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
    fn test_provider_keys_empty_placeholder_keeps_the_main_config_key() {
        // Regression: `vt_api_key = ""` in provider-config wiped the key the
        // main config had just set, and VirusTotal silently ran keyless.
        let cfg = ProviderKeysConfig {
            vt_api_key: Some(String::new()),
            urlscan_api_key: None,
            zoomeye_api_key: None,
            github_api_key: None,
            bevigil_api_key: None,
            notify_url: None,
            unknown: Default::default(),
        };
        let mut args = <Args as clap::Parser>::parse_from(["urx", "example.com"]);
        args.vt_api_key = vec!["realkey".to_string()];
        cfg.apply_to_args(&mut args, CliSuppliedKeys::default());
        assert_eq!(args.vt_api_key, vec!["realkey"]);
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

        assert_eq!(args.format, Format::Plain);
        assert_eq!(args.providers, vec!["wayback", "cc", "otx"]);
        assert_eq!(args.cc_index, vec!["latest"]);
        assert_eq!(args.network_scope, NetworkScope::All);
        assert_eq!(args.timeout, 120);
        assert_eq!(args.retries, 2);
        assert_eq!(args.parallel, 5);
        assert_eq!(args.cache_type, CacheType::Sqlite);
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

        assert_eq!(args.format, Format::Json);
        assert_eq!(args.providers, vec!["arquivo"]);
        assert_eq!(args.cc_index, vec!["CC-MAIN-2020-05"]);
        assert_eq!(args.network_scope, NetworkScope::Testers);
        assert_eq!(args.timeout, 30);
        assert_eq!(args.retries, 9);
        assert_eq!(args.parallel, 2);
        assert_eq!(args.cache_type, CacheType::Redis);
        assert_eq!(args.cache_ttl, 60);
    }

    #[test]
    fn test_cli_discovery_includes_override_config_excludes() {
        let file =
            create_temp_config_file("[provider]\nexclude_robots = true\nexclude_sitemap = true\n");
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) = parse_args_from([
            "urx",
            "--include-robots",
            "--include-sitemap",
            "example.com",
        ]);
        assert!(provided.has("include_robots"));
        assert!(provided.has("include_sitemap"));

        config.apply_to_args(&mut args, &provided);

        assert_eq!(
            (args.should_use_robots(), args.should_use_sitemap()),
            (true, true)
        );
    }

    #[test]
    fn test_config_discovery_include_and_exclude_apply_without_cli_flags() {
        let include_file = create_temp_config_file(
            "[provider]\ninclude_robots = false\ninclude_sitemap = false\n",
        );
        let config = Config::from_file(include_file.path()).unwrap();
        let (mut args, provided) = parse_args_from(["urx", "example.com"]);

        config.apply_to_args(&mut args, &provided);

        assert!(!args.include_robots);
        assert!(!args.include_sitemap);
        assert!(!args.should_use_robots());
        assert!(!args.should_use_sitemap());

        let exclude_file =
            create_temp_config_file("[provider]\nexclude_robots = true\nexclude_sitemap = true\n");
        let config = Config::from_file(exclude_file.path()).unwrap();
        let (mut args, provided) = parse_args_from(["urx", "example.com"]);

        config.apply_to_args(&mut args, &provided);

        assert!(args.exclude_robots);
        assert!(args.exclude_sitemap);
        assert!(!args.should_use_robots());
        assert!(!args.should_use_sitemap());
    }

    #[test]
    fn test_cli_discovery_excludes_override_config_includes() {
        let file =
            create_temp_config_file("[provider]\ninclude_robots = true\ninclude_sitemap = true\n");
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) = parse_args_from([
            "urx",
            "--exclude-robots",
            "--exclude-sitemap",
            "example.com",
        ]);
        assert!(provided.has("exclude_robots"));
        assert!(provided.has("exclude_sitemap"));

        config.apply_to_args(&mut args, &provided);

        assert!(!args.should_use_robots());
        assert!(!args.should_use_sitemap());
    }

    #[test]
    fn test_config_show_only_path_and_param_apply_without_cli_view() {
        let host_file = create_temp_config_file("[filter]\nshow_only_host = true\n");
        let config = Config::from_file(host_file.path()).unwrap();
        let (mut args, provided) = parse_args_from(["urx", "example.com"]);

        config.apply_to_args(&mut args, &provided);

        assert!(args.show_only_host);
        assert!(!args.show_only_path);
        assert!(!args.show_only_param);

        let path_file = create_temp_config_file("[filter]\nshow_only_path = true\n");
        let config = Config::from_file(path_file.path()).unwrap();
        let (mut args, provided) = parse_args_from(["urx", "example.com"]);

        config.apply_to_args(&mut args, &provided);

        assert!(args.show_only_path);
        assert!(!args.show_only_host);
        assert!(!args.show_only_param);

        let param_file = create_temp_config_file("[filter]\nshow_only_param = true\n");
        let config = Config::from_file(param_file.path()).unwrap();
        let (mut args, provided) = parse_args_from(["urx", "example.com"]);

        config.apply_to_args(&mut args, &provided);

        assert!(args.show_only_param);
        assert!(!args.show_only_host);
        assert!(!args.show_only_path);
    }

    #[test]
    fn test_cli_show_only_path_overrides_a_different_configured_view() {
        let file = create_temp_config_file("[filter]\nshow_only_host = true\n");
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) = parse_args_from(["urx", "--show-only-path", "example.com"]);

        config.apply_to_args(&mut args, &provided);

        assert!(!args.show_only_host);
        assert!(args.show_only_path);
        assert!(!args.show_only_param);
    }

    #[test]
    fn test_cli_parameter_view_overrides_config_show_only_view() {
        let file = create_temp_config_file("[filter]\nshow_only_host = true\n");
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) = parse_args_from(["urx", "--params", "example.com"]);

        config.apply_to_args(&mut args, &provided);

        assert!(args.params);
        assert!(!args.show_only_host);
    }

    #[test]
    fn test_config_rejects_multiple_show_only_views() {
        let file =
            create_temp_config_file("[filter]\nshow_only_host = true\nshow_only_path = true\n");

        let error = Config::from_file(file.path()).expect_err("mutually exclusive views");

        assert!(
            error.to_string().contains("mutually exclusive"),
            "{error:#}"
        );
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
            expand_specs = true
            max_spec_files = 25

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
            archive_body_dir = "/tmp/urx-bodies"
        "#;
        let file = create_temp_config_file(content);

        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);
        assert!(args.archive_body);
        assert_eq!(args.archive_body_limit, 42);
        assert_eq!(
            args.archive_body_dir,
            Some(PathBuf::from("/tmp/urx-bodies"))
        );

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
    // --- spec-expansion ---

    #[test]
    fn test_spec_expansion_settings_load_from_config_and_yield_to_the_cli() {
        let content = r#"
            [testing]
            expand_specs = true
            max_spec_files = 7
        "#;
        let file = create_temp_config_file(content);

        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);
        assert!(args.expand_specs);
        assert_eq!(args.max_spec_files, 7);

        // An explicit --max-spec-files wins even when it equals the clap
        // default.
        let (mut args, provided) =
            crate::cli::parse_args_from(["urx", "--max-spec-files", "50", "example.com"]);
        Config::from_file(file.path())
            .unwrap()
            .apply_to_args(&mut args, &provided);
        assert_eq!(args.max_spec_files, 50);
    }
    // --- result-filters ---
    #[test]
    fn test_filter_config_supplies_the_scope_and_meta_filters() {
        let content = r#"
            [filter]
            scope_file = ["/tmp/scope.txt"]
            meta_first_seen_after = "2015"
            meta_first_seen_before = "2020"
            meta_last_seen_after = "2021"
            meta_last_seen_before = "2024"
            meta_mime = ["text/html", "application/json"]
            meta_exclude_mime = ["image/*"]
            meta_status = ["200", "30x"]
            meta_exclude_status = ["404"]
        "#;
        let config = Config::from_file(create_temp_config_file(content).path()).unwrap();
        // A whole section urx does not know about is reported, so a typo in any
        // of these keys would show up here rather than doing nothing.
        assert!(
            config.unknown_keys().is_empty(),
            "{:?}",
            config.unknown_keys()
        );

        let mut args = <Args as clap::Parser>::parse_from(["urx", "--silent", "example.com"]);
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(
            args.scope_file,
            vec![std::path::PathBuf::from("/tmp/scope.txt")]
        );
        assert_eq!(args.meta_first_seen_after.as_deref(), Some("2015"));
        assert_eq!(args.meta_first_seen_before.as_deref(), Some("2020"));
        assert_eq!(args.meta_last_seen_after.as_deref(), Some("2021"));
        assert_eq!(args.meta_last_seen_before.as_deref(), Some("2024"));
        assert_eq!(args.meta_mime, vec!["text/html", "application/json"]);
        assert_eq!(args.meta_exclude_mime, vec!["image/*"]);
        assert_eq!(args.meta_status, vec!["200", "30x"]);
        assert_eq!(args.meta_exclude_status, vec!["404"]);
    }

    #[test]
    fn test_cli_beats_the_config_file_for_the_new_filters() {
        let content = r#"
            [filter]
            scope_file = ["/tmp/from-config.txt"]
            meta_status = ["404"]
            meta_last_seen_before = "2005"
        "#;
        let config = Config::from_file(create_temp_config_file(content).path()).unwrap();

        let mut args = <Args as clap::Parser>::parse_from([
            "urx",
            "--silent",
            "--scope-file",
            "/tmp/from-cli.txt",
            "--meta-status",
            "200",
            "--meta-last-seen-before",
            "2024",
            "example.com",
        ]);
        config.apply_to_args(&mut args, &CliProvided::default());

        assert_eq!(
            args.scope_file,
            vec![std::path::PathBuf::from("/tmp/from-cli.txt")]
        );
        assert_eq!(args.meta_status, vec!["200"]);
        assert_eq!(args.meta_last_seen_before.as_deref(), Some("2024"));
    }
    // --- end result-filters ---

    #[test]
    fn test_header_settings_load_from_config_and_yield_to_the_cli() {
        let content = r#"
            [network]
            header = ["X-Env: staging", "X-Team: appsec"]
            cookie = "session=from-config"
            user_agent = "urx-config/1"
        "#;
        let file = create_temp_config_file(content);

        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &provided);
        assert_eq!(args.header, vec!["X-Env: staging", "X-Team: appsec"]);
        assert_eq!(args.cookie.as_deref(), Some("session=from-config"));
        assert_eq!(args.user_agent.as_deref(), Some("urx-config/1"));

        // A -H on the command line replaces the configured set wholesale, so a
        // run can always be made anonymous again without editing the file.
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) =
            crate::cli::parse_args_from(["urx", "-H", "X-Only: cli", "example.com"]);
        config.apply_to_args(&mut args, &provided);
        assert_eq!(args.header, vec!["X-Only: cli"]);
    }

    #[test]
    fn test_a_single_header_string_is_accepted_too() {
        let content = r#"
            [network]
            header = "X-Env: staging"
        "#;
        let file = create_temp_config_file(content);
        let config = Config::from_file(file.path()).unwrap();
        let (mut args, provided) = crate::cli::parse_args_from(["urx", "example.com"]);
        config.apply_to_args(&mut args, &provided);
        assert_eq!(args.header, vec!["X-Env: staging"]);
    }
}
