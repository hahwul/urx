use clap::{CommandFactory, FromArgMatches, Parser};
use std::collections::HashSet;
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

    /// Output format: "plain", "json" (one array), "jsonl" (one JSON object
    /// per line — pipeline-friendly and valid while still being written), "csv"
    #[clap(help_heading = "Output Options")]
    #[clap(short, long, default_value = "plain")]
    pub format: String,

    /// Merge endpoints with the same path and merge URL parameters
    #[clap(help_heading = "Output Options")]
    #[clap(long)]
    pub merge_endpoint: bool,

    /// Write URLs as each provider reports them instead of once at the end, so
    /// a pipeline starts working immediately on large targets. Output is
    /// deduplicated and filtered exactly as usual, but unsorted (results arrive
    /// in provider-completion order). Caching is bypassed, and options needing
    /// the complete result set are rejected — see the error message for which.
    #[clap(help_heading = "Output Options")]
    #[clap(long)]
    pub stream: bool,

    /// Normalize URLs for better deduplication (sorts query parameters, removes trailing slashes)
    #[clap(help_heading = "Output Options")]
    #[clap(long)]
    pub normalize_url: bool,

    /// Collapse URLs that differ only in variable data: identifier-looking path
    /// segments (numbers, UUIDs, hashes, dates) and query parameter *values*.
    /// `/post/1`, `/post/2` and `/post/99999` become one line. The survivor is
    /// the lexicographically smallest URL of each group, so runs are
    /// reproducible. Independent of --normalize-url and --merge-endpoint, and
    /// combines with either.
    #[clap(help_heading = "Output Options")]
    #[clap(long)]
    pub dedup_similar: bool,

    /// Providers to use (comma-separated, e.g., "wayback,cc,otx,arquivo,vt,urlscan")
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
    /// Common Crawl index to use (default: `latest`, the newest index resolved
    /// at runtime via collinfo.json so results don't age as a pinned index
    /// would). Accepts a comma-separated list to query multiple indexes in
    /// parallel (e.g. `CC-MAIN-2026-17,CC-MAIN-2025-51`).
    #[clap(long, default_value = "latest", value_delimiter = ',')]
    pub cc_index: Vec<String>,

    /// Restrict results to captures at or after this date, on every CDX-backed
    /// provider (wayback, cc, arquivo). Accepts YYYY, YYYYMM, YYYYMMDD, or the
    /// full 14-digit CDX timestamp; partial dates pad toward the start of the
    /// range.
    #[clap(help_heading = "Provider Options")]
    #[clap(long, alias = "wayback-from")]
    pub from: Option<String>,

    /// Restrict results to captures at or before this date, on every CDX-backed
    /// provider. Same format as --from; partial dates pad toward the end of the
    /// range.
    #[clap(help_heading = "Provider Options")]
    #[clap(long, alias = "wayback-to")]
    pub to: Option<String>,

    /// Keep only captures the archive recorded with this HTTP status code
    /// (e.g. "200"). Applied by the CDX index itself, so unlike
    /// --include-status it costs no extra requests. CDX-backed providers only
    /// (wayback, cc, arquivo). Wayback treats the value as a regex ("30." =
    /// any 3xx); cc and arquivo match exactly and cannot take a multi-value
    /// list here — urx warns and skips it for them.
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',')]
    pub archive_status: Vec<String>,

    /// Drop captures the archive recorded with these HTTP status codes
    /// (comma-separated, e.g. "404,500"). Unlike --archive-status, a
    /// multi-value list works on every CDX provider. See --archive-status.
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',')]
    pub archive_exclude_status: Vec<String>,

    /// Keep only captures with this recorded MIME type (e.g.
    /// "application/json"). Catches endpoints that carry no file extension,
    /// which -e/--extensions cannot. Same dialect caveats as --archive-status.
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',')]
    pub archive_mime: Vec<String>,

    /// Drop captures with these recorded MIME types (comma-separated, e.g.
    /// "text/html,image/png"). A multi-value list works on every CDX provider.
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',')]
    pub archive_exclude_mime: Vec<String>,

    #[clap(help_heading = "Provider Options")]
    /// API key for VirusTotal (can be used multiple times for rotation, can also use URX_VT_API_KEY environment variable with comma-separated keys)
    #[clap(long, action = clap::ArgAction::Append)]
    pub vt_api_key: Vec<String>,

    #[clap(help_heading = "Provider Options")]
    /// Optional API key for Urlscan. The provider also works anonymously (rate-limited to ~30 req/min per IP); a key only raises those limits. Can be used multiple times for rotation, or via URX_URLSCAN_API_KEY (comma-separated)
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

    /// Also read the *archived* versions of robots.txt and sitemap.xml the
    /// Wayback Machine holds — every distinct version, not just today's — so
    /// paths a site once listed and has since removed are recovered. Obeys
    /// --exclude-robots / --exclude-sitemap and --from / --to. Archived
    /// results are attributed to "robots.txt (archived)" / "sitemap.xml
    /// (archived)" under --show-sources and --stats.
    #[clap(long, help_heading = "Discovery Options")]
    pub archived_discovery: bool,

    /// Maximum archived documents fetched per domain by each of the archived
    /// robots.txt and sitemap providers (nested sitemaps count). The newest
    /// versions are read first.
    #[clap(
        long,
        help_heading = "Discovery Options",
        value_name = "N",
        default_value = "50"
    )]
    pub archived_discovery_limit: usize,

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

    /// Annotate each plain-text URL with the archive capture metadata
    /// (`first_seen`, `last_seen`, `mime`, `archive_status`, `digest`) the
    /// providers reported. JSON/JSONL/CSV always carry these fields when they
    /// have values, so this flag only affects plain output — which otherwise
    /// stays one bare URL per line for piping.
    #[clap(help_heading = "Display Options")]
    #[clap(long)]
    pub show_meta: bool,

    /// Print a per-provider summary (URLs found, errors, elapsed) to stderr
    /// when the run finishes.
    #[clap(help_heading = "Display Options")]
    #[clap(long)]
    pub stats: bool,

    /// Filter presets (comma-separated). Exclude a family: "no-resources",
    /// "no-images", "no-fonts", "no-documents", "no-videos", "no-audio".
    /// Keep only a family: "only-js", "only-style", "only-fonts",
    /// "only-documents", "only-videos", "only-audio", "only-images".
    /// Keep only a security-interesting family: "only-secrets", "only-backup",
    /// "only-config", "only-api" — these match by path shape as well as by
    /// extension, so `/.env` and `/index.php~` qualify.
    /// Singular spellings are accepted too. An unknown name is an error.
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

    /// Keep only URLs matching this regular expression. Repeat the flag for
    /// several expressions; a URL survives if it matches any of them. Matched
    /// against the whole URL and case-sensitively — unlike --patterns, which
    /// lower-cases both sides. Use `(?i)` for a case-insensitive pattern. Not
    /// comma-split, so a `{2,3}` quantifier stays intact.
    #[clap(help_heading = "Filter Options")]
    #[clap(long = "match-regex", value_name = "RE", action = clap::ArgAction::Append)]
    pub match_regex: Vec<String>,

    /// Drop URLs matching this regular expression. Repeat the flag for several
    /// expressions; one match is enough to drop the URL. Same matching rules as
    /// --match-regex, and applied before it.
    #[clap(help_heading = "Filter Options")]
    #[clap(long = "filter-regex", value_name = "RE", action = clap::ArgAction::Append)]
    pub filter_regex: Vec<String>,

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

    /// Fetch the *archived* body of each collected URL from the Wayback
    /// Machine and extract the links inside it. Unlike --extract-links this
    /// works for pages that no longer exist. Only URLs with a capture
    /// timestamp qualify (the CDX providers supply one; cached, --files and
    /// non-CDX URLs have none). One request per distinct body: URLs whose
    /// captures share a content digest are fetched once. Discovered links go
    /// through the same filters and host validation as everything else.
    #[clap(help_heading = "Testing Options")]
    #[clap(long)]
    pub archive_body: bool,

    /// Maximum number of archived bodies --archive-body fetches per run. This
    /// bounds distinct bodies, not URLs — duplicates never count against it.
    #[clap(help_heading = "Testing Options")]
    #[clap(long, value_name = "N", default_value = "500")]
    pub archive_body_limit: usize,

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

    /// Print a shell completion script to stdout and exit. Needs no DOMAINS,
    /// so `urx --completions zsh > ~/.zfunc/_urx` works on its own.
    #[clap(long, value_name = "SHELL", value_enum)]
    pub completions: Option<clap_complete::Shell>,

    /// Print the roff man page to stdout and exit, e.g.
    /// `urx --manpage > ~/.local/share/man/man1/urx.1`. Needs no DOMAINS.
    #[clap(long)]
    pub manpage: bool,
}

