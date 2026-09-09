//! Small helpers shared by more than one tester.
//!
//! Only things a tester already used and another needs *verbatim* belong here.
//! The noise policies stay with the tester whose regression tests define them:
//! `js_endpoint_extractor::is_noise` exists because minified bundles produce
//! specific garbage shapes, and none of that applies to a document that lists
//! its endpoints outright.

use url::Url;

/// The extension of the last path segment of `url`, lower-cased, if any.
///
/// Lifted unchanged out of `js_endpoint_extractor` when `spec_expander` needed
/// the same test: both decide whether a URL is worth a request from the shape
/// of its file name, and both must agree that `.htaccess`, a trailing dot, and
/// a long non-alphanumeric tail are not extensions.
pub(super) fn path_extension(url: &Url) -> Option<String> {
    let last = url.path_segments()?.next_back()?;
    let (_, ext) = last.rsplit_once('.')?;
    // `.htaccess`-style names and trailing dots are not extensions.
    if ext.is_empty() || ext.len() > 5 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_extension_reads_only_real_extensions() {
        let ext = |u: &str| path_extension(&Url::parse(u).unwrap());
        assert_eq!(ext("https://x/a/swagger.json").as_deref(), Some("json"));
        assert_eq!(ext("https://x/OPENAPI.YAML").as_deref(), Some("yaml"));
        // No extension at all, a bare dotfile, a trailing dot, and a tail too
        // long or too odd to be one.
        assert_eq!(ext("https://x/v3/api-docs"), None);
        assert_eq!(ext("https://x/.htaccess"), None);
        assert_eq!(ext("https://x/name."), None);
        assert_eq!(ext("https://x/a.verylong"), None);
        assert_eq!(ext("https://x/a.js%20x"), None);
    }
}
