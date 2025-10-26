use std::collections::HashMap;
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

    fn normalize_urls(&self, urls: Vec<String>) -> Vec<String> {
        let mut normalized_urls = Vec::new();

        for url_str in urls {
            if let Ok(mut url) = Url::parse(&url_str) {
                // Normalize the path - remove trailing slash if it's not just "/"
                let path = url.path().to_string();
                if path.len() > 1 && path.ends_with('/') {
                    let normalized_path = &path[..path.len() - 1];
                    url.set_path(normalized_path);
                }

                // Normalize query parameters - sort them alphabetically
                if url.query().is_some() {
                    let mut params: Vec<(String, String)> = url
                        .query_pairs()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();

                    // Sort parameters by key
                    params.sort_by(|a, b| a.0.cmp(&b.0));

                    // Clear existing query and set sorted query
                    url.set_query(None);
                    if !params.is_empty() {
                        let query_string = params
                            .into_iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join("&");
                        url.set_query(Some(&query_string));
                    }
                }

                normalized_urls.push(url.to_string());
            } else {
                // If URL can't be parsed, keep it as is
                normalized_urls.push(url_str);
            }
        }

        // Remove duplicates that might have been created during normalization
        normalized_urls.sort();
        normalized_urls.dedup();

        normalized_urls
    }

    fn merge_endpoints(&self, urls: Vec<String>) -> Vec<String> {
        let mut path_groups: HashMap<String, Vec<String>> = HashMap::new();

        for url_str in urls {
            if let Ok(url) = Url::parse(&url_str) {
                // Create a key using host and path
                let key = format!("{}{}", url.host_str().unwrap_or(""), url.path());

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
                    let mut all_params = Vec::new();

                    // Collect parameters from all URLs
                    for url_str in &group_urls {
                        if let Ok(url) = Url::parse(url_str) {
                            for (key, value) in url.query_pairs() {
                                if !all_params.iter().any(|(k, v)| k == &key && v == &value) {
                                    all_params.push((key.to_string(), value.to_string()));
                                }
                            }
                        }
                    }

                    // Set merged parameters
                    if !all_params.is_empty() {
                        let query_string = all_params
                            .into_iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join("&");

                        // Clear existing query and set merged query
                        merged_url.set_query(None);
                        if !query_string.is_empty() {
                            merged_url.set_query(Some(&query_string));
                        }
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

    fn extract_url_parts(&self, urls: Vec<String>) -> Vec<String> {
        let mut extracted_parts = Vec::new();

        for url_str in urls {
            if let Ok(url) = Url::parse(&url_str) {
                if self.show_only_host {
                    // Extract and add host
                    if let Some(host) = url.host_str() {
                        extracted_parts.push(host.to_string());
                    }
                } else if self.show_only_path {
                    // Extract and add path
                    if url.path() != "/" {
                        extracted_parts.push(url.path().to_string());
                    }
                } else if self.show_only_param {
                    // Extract and add parameters
                    if let Some(query) = url.query() {
                        extracted_parts.push(query.to_string());
                    }
                }
            } else {
                // If URL can't be parsed, keep it as is
                extracted_parts.push(url_str);
            }
        }

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
}
