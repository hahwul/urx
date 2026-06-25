use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[clap(name = "urx", version)]
pub struct Args {
    /// Domains to fetch URLs for
    #[clap(name = "DOMAINS")]
    pub domains: Vec<String>,

    /// Config file to load
    #[clap(short, long, value_parser)]
    pub config: Option<PathBuf>,

    /// Path to a separate provider config file holding only API keys
    /// (default: $XDG_CONFIG_HOME/urx/provider-config.toml). Keeping keys in
    /// a dedicated file makes the main config safe to share.
    /// Precedence: CLI/env keys > provider-config > main config.
    #[clap(long = "provider-config", value_parser)]
    pub provider_config: Option<PathBuf>,

    #[clap(help_heading = "Input Options")]
    /// Read URLs directly from files (supports WARC, URLTeam compressed, and text files). Use multiple --files flags or space-separate multiple files.
    #[clap(long, action = clap::ArgAction::Append, num_args = 1.., value_parser)]
    pub files: Vec<PathBuf>,

    /// File(s) containing newline-separated domains to scan. Repeatable;
    /// merged with positional DOMAINS and stdin. Blank lines and `#` comments
    /// are ignored.
    #[clap(help_heading = "Input Options")]
    #[clap(long = "domain-list", visible_alias = "dL", action = clap::ArgAction::Append, value_parser)]
    pub domain_list: Vec<PathBuf>,

    #[clap(help_heading = "Output Options")]
    /// Output file to write results
    #[clap(short, long, value_parser)]
    pub output: Option<PathBuf>,

    /// Write one file per domain into this directory (e.g. `example.com.json`).
    /// Coexists with --output (which still writes the aggregated file) and
    /// stdout. The directory is created if missing. The extension matches
    /// --format (`json`, `csv`, or `txt` for plain).
    #[clap(help_heading = "Output Options")]
    #[clap(long = "output-dir", visible_alias = "oD", value_parser)]
    pub output_dir: Option<PathBuf>,

    /// Output format (e.g., "plain", "json", "csv")
    #[clap(help_heading = "Output Options")]
    #[clap(short, long, default_value = "plain")]
    pub format: String,

    /// Merge endpoints with the same path and merge URL parameters
    #[clap(help_heading = "Output Options")]
    #[clap(long)]
    pub merge_endpoint: bool,

    /// Normalize URLs for better deduplication (sorts query parameters, removes trailing slashes)
    #[clap(help_heading = "Output Options")]
    #[clap(long)]
    pub normalize_url: bool,

