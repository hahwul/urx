use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

use crate::output::UrlData;

mod archive_body;
mod body_archive;
mod js_endpoint_extractor;
mod link_extractor;
mod status_checker;
// --- spec-expansion ---
mod shared;
mod spec_expander;

pub use archive_body::{ArchiveBodyExtractor, ArchiveBodyStats, ArchiveCapture};
pub use body_archive::BodyArchive;
pub use js_endpoint_extractor::JsEndpointExtractor;
pub use link_extractor::LinkExtractor;
pub use status_checker::StatusChecker;
// --- spec-expansion ---
pub use spec_expander::SpecExpander;

/// Tester trait for URL testing operations
///
/// This trait defines common operations for classes that test URLs by fetching
/// or analyzing them and returning results.
pub trait Tester: Send + Sync {
    /// Create a boxed clone of this tester
    fn clone_box(&self) -> Box<dyn Tester>;

    /// Test a URL: the status checker returns the URL with its status, the
    /// extractors the URLs they found in its body.
    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlData>>> + Send + 'a>>;

    /// Apply proxy, timeout, TLS, retries, rate limit and the user's `-H`
    /// headers. A no-op by default, for the test doubles that make no request.
    fn with_network(&mut self, _net: crate::network::NetConfig) {}
}

/// The URLs a tester returned, for assertions.
#[cfg(test)]
pub(crate) fn urls(found: Vec<UrlData>) -> Vec<String> {
    found.into_iter().map(|d| d.url).collect()
}