/// The set of options the user actually named on the command line.
///
/// The config layers need this. A parsed [`Args`] cannot tell `--retries 2`
/// from an unset `--retries`, so the old "does this field still equal its
/// default?" test treated an explicitly supplied value as absent and let the
/// config file overwrite it — the exact inverse of the documented
/// `CLI > config` precedence.
#[derive(Debug, Default, Clone)]
pub struct CliProvided {
    ids: HashSet<String>,
}

impl CliProvided {
    /// True when `id` was supplied on the command line. `id` is the clap
    /// argument id, which for this parser is the [`Args`] field name
    /// (`format`, `providers`, `cache_ttl`, ...) regardless of the flag's
    /// spelling.
    pub fn has(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}

/// Parse the process arguments, recording which options were given explicitly.
pub fn parse_args() -> (Args, CliProvided) {
    parse_args_from(std::env::args_os())
}

/// [`parse_args`] over an explicit argv. Parse failures are rendered and the
/// process exits exactly as [`Parser::parse_from`] would.
pub fn parse_args_from<I, T>(argv: I) -> (Args, CliProvided)
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let matches = Args::command().get_matches_from(argv);
    // Collected before `from_arg_matches`, which is free to consume `matches`.
    let ids = matches
        .ids()
        .map(|id| id.as_str().to_string())
        .filter(|id| matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine))
        .collect();
    let args = Args::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    (args, CliProvided { ids })
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