    /// Providers to use (comma-separated, e.g., "wayback,cc,otx,vt,urlscan")
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',', default_value = "wayback,cc,otx")]
    pub providers: Vec<String>,

    /// Providers to exclude from enumeration (comma-separated). Applied after
    /// --providers / --all-providers, so it wins on conflict.
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',')]
    pub exclude_providers: Vec<String>,

    /// Enable every supported provider. API-keyed providers only activate
    /// when a key is available via flag, env, or config file.
    #[clap(help_heading = "Provider Options")]
    #[clap(long)]
    pub all_providers: bool,

    /// List every supported provider (name, API key requirement, summary)
    /// then exit.
    #[clap(help_heading = "Provider Options")]
    #[clap(long)]
    pub list_providers: bool,

    /// Include subdomains when searching
    #[clap(help_heading = "Provider Options")]
    #[clap(long)]
    pub subs: bool,

    #[clap(help_heading = "Provider Options")]
    /// Common Crawl index to use. Accepts a comma-separated list to query
    /// multiple indexes in parallel (e.g. `CC-MAIN-2026-17,CC-MAIN-2025-51`).
    /// The literal `latest` resolves to the newest index via collinfo.json.
    #[clap(long, default_value = "CC-MAIN-2026-17", value_delimiter = ',')]
    pub cc_index: Vec<String>,

    /// Restrict Wayback Machine results to snapshots at or after this date.
    /// Accepts YYYY, YYYYMM, YYYYMMDD, or the full 14-digit CDX timestamp.
    /// Partial dates pad toward the start of the range.
    #[clap(help_heading = "Provider Options")]
    #[clap(long)]
    pub wayback_from: Option<String>,

    /// Restrict Wayback Machine results to snapshots at or before this date.
    /// Same format as --wayback-from; partial dates pad toward the end of
    /// the range.
    #[clap(help_heading = "Provider Options")]
    #[clap(long)]
    pub wayback_to: Option<String>,

    #[clap(help_heading = "Provider Options")]
    /// API key for VirusTotal (can be used multiple times for rotation, can also use URX_VT_API_KEY environment variable with comma-separated keys)
    #[clap(long, action = clap::ArgAction::Append)]
    pub vt_api_key: Vec<String>,

    #[clap(help_heading = "Provider Options")]
    /// API key for Urlscan (can be used multiple times for rotation, can also use URX_URLSCAN_API_KEY environment variable with comma-separated keys)
    #[clap(long, action = clap::ArgAction::Append)]
    pub urlscan_api_key: Vec<String>,

    #[clap(help_heading = "Provider Options")]
    /// API key for ZoomEye (can be used multiple times for rotation, can also use URX_ZOOMEYE_API_KEY environment variable with comma-separated keys)
    #[clap(long, action = clap::ArgAction::Append)]
    pub zoomeye_api_key: Vec<String>,

    #[clap(help_heading = "Provider Options")]
    /// Personal access token for GitHub Code Search (also reads URX_GITHUB_API_KEY,
    /// comma-separated for rotation). Required for the `github` provider.
    #[clap(long, action = clap::ArgAction::Append)]
    pub github_api_key: Vec<String>,

    /// Include robots.txt discovery (default: true)
    #[clap(long, default_value = "true", hide = true)]
    pub include_robots: bool,

    /// Exclude robots.txt discovery
    #[clap(long, help_heading = "Discovery Options")]
    pub exclude_robots: bool,

    /// Include sitemap.xml discovery (default: true)
    #[clap(long, default_value = "true", hide = true)]
    pub include_sitemap: bool,

    /// Exclude sitemap.xml discovery
    #[clap(long, help_heading = "Discovery Options")]
    pub exclude_sitemap: bool,

    #[clap(help_heading = "Display Options")]
    /// Show verbose output
    #[clap(short, long)]
    pub verbose: bool,

    #[clap(help_heading = "Display Options")]
    /// Silent mode (no output)
    #[clap(long)]
    pub silent: bool,

    #[clap(help_heading = "Display Options")]
    /// No progress bar
    #[clap(long)]
    pub no_progress: bool,

    /// Disable ANSI color in the progress UI and output (the NO_COLOR env var is
    /// also honored automatically).
    #[clap(help_heading = "Display Options")]
    #[clap(long)]
    pub no_color: bool,

    /// Annotate each output URL with the providers that returned it.
    /// For JSON/CSV this adds a `sources` field/column; for plain text it
    /// appends `[provider1,provider2]` after the URL.
    #[clap(help_heading = "Display Options")]
    #[clap(long)]
    pub show_sources: bool,

    /// Print a per-provider summary (URLs found, errors, elapsed) to stderr
    /// when the run finishes.
    #[clap(help_heading = "Display Options")]
    #[clap(long)]
    pub stats: bool,

    /// Filter Presets (e.g., "no-resources,no-images,only-js,only-style")
    #[clap(help_heading = "Filter Options")]
    #[clap(short, long, value_delimiter = ',')]
    pub preset: Vec<String>,

    /// Filter URLs to only include those with specific extensions (comma-separated, e.g., "js,php,aspx")
    #[clap(help_heading = "Filter Options")]
    #[clap(short, long, value_delimiter = ',')]
    pub extensions: Vec<String>,

    /// Filter URLs to exclude those with specific extensions (comma-separated, e.g., "html,txt")
    #[clap(help_heading = "Filter Options")]
    #[clap(long, value_delimiter = ',')]
    pub exclude_extensions: Vec<String>,

    /// Filter URLs to only include those containing specific patterns (comma-separated)
    #[clap(help_heading = "Filter Options")]
    #[clap(long, value_delimiter = ',')]
    pub patterns: Vec<String>,

    /// Filter URLs to exclude those containing specific patterns (comma-separated)
    #[clap(help_heading = "Filter Options")]
    #[clap(long, value_delimiter = ',')]
    pub exclude_patterns: Vec<String>,

    /// Only show the host part of the URLs
    #[clap(help_heading = "Filter Options")]
    #[clap(long)]
    pub show_only_host: bool,

    /// Only show the path part of the URLs
    #[clap(help_heading = "Filter Options")]
    #[clap(long)]
    pub show_only_path: bool,

    /// Only show the parameters part of the URLs
    #[clap(help_heading = "Filter Options")]
    #[clap(long)]
    pub show_only_param: bool,

    /// Minimum URL length to include
    #[clap(help_heading = "Filter Options")]
    #[clap(long = "min-length")]
    pub min_length: Option<usize>,

    /// Maximum URL length to include
    #[clap(help_heading = "Filter Options")]
    #[clap(long = "max-length")]
    pub max_length: Option<usize>,

    /// Enforce exact host validation (default)
    #[clap(help_heading = "Filter Options")]
    #[clap(long, default_value = "true")]
    pub strict: bool,

    /// Disable host validation entirely (keep every URL a provider returns,
    /// regardless of host). Convenience inverse of `--strict`; wins over it.
    #[clap(help_heading = "Filter Options")]
    #[clap(long)]
    pub no_strict: bool,

    /// Control which components network settings apply to (all, providers, testers, or providers,testers)
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "all", value_parser = validate_network_scope)]
    pub network_scope: String,

    #[clap(help_heading = "Network Options")]
    /// Use proxy for HTTP requests (format: <http://proxy.example.com:8080>)
    #[clap(long)]
    pub proxy: Option<String>,

    /// Proxy authentication credentials (format: username:password)
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub proxy_auth: Option<String>,

    /// Skip SSL certificate verification (accept self-signed certs)
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub insecure: bool,

    /// Use a random User-Agent for HTTP requests
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub random_agent: bool,

    /// Request timeout in seconds
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "120", value_parser = validate_positive_timeout)]
    pub timeout: u64,

    /// Number of retries for failed requests
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "2")]
    pub retries: u32,

    /// Maximum domains fetched concurrently per provider (and concurrent URL
    /// tests). A provider's --rate-limit is shared across these, so the
    /// configured rate is still honored.
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "5", value_parser = validate_positive_parallel)]
    pub parallel: Option<u32>,

    /// Rate limit (requests per second)
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub rate_limit: Option<f32>,

    /// Per-provider rate limit overrides as comma-separated `id=req_per_sec`
    /// pairs (e.g. `--rate-limit-by vt=1,wayback=10`). Providers not listed
    /// fall back to the global --rate-limit (if set).
    #[clap(help_heading = "Network Options")]
    #[clap(long, value_delimiter = ',')]
    pub rate_limit_by: Vec<String>,

    /// Global ceiling on provider enumeration time, in seconds. When the
    /// deadline elapses, in-flight provider fetches are aborted and urx
    /// proceeds with whatever URLs have been collected so far. `0` (the
    /// default) means no ceiling.
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "0")]
    pub max_time: u64,

    /// Check HTTP status code of collected URLs
    #[clap(help_heading = "Testing Options")]
    #[clap(long, visible_alias = "cs")]
    pub check_status: bool,

    /// Include URLs with specific HTTP status codes or patterns (e.g., --is=200,30x)
    #[clap(help_heading = "Testing Options")]
    #[clap(long, visible_alias = "is")]
    pub include_status: Vec<String>,

    /// Exclude URLs with specific HTTP status codes or patterns (e.g., --es=404,50x,5xx)
    #[clap(help_heading = "Testing Options")]
    #[clap(long, visible_alias = "es")]
    pub exclude_status: Vec<String>,

    /// Extract additional links from collected URLs (requires HTTP requests)
    #[clap(help_heading = "Testing Options")]
    #[clap(long)]
    pub extract_links: bool,

    /// Enable incremental scanning mode (only return new URLs compared to previous scans)
    #[clap(help_heading = "Cache Options")]
    #[clap(long)]
    pub incremental: bool,

    /// Cache backend type (sqlite or redis)
    #[clap(help_heading = "Cache Options")]
    #[clap(long, default_value = "sqlite")]
    pub cache_type: String,

    /// Path for SQLite cache database
    #[clap(help_heading = "Cache Options")]
    #[clap(long)]
    pub cache_path: Option<std::path::PathBuf>,

    /// Redis connection URL for remote caching
    #[clap(help_heading = "Cache Options")]
    #[clap(long)]
    pub redis_url: Option<String>,

    /// Cache time-to-live in seconds (default: 24 hours)
    #[clap(help_heading = "Cache Options")]
    #[clap(long, default_value = "86400")]
    pub cache_ttl: u64,

    /// Disable caching entirely
    #[clap(help_heading = "Cache Options")]
    #[clap(long)]
    pub no_cache: bool,
}

