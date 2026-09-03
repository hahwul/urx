use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

mod api_key_rotation;
mod arquivo;
mod commoncrawl;
pub mod filters;
mod github;
mod otx;
mod record;
mod robots;
mod sitemap;
mod urlscan;
mod vt;
pub mod wayback;
mod zoomeye;
pub use api_key_rotation::ApiKeyRotator;
pub use arquivo::ArquivoProvider;
pub use commoncrawl::CommonCrawlProvider;
pub use filters::{normalize_cdx_timestamp, ArchiveFilters, CdxDialect};
pub use github::GitHubProvider;
pub use otx::OTXProvider;
pub use record::{CaptureMeta, RecordSet, UrlRecord};
pub use robots::RobotsProvider;
pub use sitemap::SitemapProvider;
pub use urlscan::UrlscanProvider;
pub use vt::VirusTotalProvider;
pub use wayback::WaybackMachineProvider;
pub use zoomeye::ZoomEyeProvider;

/// Provider trait for URL discovery services
///
/// This trait defines common operations for classes that fetch URLs
/// from various external sources like archives and crawlers.
pub trait Provider: Send + Sync {
    /// Create a boxed clone of this provider
    fn clone_box(&self) -> Box<dyn Provider>;

    /// Fetch URLs for a given domain from the provider.
    ///
    /// One [`UrlRecord`] per distinct URL, carrying whatever archive metadata
    /// the provider had. Providers without a capture index return
    /// [`UrlRecord::bare`] records rather than inventing values.
    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlRecord>>> + Send + 'a>>;

    /// Fetch URLs while optionally reporting fine-grained progress (e.g. a
    /// paginating provider can surface "page 3/12") through `reporter`.
    ///
    /// The default implementation ignores the reporter and delegates to
    /// [`Provider::fetch_urls`], so providers that have nothing interesting to
    /// report need not implement it.
    fn fetch_urls_with_progress<'a>(
        &'a self,
        domain: &'a str,
        _reporter: Option<crate::progress::ProgressReporter>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlRecord>>> + Send + 'a>> {
        self.fetch_urls(domain)
    }

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

    /// Set rate limiting to avoid being blocked by providers
    fn with_rate_limit(&mut self, requests_per_second: Option<f32>);
}

/// Test helper: reduce a provider result to plain URL strings. Most provider
/// tests assert on *which* URLs came back, not on their capture metadata.
#[cfg(test)]
pub(crate) fn urls_of(records: Vec<UrlRecord>) -> Vec<String> {
    records.into_iter().map(|r| r.url).collect()
}
