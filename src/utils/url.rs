use std::collections::{HashMap, HashSet};
use url::Url;

/// Utility for transforming and manipulating URL collections
///
/// Provides methods for merging, filtering, and extracting parts of URLs.
pub struct UrlTransformer {
    merge_endpoint: bool,
    show_only_host: bool,
    show_only_path: bool,
    show_only_param: bool,
    normalize_url: bool,
}

impl UrlTransformer {
    /// Creates a new URL transformer with default settings
    pub fn new() -> Self {
        UrlTransformer {
            merge_endpoint: false,
            show_only_host: false,
            show_only_path: false,
            show_only_param: false,
            normalize_url: false,
        }
    }

    /// Enables or disables merging of endpoints with the same path but different parameters
    pub fn with_merge_endpoint(&mut self, merge: bool) -> &mut Self {
        self.merge_endpoint = merge;
        self
    }

    /// When enabled, shows only the hostname part of URLs
    pub fn with_show_only_host(&mut self, show: bool) -> &mut Self {
        self.show_only_host = show;
        self
    }

    /// When enabled, shows only the path part of URLs
    pub fn with_show_only_path(&mut self, show: bool) -> &mut Self {
        self.show_only_path = show;
        self
    }

    /// When enabled, shows only the query parameters of URLs
    pub fn with_show_only_param(&mut self, show: bool) -> &mut Self {
        self.show_only_param = show;
        self
    }

    /// When enabled, normalizes URLs for better deduplication
    /// Sorts query parameters alphabetically and normalizes paths
    pub fn with_normalize_url(&mut self, normalize: bool) -> &mut Self {
        self.normalize_url = normalize;
        self
    }

    /// Transforms a list of URLs according to the configured settings
    pub fn transform(&self, urls: Vec<String>) -> Vec<String> {
        let mut transformed_urls = urls;

        // Normalize URLs if requested (should happen before merging)
        if self.normalize_url {
            transformed_urls = self.normalize_urls(transformed_urls);
        }

        // Merge endpoints if requested
        if self.merge_endpoint {
            transformed_urls = self.merge_endpoints(transformed_urls);
        }

        // Extract URL parts if any show_only option is enabled
        if self.show_only_host || self.show_only_path || self.show_only_param {
            transformed_urls = self.extract_url_parts(transformed_urls);
        }

        transformed_urls
    }

    /// Transform a single URL the way [`UrlTransformer::transform`] would,
    /// minus `merge_endpoint` (which is inherently cross-URL and so has no
    /// single-URL equivalent). Returns `None` when the URL carries nothing for
    /// the configured `show_only_*` view — e.g. `--show-only-param` on a URL
    /// with no query.
    ///
    /// Used by streaming output, where each URL must be decided on arrival.
    pub fn transform_one(&self, url: &str) -> Option<String> {
        let normalized = if self.normalize_url {
            self.normalize_one(url)
        } else {
            url.to_string()
        };

        if self.show_only_host || self.show_only_path || self.show_only_param {
            self.extract_parts_one(&normalized)
        } else {
            Some(normalized)
        }
    }

    /// Normalise one URL: drop a trailing slash and sort query parameters.
    /// Unparseable input is passed through untouched.
    fn normalize_one(&self, url_str: &str) -> String {
        match Url::parse(url_str) {
            Ok(mut url) => {
                // Normalize the path - remove trailing slash if it's not just "/"
                let path = url.path().to_string();
                if path.len() > 1 {
                    if let Some(normalized_path) = path.strip_suffix('/') {
                        url.set_path(normalized_path);
                    }
                }

                // Normalize query parameters by sorting them. We sort the *raw*
                // `key=value` tokens without decoding, so this stays a lossless
                // reordering: a bare `?foo` is not rewritten to `?foo=`, and a
                // literal '+' is not turned into '%20' (query_pairs() decodes
                // both, which silently mutates the URL the archive recorded).
                let sorted_query: Option<String> = url.query().map(|query| {
                    let mut pairs: Vec<&str> = query.split('&').filter(|s| !s.is_empty()).collect();
                    pairs.sort_unstable();
                    pairs.join("&")
                });
                if let Some(query) = sorted_query {
                    url.set_query(None);
                    if !query.is_empty() {
                        url.set_query(Some(&query));
                    }
                }

                url.to_string()
            }
            // If URL can't be parsed, keep it as is
            Err(_) => url_str.to_string(),
        }
    }

    fn normalize_urls(&self, urls: Vec<String>) -> Vec<String> {
        let mut normalized_urls: Vec<String> = urls.iter().map(|u| self.normalize_one(u)).collect();

        // Remove duplicates that might have been created during normalization
        normalized_urls.sort();
        normalized_urls.dedup();

        normalized_urls
    }