pub fn read_domains_from_stdin() -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    use std::io::{self, BufRead};

    let stdin = io::stdin();
    let mut domains = Vec::new();

    for line in stdin.lock().lines() {
        let domain = line.context("Failed to read line from stdin")?;
        let domain = parse_domain_line(&domain);
        if let Some(d) = domain {
            domains.push(d);
        }
    }

    Ok(domains)
}

/// Read newline-separated domains from a file. Blank lines and lines that
/// start with `#` (after trimming) are skipped so users can keep notes
/// alongside the list.
pub fn read_domains_from_file(path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open domain list: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut domains = Vec::new();
    for line in reader.lines() {
        let raw = line.with_context(|| format!("Failed to read {}", path.display()))?;
        if let Some(d) = parse_domain_line(&raw) {
            domains.push(d);
        }
    }
    Ok(domains)
}

/// Trim whitespace and drop blank / comment lines from a single text line.
fn parse_domain_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Reduce a user-supplied target to a bare host. People routinely paste a full
/// URL (`https://example.com/path?q=1`) or `example.com/` as the target; left
/// as-is those produce a malformed provider query (`url=https://example.com/...`)
/// that silently returns nothing. We strip any scheme, path, query, and
/// fragment and lowercase the host. Returns `None` when nothing host-like
/// remains. `www.` is intentionally preserved (it can be a distinct host).
pub fn normalize_domain(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // A pasted full URL: let the URL parser pull out the host. This branch is
    // authoritative — a `://` means the input is meant as a URL, so if it has
    // no parseable host we return None rather than mis-reading the scheme.
    if trimmed.contains("://") {
        return url::Url::parse(trimmed)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_lowercase()));
    }
    // Otherwise drop a scheme-relative prefix and anything from the first
    // path/query/fragment separator onward.
    let host = trimmed
        .trim_start_matches("//")
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

