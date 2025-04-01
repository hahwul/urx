use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

mod link_extractor;
mod status_checker;

pub use link_extractor::LinkExtractor;
pub use status_checker::StatusChecker;

/// Tester trait for URL testing operations
///
/// This trait defines common operations for classes that test URLs by fetching
/// or analyzing them and returning results.
pub trait Tester: Send + Sync {
    /// Create a boxed clone of this tester
    fn clone_box(&self) -> Box<dyn Tester>;

    /// Test a URL and return results as strings
    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

    // Configuration options
    /// Set the request timeout in seconds
    fn with_timeout(&mut self, seconds: u64);

    /// Set the number of retry attempts for failed requests
    fn with_retries(&mut self, count: u32);

    /// Enable or disable the use of random User-Agent headers
    fn with_random_agent(&mut self, enabled: bool);

    /// Enable or disable SSL certificate verification (for self-signed certificates)
    fn with_insecure(&mut self, enabled: bool);

    /// Set the proxy server for HTTP requests
    fn with_proxy(&mut self, proxy: Option<String>);

    /// Set the proxy authentication credentials (username:password)
    fn with_proxy_auth(&mut self, auth: Option<String>);
}
