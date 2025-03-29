use std::collections::{HashMap, HashSet};
use url::Url;

pub struct UrlFilter {
    extensions: Vec<String>,
    exclude_extensions: Vec<String>,
    patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    min_length: Option<usize>,
    max_length: Option<usize>,
}

impl UrlFilter {
    pub fn new() -> Self {
        UrlFilter {
            extensions: Vec::new(),
            exclude_extensions: Vec::new(),
            patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            min_length: None,
            max_length: None,
        }
    }

    pub fn with_extensions(&mut self, exts: Vec<String>) -> &mut Self {
        self.extensions = exts;
        self
    }

    pub fn with_exclude_extensions(&mut self, exts: Vec<String>) -> &mut Self {
        self.exclude_extensions = exts;
        self
    }

    pub fn with_patterns(&mut self, patterns: Vec<String>) -> &mut Self {
        self.patterns = patterns;
        self
    }

    pub fn with_exclude_patterns(&mut self, patterns: Vec<String>) -> &mut Self {
        self.exclude_patterns = patterns;
        self
    }

    pub fn with_min_length(&mut self, min: Option<usize>) -> &mut Self {
        self.min_length = min;
        self
    }

    pub fn with_max_length(&mut self, max: Option<usize>) -> &mut Self {
        self.max_length = max;
        self
    }

    pub fn apply_filters(&self, urls: &HashSet<String>) -> Vec<String> {
        let mut filtered_urls: Vec<String> = urls.iter().cloned().collect();

        // Apply extension filter
        if !self.extensions.is_empty() {
            filtered_urls.retain(|url| {
                let path = url.split('?').next().unwrap_or(url);
                let ext = path.split('.').last().unwrap_or("");
                self.extensions.iter().any(|e| e == ext)
            });
        }

        // Apply pattern filter
        if !self.patterns.is_empty() {
            filtered_urls.retain(|url| self.patterns.iter().any(|pattern| url.contains(pattern)));
        }

        // Apply exclude extension filter
        if !self.exclude_extensions.is_empty() {
            filtered_urls.retain(|url| {
                let path = url.split('?').next().unwrap_or(url);
                let ext = path.split('.').last().unwrap_or("");
                !self.exclude_extensions.iter().any(|e| e == ext)
            });
        }

        // Apply exclude pattern filter
        if !self.exclude_patterns.is_empty() {
            filtered_urls.retain(|url| {
                !self
                    .exclude_patterns
                    .iter()
                    .any(|pattern| url.contains(pattern))
            });
        }

        // Apply minimum length filter
        if let Some(min_length) = self.min_length {
            filtered_urls.retain(|url| url.len() >= min_length);
        }

        // Apply maximum length filter
        if let Some(max_length) = self.max_length {
            filtered_urls.retain(|url| url.len() <= max_length);
        }

        // Sort for consistent output
        filtered_urls.sort();

        filtered_urls
    }
}

pub struct UrlTransformer {
    merge_endpoint: bool,
    show_only_host: bool,
    show_only_path: bool,
    show_only_param: bool,
}

impl UrlTransformer {
    pub fn new() -> Self {
        UrlTransformer {
            merge_endpoint: false,
            show_only_host: false,
            show_only_path: false,
            show_only_param: false,
        }
    }

    pub fn with_merge_endpoint(&mut self, merge: bool) -> &mut Self {
        self.merge_endpoint = merge;
        self
    }

    pub fn with_show_only_host(&mut self, show: bool) -> &mut Self {
        self.show_only_host = show;
        self
    }

    pub fn with_show_only_path(&mut self, show: bool) -> &mut Self {
        self.show_only_path = show;
        self
    }

    pub fn with_show_only_param(&mut self, show: bool) -> &mut Self {
        self.show_only_param = show;
        self
    }

    pub fn transform(&self, urls: Vec<String>) -> Vec<String> {
        let mut transformed_urls = urls;

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
                            .map(|(k, v)| format!("{}={}", k, v))
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
    fn test_url_filter_extensions() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string(), "php".to_string()]);

        let urls = HashSet::from([
            "https://example.com/script.js".to_string(),
            "https://example.com/page.php".to_string(),
            "https://example.com/page.html".to_string(),
        ]);

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(filtered.contains(&"https://example.com/page.php".to_string()));
        assert!(!filtered.contains(&"https://example.com/page.html".to_string()));
    }

    #[test]
    fn test_url_filter_exclude_extensions() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_extensions(vec!["html".to_string()]);

        let urls = HashSet::from([
            "https://example.com/script.js".to_string(),
            "https://example.com/page.php".to_string(),
            "https://example.com/page.html".to_string(),
        ]);

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(filtered.contains(&"https://example.com/page.php".to_string()));
        assert!(!filtered.contains(&"https://example.com/page.html".to_string()));
    }

    #[test]
    fn test_url_filter_patterns() {
        let mut filter = UrlFilter::new();
        filter.with_patterns(vec!["admin".to_string()]);

        let urls = HashSet::from([
            "https://example.com/admin/dashboard".to_string(),
            "https://example.com/admin/users".to_string(),
            "https://example.com/public/page".to_string(),
        ]);

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/admin/dashboard".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/users".to_string()));
        assert!(!filtered.contains(&"https://example.com/public/page".to_string()));
    }

    #[test]
    fn test_url_filter_exclude_patterns() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_patterns(vec!["public".to_string()]);

        let urls = HashSet::from([
            "https://example.com/admin/dashboard".to_string(),
            "https://example.com/admin/users".to_string(),
            "https://example.com/public/page".to_string(),
        ]);

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/admin/dashboard".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/users".to_string()));
        assert!(!filtered.contains(&"https://example.com/public/page".to_string()));
    }

    #[test]
    fn test_url_filter_min_length() {
        let mut filter = UrlFilter::new();
        filter.with_min_length(Some(30));

        let urls = HashSet::from([
            "https://example.com/short".to_string(),
            "https://example.com/very/long/path/to/resource".to_string(),
        ]);

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains(&"https://example.com/very/long/path/to/resource".to_string()));
    }

    #[test]
    fn test_url_filter_max_length() {
        let mut filter = UrlFilter::new();
        filter.with_max_length(Some(30));

        let urls = HashSet::from([
            "https://example.com/short".to_string(),
            "https://example.com/very/long/path/to/resource".to_string(),
        ]);

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains(&"https://example.com/short".to_string()));
    }

    #[test]
    fn test_url_filter_combined() {
        let mut filter = UrlFilter::new();
        filter
            .with_extensions(vec!["php".to_string()])
            .with_patterns(vec!["admin".to_string()])
            .with_min_length(Some(36));

        let urls = HashSet::from([
            "https://example.com/admin/dashboard.php".to_string(),
            "https://example.com/admin/user_login.php".to_string(),
            "https://example.com/admin/short.php".to_string(), // too short
            "https://example.com/public/page.php".to_string(), // no "admin"
            "https://example.com/admin/page.html".to_string(), // not php
        ]);

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/admin/dashboard.php".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/user_login.php".to_string()));
    }

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
}
