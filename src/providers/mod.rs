use anyhow::Result;
use async_trait::async_trait;

use crate::progress::ProgressReporter;

mod api_key_rotation;
pub mod archived;
mod arquivo;
mod bevigil;
pub mod cdx;
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
pub use bevigil::BeVigilProvider;
pub use cdx::CdxProvider;
pub use commoncrawl::CommonCrawlProvider;
pub use filters::{cdx_url_pattern, normalize_cdx_timestamp, ArchiveFilters, CdxDialect};
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
#[async_trait]
pub trait Provider: Send + Sync {
    /// Create a boxed clone of this provider
    fn clone_box(&self) -> Box<dyn Provider>;

    /// Fetch URLs for a given domain from the provider, reporting
    /// fine-grained progress (e.g. a paginating provider can surface
    /// "page 3/12") through `reporter`.
    ///
    /// One [`UrlRecord`] per distinct URL, carrying whatever archive metadata
    /// the provider had. Providers without a capture index return
    /// [`UrlRecord::bare`] records rather than inventing values.
    async fn fetch_urls_with_progress(
        &self,
        domain: &str,
        reporter: ProgressReporter,
    ) -> Result<Vec<UrlRecord>>;

    /// Test shorthand: fetch with a reporter nobody watches.
    #[cfg(test)]
    async fn fetch_urls(&self, domain: &str) -> Result<Vec<UrlRecord>> {
        self.fetch_urls_with_progress(domain, ProgressReporter::hidden())
            .await
    }

    // Configuration options
    /// Include or exclude subdomains in the search
    fn with_subdomains(&mut self, include: bool);

    /// Apply proxy, timeout, TLS, User-Agent, retry and rate-limit settings.
    fn with_network(&mut self, net: crate::network::NetConfig);

    /// Whether this provider's own query can express a target's path scope.
    ///
    /// A CDX index can: `url=example.com/shop*` is native prefix matching, so
    /// the archive never sends the rows outside the scope. Nothing else urx
    /// queries can — OTX, VirusTotal, urlscan and the rest take a hostname —
    /// and neither can robots.txt or sitemap.xml, which live at the root
    /// whatever the scope is.
    ///
    /// The default is therefore `false`, and a provider that says so is handed
    /// the bare host; [`crate::filters::HostValidator`] applies the path scope
    /// to whatever comes back. Defaulting the safe way round matters: a new
    /// provider handed `example.com/shop` where it expected a hostname would
    /// quietly return nothing at all.
    fn accepts_path_scope(&self) -> bool {
        false
    }

    /// Send the user's `-H` / `--cookie` / `--user-agent` headers.
    ///
    /// The default does nothing, and that is the right default: it is taken by
    /// every component whose requests go to an *archive* or a third-party API
    /// rather than to the target. Handing a target's `Authorization` header to
    /// web.archive.org would mail the user's credentials to a service that
    /// keeps what it receives, so only the components that fetch from the
    /// target itself override this. See [`crate::network::CustomHeaders`].
    fn with_headers(&mut self, _headers: crate::network::CustomHeaders) {}
}

/// Test helper: reduce a provider result to plain URL strings. Most provider
/// tests assert on *which* URLs came back, not on their capture metadata.
#[cfg(test)]
pub(crate) fn urls_of(records: Vec<UrlRecord>) -> Vec<String> {
    records.into_iter().map(|r| r.url).collect()
}