impl Args {
    /// Parse `--rate-limit-by` entries into a `provider_id -> requests/sec`
    /// map. Malformed entries are dropped and reported via `parse_errors`
    /// when verbose; the caller decides whether to surface them.
    pub fn rate_limit_overrides(&self) -> std::collections::HashMap<String, f32> {
        let mut map = std::collections::HashMap::new();
        for raw in &self.rate_limit_by {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let id = k.trim().to_string();
                if let Ok(rate) = v.trim().parse::<f32>() {
                    if !id.is_empty() && rate > 0.0 {
                        map.insert(id, rate);
                    }
                }
            }
        }
        map
    }

    /// Effective host-validation setting. `--no-strict` wins over `--strict`,
    /// so users can disable filtering with the natural flag instead of the
    /// unusual `--strict false`.
    pub fn strict_enabled(&self) -> bool {
        self.strict && !self.no_strict
    }

    /// Check if robots.txt discovery should be used
    pub fn should_use_robots(&self) -> bool {
        !self.exclude_robots && self.include_robots
    }

    /// Check if sitemap.xml discovery should be used
    pub fn should_use_sitemap(&self) -> bool {
        !self.exclude_sitemap && self.include_sitemap
    }
}

fn validate_network_scope(s: &str) -> Result<String, String> {
    match s {
        "all" | "providers" | "testers" | "providers,testers" | "testers,providers" => Ok(s.to_string()),
        _ => Err(format!("Invalid network scope: {s}. Allowed values are all, providers, testers, or providers,testers")),
    }
}

