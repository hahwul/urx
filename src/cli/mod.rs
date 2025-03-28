use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(
    name = "urx",
    about = "Extracts URLs from OSINT Archives for Security Insights.",
    version
)]
pub struct Args {
    /// Domains to fetch URLs for
    #[clap(name = "DOMAINS")]
    pub domains: Vec<String>,

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

    #[clap(help_heading = "Provider Options")]
    /// Common Crawl index to use (e.g., CC-MAIN-2025-08)
    #[clap(long, default_value = "CC-MAIN-2025-08")]
    pub cc_index: String,

    /// Providers to use (comma-separated, e.g., "wayback,cc,otx")
    #[clap(help_heading = "Provider Options")]
    #[clap(long, value_delimiter = ',', default_value = "wayback,cc")]
    pub providers: Vec<String>,

    /// Include subdomains when searching
    #[clap(help_heading = "Provider Options")]
    #[clap(long)]
    pub subs: bool,

    #[clap(help_heading = "Display Options")]
    /// Show verbose output
    #[clap(short, long)]
    pub verbose: bool,

    #[clap(help_heading = "Filter Options")]
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

    #[clap(help_heading = "Network Options")]
    /// Use proxy for HTTP requests (format: http://proxy.example.com:8080)
    #[clap(long)]
    pub proxy: Option<String>,

    /// Proxy authentication credentials (format: username:password)
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub proxy_auth: Option<String>,

    /// Use a random User-Agent for HTTP requests
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub random_agent: bool,

    /// Request timeout in seconds
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "30")]
    pub timeout: u64,

    /// Number of retries for failed requests
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "3")]
    pub retries: u32,

    /// Maximum number of parallel requests
    #[clap(help_heading = "Network Options")]
    #[clap(long, default_value = "5")]
    pub parallel: u32,

    /// Rate limit (requests per second)
    #[clap(help_heading = "Network Options")]
    #[clap(long)]
    pub rate_limit: Option<f32>,

    /// Check HTTP status code of collected URLs
    #[clap(help_heading = "Testing Options")]
    #[clap(long)]
    pub check_status: bool,

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