    fn merge_endpoints(&self, urls: Vec<String>) -> Vec<String> {
        let mut path_groups: HashMap<String, Vec<String>> = HashMap::new();

        for url_str in urls {
            if let Ok(url) = Url::parse(&url_str) {
                // Key on the full origin plus the path. Keying on host+path alone
                // merged endpoints that are not the same endpoint at all:
                // `http://host/api?a=1` and `https://host/api?b=2` collapsed into a
                // single URL that carried one scheme and both origins' parameters,
                // and `host:8080/api` was folded into `host/api` the same way.
                let key = format!(
                    "{}://{}{}{}",
                    url.scheme(),
                    url.host_str().unwrap_or(""),
                    url.port().map(|p| format!(":{p}")).unwrap_or_default(),
                    url.path()
                );

                path_groups.entry(key).or_default().push(url_str);
            } else {
                // If URL can't be parsed, keep it as is
                path_groups
                    .entry(url_str.clone())
                    .or_default()
                    .push(url_str);
            }
        }

        // Now create merged URLs
        let mut merged_urls = Vec::new();

        for (_, group_urls) in path_groups {
            if group_urls.len() == 1 {
                // If only one URL with this path, use it as is
                merged_urls.push(group_urls[0].clone());
            } else {
                // Merge parameters from all URLs with the same path
                if let Ok(base_url) = Url::parse(&group_urls[0]) {
                    let mut merged_url = base_url.clone();
                    let mut all_params: Vec<String> = Vec::new();
                    let mut seen_params = HashSet::new();

                    // Collect parameters from all URLs. We copy the *raw*
                    // `key=value` tokens rather than decoding via
                    // `query_pairs()`: decoding and re-joining silently rewrites
                    // the URL the archive recorded — a value containing an
                    // encoded `&` or `=` (`?next=%2Fa%3Fb%3D1`) would come back
                    // out as extra parameters, `+` would turn into a space, and
                    // a bare `?foo` would gain an `=`. Merging must only add
                    // parameters, never alter them.
                    for url_str in &group_urls {
                        if let Ok(url) = Url::parse(url_str) {
                            for pair in url
                                .query()
                                .unwrap_or("")
                                .split('&')
                                .filter(|s| !s.is_empty())
                            {
                                if seen_params.insert(pair.to_string()) {
                                    all_params.push(pair.to_string());
                                }
                            }
                        }
                    }

                    // Set merged parameters
                    if !all_params.is_empty() {
                        let query_string = all_params.join("&");

                        // Clear existing query and set merged query
                        merged_url.set_query(None);
                        merged_url.set_query(Some(&query_string));
                    }

                    merged_urls.push(merged_url.to_string());
                } else {
                    // If URL can't be parsed, use the first one
                    merged_urls.push(group_urls[0].clone());
                }
            }
        }

        // Sort again for consistency
        merged_urls.sort();
        merged_urls
    }

    /// Reduce one URL to the configured `show_only_*` view. `None` means the
    /// URL has nothing to show for that view (no host, a bare `/` path, or no
    /// query) and is therefore dropped.
    fn extract_parts_one(&self, url_str: &str) -> Option<String> {
        match Url::parse(url_str) {
            Ok(url) => {
                if self.show_only_host {
                    url.host_str().map(|h| h.to_string())
                } else if self.show_only_path {
                    if url.path() != "/" {
                        Some(url.path().to_string())
                    } else {
                        None
                    }
                } else if self.show_only_param {
                    url.query().map(|q| q.to_string())
                } else {
                    Some(url_str.to_string())
                }
            }
            // If URL can't be parsed, keep it as is
            Err(_) => Some(url_str.to_string()),
        }
    }

    fn extract_url_parts(&self, urls: Vec<String>) -> Vec<String> {
        let mut extracted_parts: Vec<String> = urls
            .iter()
            .filter_map(|u| self.extract_parts_one(u))
            .collect();

        // Remove duplicates that might have been created during transformation
        extracted_parts.sort();
        extracted_parts.dedup();

        extracted_parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_transformer_merge_endpoints() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/api?param1=value1".to_string(),
            "https://example.com/api?param2=value2".to_string(),
            "https://example.com/api?param3=value3".to_string(),
            "https://other.com/path".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert!(transformed.contains(
            &"https://example.com/api?param1=value1&param2=value2&param3=value3".to_string()
        ));
        assert!(transformed.contains(&"https://other.com/path".to_string()));
    }

    #[test]
    fn test_merge_endpoints_preserves_encoded_values() {
        // Regression: merging used to decode each pair with query_pairs() and
        // re-join the results raw, so an encoded '&' or '=' inside a value broke
        // out and became extra parameters — silently rewriting the URL the
        // archive actually recorded.
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/go?next=%2Fadmin%3Fdebug%3D1".to_string(),
            "https://example.com/go?ref=home".to_string(),
        ];