fn validate_positive_timeout(s: &str) -> Result<u64, String> {
    let value = s
        .parse::<u64>()
        .map_err(|_| format!("Invalid timeout: {s}. Must be a positive integer"))?;
    if value == 0 {
        Err("Invalid timeout: 0. Must be at least 1 second".to_string())
    } else {
        Ok(value)
    }
}

fn validate_positive_parallel(s: &str) -> Result<u32, String> {
    let value = s
        .parse::<u32>()
        .map_err(|_| format!("Invalid parallel value: {s}. Must be a positive integer"))?;
    if value == 0 {
        Err("Invalid parallel value: 0. Must be at least 1".to_string())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_default_values() {
        let args = Args::parse_from(["urx", "example.com"]);
        assert_eq!(args.domains, vec!["example.com"]);
        assert_eq!(args.format, "plain");
        assert_eq!(args.providers, vec!["wayback", "cc", "otx"]);
        assert_eq!(args.cc_index, vec!["CC-MAIN-2026-17"]);
        assert_eq!(args.timeout, 120);
        assert_eq!(args.retries, 2);
        assert!(args.include_robots);
        assert!(args.include_sitemap);
        assert!(!args.exclude_robots);
        assert!(!args.exclude_sitemap);
        assert!(args.should_use_robots());
        assert!(args.should_use_sitemap());
    }

    #[test]
    fn test_args_multiple_domains() {
        let args = Args::parse_from(["urx", "example.com", "example.org"]);
        assert_eq!(args.domains, vec!["example.com", "example.org"]);
    }

    #[test]
    fn test_args_output_options() {
        let args = Args::parse_from(["urx", "example.com", "-o", "output.txt", "-f", "json"]);
        assert_eq!(args.domains, vec!["example.com"]);
        assert!(args.output.is_some());
        assert_eq!(args.output.unwrap().to_str().unwrap(), "output.txt");
        assert_eq!(args.format, "json");
    }

    #[test]
    fn test_args_providers() {
        let args = Args::parse_from(["urx", "example.com", "--providers", "wayback,vt"]);
        assert_eq!(args.providers, vec!["wayback", "vt"]);
    }

    #[test]
    fn test_network_options() {
        let args = Args::parse_from([
            "urx",
            "example.com",
            "--proxy",
            "http://proxy:8080",
            "--timeout",
            "60",
        ]);
        assert_eq!(args.proxy.unwrap(), "http://proxy:8080");
        assert_eq!(args.timeout, 60);
    }

    #[test]
    fn test_timeout_must_be_positive() {
        let err = Args::try_parse_from(["urx", "example.com", "--timeout", "0"]).unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("Invalid timeout: 0"));
    }

    #[test]
    fn test_parallel_must_be_positive() {
        let err = Args::try_parse_from(["urx", "example.com", "--parallel", "0"]).unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("Invalid parallel value: 0"));
    }

    #[test]
    fn test_filter_options() {
        let args = Args::parse_from([
            "urx",
            "example.com",
            "-e",
            "js,php",
            "--exclude-extensions",
            "html,css",
        ]);
        assert_eq!(args.extensions, vec!["js", "php"]);
        assert_eq!(args.exclude_extensions, vec!["html", "css"]);
    }

    #[test]
    fn test_robots_sitemap_flags() {
        // Test default values are true for include flags and false for exclude flags
        let args = Args::parse_from(["urx", "example.com"]);
        assert!(args.include_robots);
        assert!(args.include_sitemap);
        assert!(!args.exclude_robots);
        assert!(!args.exclude_sitemap);
        assert!(args.should_use_robots());
        assert!(args.should_use_sitemap());

        // Test they can be disabled via exclude flags (visible in help)
        let args = Args::parse_from([
            "urx",
            "example.com",
            "--exclude-robots",
            "--exclude-sitemap",
        ]);
        assert!(args.exclude_robots);
        assert!(args.exclude_sitemap);
        assert!(!args.should_use_robots());
        assert!(!args.should_use_sitemap());
    }

    #[test]
    fn test_robots_sitemap_helper_methods() {
        // Default is to use both
        let args = Args::parse_from(["urx", "example.com"]);
        assert!(args.should_use_robots());
        assert!(args.should_use_sitemap());

        // Exclude flags take precedence over include flags
        let args = Args::parse_from(["urx", "example.com", "--exclude-robots"]);
        assert!(!args.should_use_robots());
        assert!(args.should_use_sitemap());

        // Explicit exclude always wins over include setting
        let args = Args::parse_from(["urx", "example.com", "--include-robots", "--exclude-robots"]);
        assert!(args.exclude_robots);
        assert!(args.include_robots); // Both flags retain their values
        assert!(!args.should_use_robots()); // But should_use_robots uses the logic
    }

    #[test]
    fn test_validate_network_scope_valid() {
        assert!(validate_network_scope("all").is_ok());
        assert!(validate_network_scope("providers").is_ok());
        assert!(validate_network_scope("testers").is_ok());
        assert!(validate_network_scope("providers,testers").is_ok());
    }

    #[test]
    fn test_validate_network_scope_invalid() {
        assert!(validate_network_scope("invalid").is_err());
    }

    #[test]
    fn test_validate_positive_timeout() {
        assert_eq!(validate_positive_timeout("1"), Ok(1));
        assert!(validate_positive_timeout("0").is_err());
        assert!(validate_positive_timeout("abc").is_err());
    }

    #[test]
    fn test_validate_positive_parallel() {
        assert_eq!(validate_positive_parallel("1"), Ok(1));
        assert!(validate_positive_parallel("0").is_err());
        assert!(validate_positive_parallel("abc").is_err());
    }

    #[test]
    fn test_files_flag() {
        // Test that the new --files flag accepts multiple files
        let args = Args::parse_from(["urx", "--files", "file1.txt", "file2.warc", "--verbose"]);
        assert_eq!(args.files.len(), 2);
        assert_eq!(args.files[0].to_str().unwrap(), "file1.txt");
        assert_eq!(args.files[1].to_str().unwrap(), "file2.warc");
        assert!(args.verbose);
    }

    #[test]
    fn test_multiple_files_flags() {
        // Test that repeated --files flags work
        let args = Args::parse_from(["urx", "--files", "file1.txt", "--files", "file2.warc"]);
        assert_eq!(args.files.len(), 2);
        assert_eq!(args.files[0].to_str().unwrap(), "file1.txt");
        assert_eq!(args.files[1].to_str().unwrap(), "file2.warc");
    }

    #[test]
    fn test_normalize_domain() {
        assert_eq!(
            normalize_domain("example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_domain("https://example.com/path?q=1#frag").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_domain("http://www.example.com/").as_deref(),
            Some("www.example.com")
        );
        assert_eq!(
            normalize_domain("example.com/foo").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_domain("  EXAMPLE.com.  ").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_domain("//cdn.example.com/x").as_deref(),
            Some("cdn.example.com")
        );
        assert_eq!(normalize_domain(""), None);
        assert_eq!(normalize_domain("   "), None);
        assert_eq!(normalize_domain("https://"), None);
    }

    #[test]
    fn test_strict_enabled() {
        let args = Args::parse_from(["urx", "example.com"]);
        assert!(args.strict_enabled()); // default on

        let args = Args::parse_from(["urx", "example.com", "--no-strict"]);
        assert!(!args.strict_enabled()); // --no-strict wins

        let args = Args::parse_from(["urx", "example.com", "--strict", "true", "--no-strict"]);
        assert!(!args.strict_enabled()); // --no-strict still wins over --strict true
    }

    #[test]
    fn test_parse_domain_line_skips_blank_and_comments() {
        assert_eq!(parse_domain_line(""), None);
        assert_eq!(parse_domain_line("   "), None);
        assert_eq!(parse_domain_line("# comment"), None);
        assert_eq!(parse_domain_line("  # leading-space comment"), None);
        assert_eq!(
            parse_domain_line("  example.com  "),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_read_domains_from_file() -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new()?;
        writeln!(
            file,
            "example.com\n  # comment\n\n  another.test  \n#trailing"
        )?;
        let domains = read_domains_from_file(file.path())?;
        assert_eq!(domains, vec!["example.com", "another.test"]);
        Ok(())
    }

    #[test]
    fn test_domain_list_flag_parsed() {
        let args = Args::parse_from([
            "urx",
            "--domain-list",
            "domains.txt",
            "--domain-list",
            "more.txt",
        ]);
        assert_eq!(args.domain_list.len(), 2);
        assert_eq!(args.domain_list[0].to_str().unwrap(), "domains.txt");
        assert_eq!(args.domain_list[1].to_str().unwrap(), "more.txt");
    }

    #[test]
    fn test_max_time_defaults_to_zero() {
        let args = Args::parse_from(["urx", "example.com"]);
        assert_eq!(args.max_time, 0);
        let args = Args::parse_from(["urx", "--max-time", "300", "example.com"]);
        assert_eq!(args.max_time, 300);
    }

    #[test]
    fn test_rate_limit_overrides_parses_valid_entries() {
        let args = Args::parse_from([
            "urx",
            "--rate-limit-by",
            "vt=2,wayback=10.5",
            "--rate-limit-by",
            "otx=1",
            "example.com",
        ]);
        let map = args.rate_limit_overrides();
        assert_eq!(map.get("vt"), Some(&2.0));
        assert_eq!(map.get("wayback"), Some(&10.5));
        assert_eq!(map.get("otx"), Some(&1.0));
    }

    #[test]
    fn test_rate_limit_overrides_skips_malformed() {
        let args = Args::parse_from([
            "urx",
            "--rate-limit-by",
            "vt=oops,nokey=1,=2,wayback=-1",
            "example.com",
        ]);
        let map = args.rate_limit_overrides();
        // "vt=oops" -> not a number, dropped
        // "nokey=1" -> kept, "nokey" -> 1
        // "=2" -> empty id, dropped
        // "wayback=-1" -> non-positive, dropped
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("nokey"), Some(&1.0));
    }

    #[test]
    fn test_cc_index_accepts_comma_separated_list() {
        let args = Args::parse_from([
            "urx",
            "--cc-index",
            "CC-MAIN-2026-17,CC-MAIN-2025-51",
            "example.com",
        ]);
        assert_eq!(args.cc_index, vec!["CC-MAIN-2026-17", "CC-MAIN-2025-51"]);
    }

    #[test]
    fn test_wayback_date_flags_parsed() {
        let args = Args::parse_from([
            "urx",
            "--wayback-from",
            "2020",
            "--wayback-to",
            "2023-06-30",
            "example.com",
        ]);
        assert_eq!(args.wayback_from.as_deref(), Some("2020"));
        assert_eq!(args.wayback_to.as_deref(), Some("2023-06-30"));
    }

    #[test]
    fn test_output_dir_flag_parsed() {
        let args = Args::parse_from(["urx", "--output-dir", "out/", "example.com"]);
        assert_eq!(
            args.output_dir.as_deref().map(|p| p.to_str().unwrap()),
            Some("out/")
        );
    }

    #[test]
    fn test_provider_config_flag_parsed() {
        let args = Args::parse_from(["urx", "--provider-config", "/tmp/keys.toml", "example.com"]);
        assert_eq!(
            args.provider_config.as_deref().map(|p| p.to_str().unwrap()),
            Some("/tmp/keys.toml")
        );
    }

    #[test]
    fn test_read_domains_from_stdin() {
        use std::io::{self, BufRead, Cursor};

        // Create a cursor with test input data
        let input = "example.com\nexample.org\n\n";
        let cursor = Cursor::new(input);

        // Extract lines from the cursor
        let buffer = io::BufReader::new(cursor);
        let mut domains = Vec::new();
        for line in buffer.lines() {
            let domain = line.unwrap();
            if !domain.trim().is_empty() {
                domains.push(domain.trim().to_string());
            }
        }

        assert_eq!(domains, vec!["example.com", "example.org"]);
    }
}
