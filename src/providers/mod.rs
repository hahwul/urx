use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

mod api_key_rotation;
mod commoncrawl;
mod otx;
mod robots;
mod sitemap;
mod urlscan;
mod vt;
mod wayback;
pub use api_key_rotation::ApiKeyRotator;
pub use commoncrawl::CommonCrawlProvider;
pub use otx::OTXProvider;
pub use robots::RobotsProvider;
pub use sitemap::SitemapProvider;
pub use urlscan::UrlscanProvider;
pub use vt::VirusTotalProvider;
pub use wayback::WaybackMachineProvider;

/// Provider trait for URL discovery services
///
/// This trait defines common operations for classes that fetch URLs
/// from various external sources like archives and crawlers.
pub trait Provider: Send + Sync {
    /// Create a boxed clone of this provider
    fn clone_box(&self) -> Box<dyn Provider>;

    /// Fetch URLs for a given domain from the provider
    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

    // Configuration options
    /// Include or exclude subdomains in the search
    fn with_subdomains(&mut self, include: bool);

    /// Set the proxy server for HTTP requests
    fn with_proxy(&mut self, proxy: Option<String>);

    /// Set the proxy authentication credentials (username:password)
    fn with_proxy_auth(&mut self, auth: Option<String>);

    /// Set the request timeout in seconds
    fn with_timeout(&mut self, seconds: u64);

    /// Set the number of retry attempts for failed requests
    fn with_retries(&mut self, count: u32);

    /// Enable or disable the use of random User-Agent headers
    fn with_random_agent(&mut self, enabled: bool);

    /// Enable or disable SSL certificate verification (for self-signed certificates)
    fn with_insecure(&mut self, enabled: bool);

    /// Set the number of parallel requests
    fn with_parallel(&mut self, count: u32);

    /// Set rate limiting to avoid being blocked by providers
    fn with_rate_limit(&mut self, requests_per_second: Option<f32>);
}