/// Drop a leading UTF-8 byte-order mark.
///
/// `str::trim` does not remove U+FEFF (it is not `White_Space`), so a domain
/// list saved by Notepad, Excel, or PowerShell's `>` redirect used to turn its
/// first entry into the host `"\u{feff}example.com"`. Every provider then
/// queried that literally and returned nothing, with no diagnostic — the run
/// simply looked like the archives had never seen the domain.
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Trim whitespace and drop blank / comment lines from a single text line.
fn parse_domain_line(line: &str) -> Option<String> {
    // The BOM has to go before the comment test too, or `\u{feff}# note` on
    // the first line reads as a domain instead of a comment.
    let trimmed = strip_bom(line.trim()).trim();
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
    // Also applied here, not just in `parse_domain_line`: a positional target
    // can be pasted with a BOM too, and `--files`-mode host validation
    // re-reads the raw target list.
    let trimmed = strip_bom(raw.trim()).trim();
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
    /// Parse `--rate-limit-by` entries into a `provider_id -> requests/sec` map,
    /// alongside the raw entries that did not parse.
    ///
    /// Every rejection here is otherwise invisible: `wayback` (no `=`),
    /// `wayback:5` (wrong separator), `wayback=fast`, and `wayback=0` all used to
    /// vanish, leaving the user with an unthrottled run they believe they
    /// limited. Unknown provider *ids* are already rejected loudly, as are
    /// unknown `--preset` values; malformed entries now travel back to the caller
    /// so they can be too.
    pub fn parse_rate_limit_overrides(
        &self,
    ) -> (std::collections::HashMap<String, f32>, Vec<String>) {
        let mut map = std::collections::HashMap::new();
        let mut malformed = Vec::new();
        for raw in &self.rate_limit_by {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Some((k, v)) = trimmed.split_once('=') else {
                malformed.push(trimmed.to_string());
                continue;
            };
            let id = k.trim().to_string();
            match v.trim().parse::<f32>() {
                Ok(rate) if !id.is_empty() && rate > 0.0 && rate.is_finite() => {
                    map.insert(id, rate);
                }
                _ => malformed.push(trimmed.to_string()),
            }
        }
        (map, malformed)
    }

    /// Just the successfully parsed overrides. See
    /// [`Args::parse_rate_limit_overrides`] for what gets rejected.
    pub fn rate_limit_overrides(&self) -> std::collections::HashMap<String, f32> {
        self.parse_rate_limit_overrides().0
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
        assert_eq!(args.cc_index, vec!["latest"]);
        assert_eq!(args.timeout, 120);
        assert_eq!(args.retries, 2);
        assert!(!args.archive_body);
        assert_eq!(args.archive_body_limit, 500);
        assert!(!args.archived_discovery);
        assert_eq!(args.archived_discovery_limit, 50);
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
        let (map, malformed) = args.parse_rate_limit_overrides();
        // "vt=oops" -> not a number, rejected
        // "nokey=1" -> kept, "nokey" -> 1 (an unknown id, rejected separately)
        // "=2" -> empty id, rejected
        // "wayback=-1" -> non-positive, rejected
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("nokey"), Some(&1.0));
        // ...and every rejection is reported rather than vanishing.
        assert_eq!(malformed, vec!["vt=oops", "=2", "wayback=-1"]);
    }

    #[test]
    fn test_rate_limit_overrides_report_every_silent_rejection() {
        // Each of these used to leave the run unthrottled with no diagnostic.
        for entry in ["wayback", "wayback:5", "wayback=fast", "wayback=0"] {
            let args = Args::parse_from(["urx", "--rate-limit-by", entry, "example.com"]);
            let (map, malformed) = args.parse_rate_limit_overrides();
            assert!(map.is_empty(), "{entry}: {map:?}");
            assert_eq!(malformed, vec![entry.to_string()], "{entry}");
        }
    }

    #[test]
    fn test_rate_limit_overrides_accept_valid_entries_without_complaint() {
        let args = Args::parse_from(["urx", "--rate-limit-by", "wayback=5", "example.com"]);
        let (map, malformed) = args.parse_rate_limit_overrides();
        assert_eq!(map.get("wayback"), Some(&5.0));
        assert!(malformed.is_empty());
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
    fn test_date_flags_parsed() {
        let args = Args::parse_from(["urx", "--from", "2020", "--to", "2023-06-30", "example.com"]);
        assert_eq!(args.from.as_deref(), Some("2020"));
        assert_eq!(args.to.as_deref(), Some("2023-06-30"));
    }

    #[test]
    fn test_legacy_wayback_date_aliases_still_parse() {
        // --from/--to replaced these once the date range stopped being
        // Wayback-only; existing command lines must keep working.
        let args = Args::parse_from([
            "urx",
            "--wayback-from",
            "2020",
            "--wayback-to",
            "2023-06-30",
            "example.com",
        ]);
        assert_eq!(args.from.as_deref(), Some("2020"));
        assert_eq!(args.to.as_deref(), Some("2023-06-30"));
    }

    #[test]
    fn test_archive_filter_flags_parsed() {
        let args = Args::parse_from([
            "urx",
            "--archive-status",
            "200,301",
            "--archive-exclude-status",
            "404",
            "--archive-mime",
            "application/json",
            "--archive-exclude-mime",
            "text/html,image/png",
            "example.com",
        ]);
        assert_eq!(args.archive_status, vec!["200", "301"]);
        assert_eq!(args.archive_exclude_status, vec!["404"]);
        assert_eq!(args.archive_mime, vec!["application/json"]);
        assert_eq!(args.archive_exclude_mime, vec!["text/html", "image/png"]);
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
    fn test_utf8_bom_is_stripped_from_domain_lines() {
        // Regression: `str::trim` leaves U+FEFF in place (it is not
        // `White_Space`), so the first entry of a BOM-prefixed domain list
        // became the host "\u{feff}example.com". That host is interpolated
        // straight into the CDX query string (`?url={domain}/*`), where reqwest
        // percent-encodes it to %EF%BB%BF and every archive returns nothing —
        // and strict host validation then rejects any URL that did come back.
        // Both failures are silent: the run just looks like an empty archive.
        assert_eq!(
            parse_domain_line("\u{feff}example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            normalize_domain("\u{feff}example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_domain("\u{feff}https://example.com/path").as_deref(),
            Some("example.com")
        );
        // A BOM in front of a comment marker must not turn the comment into a
        // target either.
        assert_eq!(parse_domain_line("\u{feff}# a note"), None);
        // A BOM-only line is blank.
        assert_eq!(parse_domain_line("\u{feff}"), None);
        assert_eq!(normalize_domain("\u{feff}"), None);
    }

    #[test]
    fn test_read_domains_from_file_handles_bom_and_crlf() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Exactly what Notepad / Excel / PowerShell `>` produce.
        file.write_all("\u{feff}example.com\r\n# note\r\nanother.test\r\n".as_bytes())
            .unwrap();
        let domains = read_domains_from_file(file.path()).unwrap();
        assert_eq!(domains, vec!["example.com", "another.test"]);
    }

    #[test]
    fn test_parse_args_records_only_command_line_options() {
        // The whole point of `CliProvided`: a flag whose value equals the clap
        // default is still an explicit choice, and the config layers must be
        // able to see that.
        let (args, provided) = parse_args_from(["urx", "example.com", "--format", "plain"]);
        assert_eq!(args.format, "plain");
        assert!(provided.has("format"), "--format was typed");
        assert!(!provided.has("retries"), "--retries was not");
        assert!(!provided.has("providers"));

        let (_, provided) = parse_args_from(["urx", "example.com"]);
        assert!(
            !provided.has("format"),
            "an untouched default must not look supplied"
        );

        // Defaults that happen to match what the user typed still count.
        let (_, provided) = parse_args_from([
            "urx",
            "example.com",
            "--retries",
            "2",
            "--providers",
            "wayback,cc,otx",
            "--cache-ttl",
            "86400",
        ]);
        for id in ["retries", "providers", "cache_ttl"] {
            assert!(provided.has(id), "{id} was supplied on the command line");
        }
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
