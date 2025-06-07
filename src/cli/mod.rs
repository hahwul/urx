use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(name = "urx", version)]
pub struct Args {
    /// Domains to fetch URLs for
    #[clap(name = "DOMAINS")]
    pub domains: Vec<String>,

    /// Config file to load
    #[clap(short, long, value_parser)]
    pub config: Option<PathBuf>,

    #[clap(help_heading = "Output Options")]
    /// Output file to write results
    #[clap(short, long, value_parser)]
    pub output: Option<PathBuf>,

    /// Output format (e.g., "plain", "json", "csv")
    #[clap(help_heading = "Output Options")]
    #[clap(short, long, default_value = "plain")]
    pub format: String,

    /// Merge endpoints with the same path and merge URL parameters
    #[clap(help_heading = "Output Options")]
    #[clap(long)]
    pub merge_endpoint: bool,

    /// Providers to use (comma-separated, e.g., "wayback,cc,otx,vt,urlscan")
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',', default_value = "wayback,cc,otx")]
    pub providers: Vec<String>,

    /// Include subdomains when searching
    #[clap(help_heading = "Provider Options")]
    #[clap(long)]
    pub subs: bool,

    #[clap(help_heading = "Provider Options")]
    /// Common Crawl index to use (e.g., CC-MAIN-2025-13)
    #[clap(long, default_value = "CC-MAIN-2025-13")]
    pub cc_index: String,

    #[clap(help_heading = "Provider Options")]
    /// API key for VirusTotal (can also use URX_VT_API_KEY environment variable)
    #[clap(long)]
    pub vt_api_key: Option<String>,

    #[clap(help_heading = "Provider Options")]
    /// API key for Urlscan (can also use URX_URLSCAN_API_KEY environment variable)
    #[clap(long)]
    pub urlscan_api_key: Option<String>,

    #[clap(help_heading = "Provider Options")]
    /// Include robots.txt discovery
    #[clap(long)]
    pub include_robots: bool,

    #[clap(help_heading = "Provider Options")]
    /// Include sitemap.xml discovery
    #[clap(long)]
    pub include_sitemap: bool,

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
    #[clap(long, default_value = "120")]
    pub timeout: u64,

    /// Number of retries for failed requests
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "2")]
    pub retries: u32,

    /// Maximum number of parallel requests per provider and maximum concurrent domain processing
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "5")]
    pub parallel: Option<u32>,

    /// Rate limit (requests per second)
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub rate_limit: Option<f32>,

    /// Check HTTP status code of collected URLs
    #[clap(help_heading = "Testing Options")]
    #[clap(long, alias = "cs", visible_alias = "--cs")]
    pub check_status: bool,

    /// Include URLs with specific HTTP status codes or patterns (e.g., --is=200,30x)
    #[clap(help_heading = "Testing Options")]
    #[clap(long, alias = "is", visible_alias = "--is")]
    pub include_status: Vec<String>,

    /// Exclude URLs with specific HTTP status codes or patterns (e.g., --es=404,50x,5xx)
    #[clap(help_heading = "Testing Options")]
    #[clap(long, alias = "es", visible_alias = "--es")]
    pub exclude_status: Vec<String>,

    /// Extract additional links from collected URLs (requires HTTP requests)
    #[clap(help_heading = "Testing Options")]
    #[clap(long)]
    pub extract_links: bool,
}

pub fn read_domains_from_stdin() -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    use std::io::{self, BufRead};

    let stdin = io::stdin();
    let mut domains = Vec::new();

    for line in stdin.lock().lines() {
        let domain = line.context("Failed to read line from stdin")?;
        if !domain.trim().is_empty() {
            domains.push(domain.trim().to_string());
        }
    }

    Ok(domains)
}

fn validate_network_scope(s: &str) -> Result<String, String> {
    match s {
        "all" | "providers" | "testers" | "providers,testers" | "testers,providers" => Ok(s.to_string()),
        _ => Err(format!("Invalid network scope: {}. Allowed values are all, providers, testers, or providers,testers", s)),
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
        assert_eq!(args.cc_index, "CC-MAIN-2025-13");
        assert_eq!(args.timeout, 120);
        assert_eq!(args.retries, 2);
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