        let out = transformer.transform(urls);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(
            out[0],
            "https://example.com/go?next=%2Fadmin%3Fdebug%3D1&ref=home"
        );
    }

    #[test]
    fn test_merge_endpoints_preserves_plus_and_bare_params() {
        // '+' must not become a space, and a valueless `?foo` must not gain an
        // '=' — both were casualties of the decode/re-encode round trip.
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/s?q=a+b".to_string(),
            "https://example.com/s?debug".to_string(),
        ];

        let out = transformer.transform(urls);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0], "https://example.com/s?q=a+b&debug");
    }

    #[test]
    fn test_merge_endpoints_keeps_origins_apart() {
        // Regression: the group key was host+path, so a plain-HTTP endpoint and
        // its HTTPS counterpart (and a non-default port) merged into one URL —
        // inventing an endpoint that carried parameters never seen on that origin.
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "http://example.com/api?a=1".to_string(),
            "https://example.com/api?b=2".to_string(),
            "https://example.com:8443/api?c=3".to_string(),
        ];

        let out = transformer.transform(urls);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(
            out.contains(&"http://example.com/api?a=1".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com/api?b=2".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com:8443/api?c=3".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn test_merge_endpoints_still_merges_same_origin() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/api?a=1".to_string(),
            "https://example.com/api?b=2".to_string(),
        ];

        let out = transformer.transform(urls);
        assert_eq!(out, vec!["https://example.com/api?a=1&b=2".to_string()]);
    }

    #[test]
    fn test_url_transformer_show_only_host() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_host(true);

        let urls = vec![
            "https://example.com/path1".to_string(),
            "https://example.com/path2".to_string(),
            "https://other.com/path".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 2); // Duplicates should be removed
        assert!(transformed.contains(&"example.com".to_string()));
        assert!(transformed.contains(&"other.com".to_string()));
    }

    #[test]
    fn test_url_transformer_show_only_path() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_path(true);

        let urls = vec![
            "https://example.com/path1".to_string(),
            "https://example.com/path2".to_string(),
            "https://other.com/path1".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 2); // Duplicates should be removed
        assert!(transformed.contains(&"/path1".to_string()));
        assert!(transformed.contains(&"/path2".to_string()));
    }

    #[test]
    fn test_url_transformer_show_only_param() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_param(true);

        let urls = vec![
            "https://example.com/api?param1=value1".to_string(),
            "https://example.com/api?param2=value2".to_string(),
            "https://other.com/api?param1=value1".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 2); // Duplicates should be removed
        assert!(transformed.contains(&"param1=value1".to_string()));
        assert!(transformed.contains(&"param2=value2".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_query_params() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api?b=2&a=1".to_string(),
            "https://example.com/api?a=1&b=2".to_string(),
            "https://example.com/path?z=3&y=2&x=1".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 2); // First two should be deduplicated
        assert!(transformed.contains(&"https://example.com/api?a=1&b=2".to_string()));
        assert!(transformed.contains(&"https://example.com/path?x=1&y=2&z=3".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_trailing_slashes() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api/".to_string(),
            "https://example.com/api".to_string(),
            "https://example.com/path/subpath/".to_string(),
            "https://example.com/path/subpath".to_string(),
            "https://example.com/".to_string(), // Root path should keep trailing slash
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 3); // Should deduplicate trailing slash variants
        assert!(transformed.contains(&"https://example.com/".to_string())); // Root keeps slash
        assert!(transformed.contains(&"https://example.com/api".to_string()));
        assert!(transformed.contains(&"https://example.com/path/subpath".to_string()));
        assert!(!transformed.contains(&"https://example.com/api/".to_string()));
        assert!(!transformed.contains(&"https://example.com/path/subpath/".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_complex() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api/?c=3&b=2&a=1".to_string(),
            "https://example.com/api?a=1&b=2&c=3".to_string(),
            "https://example.com/api/?a=1&c=3&b=2".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 1); // All should be normalized to the same URL
        assert!(transformed.contains(&"https://example.com/api?a=1&b=2&c=3".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_with_merge_endpoint() {
        let mut transformer = UrlTransformer::new();
        transformer
            .with_normalize_url(true)
            .with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/api/?param2=value2&param1=value1".to_string(),
            "https://example.com/api?param3=value3".to_string(),
            "https://example.com/api/?param1=value1&param2=value2".to_string(),
        ];

        let transformed = transformer.transform(urls);
        // After normalization, first and third URLs should be identical
        // Then merge_endpoint should combine all parameters
        assert_eq!(transformed.len(), 1);
        let result_url = &transformed[0];
        assert!(result_url.starts_with("https://example.com/api?"));
        assert!(result_url.contains("param1=value1"));
        assert!(result_url.contains("param2=value2"));
        assert!(result_url.contains("param3=value3"));
    }

    #[test]
    fn test_url_transformer_normalize_preserves_bare_param_and_plus() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/a?foo".to_string(), // bare param, no '='
            "https://example.com/b?q=a+b".to_string(), // literal '+'
            "https://example.com/c?b=2&a=1".to_string(), // still gets sorted
        ];

        let out = transformer.transform(urls);
        // Bare param keeps no '='; '+' is not rewritten to '%20'; order sorted.
        assert!(
            out.contains(&"https://example.com/a?foo".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com/b?q=a+b".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com/c?a=1&b=2".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn test_url_transformer_normalize_invalid_urls() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api?a=1&b=2".to_string(),
            "not-a-valid-url".to_string(),
            "https://example.com/api?b=2&a=1".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 2); // Valid URLs normalized, invalid kept as-is
        assert!(transformed.contains(&"https://example.com/api?a=1&b=2".to_string()));
        assert!(transformed.contains(&"not-a-valid-url".to_string()));
    }

    #[test]
    fn test_url_transformer_new() {
        let transformer = UrlTransformer::new();

        // Transform empty list should return empty list
        let urls: Vec<String> = vec![];
        let transformed = transformer.transform(urls);
        assert!(transformed.is_empty());
    }

    #[test]
    fn test_url_transformer_no_options() {
        let transformer = UrlTransformer::new();

        let urls = vec![
            "https://example.com/path1".to_string(),
            "https://example.com/path2".to_string(),
        ];

        let transformed = transformer.transform(urls.clone());
        // Without any options, URLs should be returned as-is
        assert_eq!(transformed, urls);
    }

    #[test]
    fn test_url_transformer_show_only_path_root_path() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_path(true);

        let urls = vec![
            "https://example.com/".to_string(),
            "https://example.com/path".to_string(),
        ];

        let transformed = transformer.transform(urls);
        // Root path "/" should not be included
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"/path".to_string()));
    }

    #[test]
    fn test_url_transformer_show_only_param_no_params() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_param(true);

        let urls = vec![
            "https://example.com/path".to_string(),
            "https://example.com/api?id=123".to_string(),
        ];

        let transformed = transformer.transform(urls);
        // URL without params should not contribute to the result
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"id=123".to_string()));
    }

    #[test]
    fn test_url_transformer_merge_endpoints_single_url() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec!["https://example.com/api?param1=value1".to_string()];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"https://example.com/api?param1=value1".to_string()));
    }

    #[test]
    fn test_url_transformer_merge_endpoints_no_params() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/path".to_string(),
            "https://example.com/path".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"https://example.com/path".to_string()));
    }

    #[test]
    fn test_url_transformer_merge_endpoints_invalid_url() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "not-a-valid-url".to_string(),
            "another-invalid-url".to_string(),
        ];

        let transformed = transformer.transform(urls);
        // Invalid URLs should be kept as-is
        assert_eq!(transformed.len(), 2);
    }

    #[test]
    fn test_url_transformer_normalize_empty_query() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec!["https://example.com/path".to_string()];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"https://example.com/path".to_string()));
    }

    #[test]
    fn test_url_transformer_chaining() {
        let mut transformer = UrlTransformer::new();
        transformer
            .with_merge_endpoint(true)
            .with_show_only_host(false)
            .with_show_only_path(false)
            .with_show_only_param(false)
            .with_normalize_url(true);

        let urls = vec![
            "https://example.com/api?b=2&a=1".to_string(),
            "https://example.com/api?a=1&b=2".to_string(),
        ];

        let transformed = transformer.transform(urls);
        assert_eq!(transformed.len(), 1);
    }

    #[test]
    fn test_url_transformer_show_only_host_invalid_url() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_host(true);

        let urls = vec![
            "https://example.com/path".to_string(),
            "not-a-valid-url".to_string(),
        ];

        let transformed = transformer.transform(urls);
        // Invalid URL should be kept as-is
        assert!(transformed.contains(&"example.com".to_string()));
        assert!(transformed.contains(&"not-a-valid-url".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_completely_invalid_inputs() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "plain-text".to_string(),
            "://start-with-colon".to_string(),
            "".to_string(),
        ];

        let transformed = transformer.transform(urls);

        assert_eq!(transformed.len(), 3);
        assert!(transformed.contains(&"plain-text".to_string()));
        assert!(transformed.contains(&"://start-with-colon".to_string()));
        assert!(transformed.contains(&"".to_string()));
    }
}
