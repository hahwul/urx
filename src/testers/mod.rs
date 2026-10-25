use anyhow::Result;
use async_trait::async_trait;

use crate::output::UrlData;

mod archive_body;
mod body_archive;
mod js_endpoint_extractor;
mod link_extractor;
mod shared;
mod spec_expander;
mod status_checker;

pub use archive_body::{ArchiveBodyExtractor, ArchiveBodyStats, ArchiveCapture};
pub use body_archive::BodyArchive;
pub use js_endpoint_extractor::JsEndpointExtractor;
pub use link_extractor::LinkExtractor;
pub use spec_expander::SpecExpander;
pub use status_checker::StatusChecker;

/// Tester trait for URL testing operations
///
/// This trait defines common operations for classes that test URLs by fetching
/// or analyzing them and returning results.
#[async_trait]
pub trait Tester: Send + Sync {
    /// Create a boxed clone of this tester
    fn clone_box(&self) -> Box<dyn Tester>;

    /// Test a URL: the status checker returns the URL with its status, the
    /// extractors the URLs they found in its body.
    async fn test_url(&self, url: &str) -> Result<Vec<UrlData>>;

    /// Apply proxy, timeout, TLS, retries, rate limit and the user's `-H`
    /// headers. A no-op by default, for the test doubles that make no request.
    fn with_network(&mut self, _net: crate::network::NetConfig) {}
}

/// The URLs a tester returned, for assertions.
#[cfg(test)]
pub(crate) fn urls(found: Vec<UrlData>) -> Vec<String> {
    found.into_iter().map(|d| d.url).collect()
}
