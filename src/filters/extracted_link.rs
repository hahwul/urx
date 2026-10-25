use super::{HostValidator, UrlFilter};
use crate::utils::UrlTransformer;

/// The filtering a URL discovered by `--extract-links`, `--extract-js-endpoints`
/// or `--archive-body` has to pass.
///
/// The primary URL list goes through the filters, host validation, and the
/// `show_only_*`/`--normalize-url` views *before* testing starts. Links found
/// inside those pages only exist afterwards, so without applying the same rules
/// here `--extract-links` silently bypasses every filter the user set: `-e js`
/// would emit non-JS links, and strict mode (on by default) would emit every
/// off-site link a page happens to point at — ads, CDNs, social buttons.
pub struct ExtractedLinkFilter {
    filter: UrlFilter,
    transformer: UrlTransformer,
    host_validator: Option<HostValidator>,
}

impl ExtractedLinkFilter {
    pub fn new(
        filter: UrlFilter,
        transformer: UrlTransformer,
        host_validator: Option<HostValidator>,
    ) -> Self {
        ExtractedLinkFilter {
            filter,
            transformer,
            host_validator,
        }
    }

    /// The surviving, transformed form of `url`, or `None` if it is filtered out.
    pub fn accept(&self, url: &str) -> Option<String> {
        if !self.filter.matches(url) {
            return None;
        }
        if let Some(v) = &self.host_validator {
            if !v.is_valid_host(url) {
                return None;
            }
        }
        self.transformer.transform_one(url)
    }
}
