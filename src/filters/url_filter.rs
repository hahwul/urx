use std::collections::HashSet;
use std::path::Path;
use url::Url;

use super::preset::FilterPreset;

/// URL Filter for filtering URLs based on extensions, patterns, length, etc.
pub struct UrlFilter {
    extensions: Vec<String>,
    exclude_extensions: Vec<String>,
    patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    min_length: Option<usize>,
    max_length: Option<usize>,
}

impl UrlFilter {
    /// Create a new URL filter
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

    /// Apply filter presets to this URL filter
    pub fn apply_presets(&mut self, presets: &[String]) -> &mut Self {
        for preset_str in presets {
            if let Some(preset) = FilterPreset::from_str(preset_str) {
                // Merge preset extensions/patterns with existing ones
                self.extensions.extend(preset.get_extensions());
                self.exclude_extensions
                    .extend(preset.get_exclude_extensions());
                self.patterns.extend(preset.get_patterns());
                self.exclude_patterns.extend(preset.get_exclude_patterns());
            }
        }
        self
    }

    /// Set extensions to include
    pub fn with_extensions(&mut self, extensions: Vec<String>) -> &mut Self {
        // Merge with existing extensions instead of replacing
        self.extensions.extend(extensions);
        self
    }

    /// Set extensions to exclude
    pub fn with_exclude_extensions(&mut self, exclude_extensions: Vec<String>) -> &mut Self {
        self.exclude_extensions.extend(exclude_extensions);
        self
    }

    /// Set patterns to include
    pub fn with_patterns(&mut self, patterns: Vec<String>) -> &mut Self {
        // Merge with existing patterns instead of replacing
        self.patterns.extend(patterns);
        self
    }

    /// Set patterns to exclude
    pub fn with_exclude_patterns(&mut self, exclude_patterns: Vec<String>) -> &mut Self {
        // Merge with existing exclude_patterns instead of replacing
        self.exclude_patterns.extend(exclude_patterns);
        self
    }

    /// Set minimum URL length
    pub fn with_min_length(&mut self, min_length: Option<usize>) -> &mut Self {
        self.min_length = min_length;
        self
    }

    /// Set maximum URL length
    pub fn with_max_length(&mut self, max_length: Option<usize>) -> &mut Self {
        self.max_length = max_length;
        self
    }

    /// Apply filters to a set of URLs
    pub fn apply_filters(&self, urls: &HashSet<String>) -> Vec<String> {
        let mut result = Vec::new();

        for url in urls {
            // Skip if URL doesn't match the length criteria
            if let Some(min) = self.min_length {
                if url.len() < min {
                    continue;
                }
            }

            if let Some(max) = self.max_length {
                if url.len() > max {
                    continue;
                }
            }

            // Parse the URL to extract the path for better extension handling
            let extension = match Url::parse(url) {
                Ok(parsed_url) => {
                    // Get the path from the URL
                    if let Some(path) = parsed_url
                        .path_segments()
                        .and_then(|mut segments| segments.next_back())
                    {
                        // Manually handle complex extensions like "tar.gz"
                        let filename = Path::new(path).file_name().and_then(|s| s.to_str());
                        if let Some(name) = filename {
                            let parts: Vec<&str> = name.split('.').collect();
                            if parts.len() > 1 {
                                Some(parts[1..].join(".").to_lowercase())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                Err(_) => {
                    // Fallback for invalid URLs - try to extract extension from the whole string
                    let parts: Vec<&str> = url.split('/').collect();
                    if let Some(last) = parts.last() {
                        let filename_parts: Vec<&str> = last.split('.').collect();
                        if filename_parts.len() > 1 {
                            Some(
                                filename_parts[1..]
                                    .join(".")
                                    .split('?')
                                    .next()
                                    .unwrap_or("")
                                    .to_lowercase(),
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            };

            // Check exclusions first
            if !self.exclude_extensions.is_empty() {
                if let Some(ext) = &extension {
                    let lowercase_ext = ext.to_lowercase();
                    if self
                        .exclude_extensions
                        .iter()
                        .any(|excluded_ext| excluded_ext.to_lowercase() == lowercase_ext)
                    {
                        continue;
                    }
                }
            }

            if !self.exclude_patterns.is_empty() {
                let url_lower = url.to_lowercase();
                if self
                    .exclude_patterns
                    .iter()
                    .any(|pattern| url_lower.contains(&pattern.to_lowercase()))
                {
                    continue;
                }
            }

            // Then check inclusions
            let mut include = true;

            if !self.extensions.is_empty() {
                if let Some(ext) = &extension {
                    let lowercase_ext = ext.to_lowercase();
                    include = self
                        .extensions
                        .iter()
                        .any(|included_ext| included_ext.to_lowercase() == lowercase_ext);
                } else {
                    include = false; // No extension found but extensions filter is set
                }
            }

            if !self.patterns.is_empty() {
                let url_lower = url.to_lowercase();
                let pattern_match = self
                    .patterns
                    .iter()
                    .any(|pattern| url_lower.contains(&pattern.to_lowercase()));
                if !pattern_match {
                    include = false;
                }
            }

            if include {
                result.push(url.clone());
            }
        }

        // Sort the results for consistent output
        result.sort();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn create_test_urls() -> HashSet<String> {
        let urls = vec![
            "https://example.com/index.html",
            "https://example.com/script.js",
            "https://example.com/style.css",
            "https://example.com/image.png",
            "https://example.com/document.pdf",
            "https://example.com/font.woff2",
            "https://example.com/video.mp4",
            "https://example.com/admin/login.php",
            "https://example.com/api/v1/users?id=123",
            "https://example.com/very/long/path/to/resource/file.html",
            "https://example.com/.git/config",
        ];
        urls.into_iter().map(String::from).collect()
    }

    #[test]
    fn test_new_filter() {
        let filter = UrlFilter::new();
        assert!(filter.extensions.is_empty());
        assert!(filter.exclude_extensions.is_empty());
        assert!(filter.patterns.is_empty());
        assert!(filter.exclude_patterns.is_empty());
        assert_eq!(filter.min_length, None);
        assert_eq!(filter.max_length, None);
    }

    #[test]
    fn test_with_extensions() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string(), "php".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
    }

    #[test]
    fn test_with_exclude_extensions() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_extensions(vec![
            "js".to_string(),
            "css".to_string(),
            "png".to_string(),
        ]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 8);
        assert!(!filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(!filtered.contains(&"https://example.com/style.css".to_string()));
        assert!(!filtered.contains(&"https://example.com/image.png".to_string()));
    }

    #[test]
    fn test_with_patterns() {
        let mut filter = UrlFilter::new();
        filter.with_patterns(vec!["admin".to_string(), "api".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
        assert!(filtered.contains(&"https://example.com/api/v1/users?id=123".to_string()));
    }

    #[test]
    fn test_with_exclude_patterns() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_patterns(vec!["admin".to_string(), ".git".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 9);
        assert!(!filtered.contains(&"https://example.com/admin/login.php".to_string()));
        assert!(!filtered.contains(&"https://example.com/.git/config".to_string()));
    }

    #[test]
    fn test_with_length_filters() {
        let mut filter = UrlFilter::new();
        filter.with_min_length(Some(40));
        filter.with_max_length(Some(60));

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        for url in &filtered {
            assert!(url.len() >= 40);
            assert!(url.len() <= 60);
        }
    }

    #[test]
    fn test_apply_presets() {
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["no-images".to_string(), "only-js".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(!filtered.contains(&"https://example.com/image.png".to_string()));
    }

    #[test]
    fn test_with_complex_extensions() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["tar.gz".to_string(), "min.js".to_string()]);

        let mut urls = create_test_urls();
        urls.insert("https://example.com/archive.tar.gz".to_string());
        urls.insert("https://example.com/script.min.js".to_string());
        urls.insert("https://example.com/not-min.js".to_string());

        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/archive.tar.gz".to_string()));
        assert!(filtered.contains(&"https://example.com/script.min.js".to_string()));
    }

    #[test]
    fn test_case_insensitive_filtering() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["JPG".to_string(), "PnG".to_string()]);
        filter.with_patterns(vec!["AdMiN".to_string()]);

        let mut urls = create_test_urls();
        urls.insert("https://example.com/image.jpg".to_string());
        urls.insert("https://example.com/photo.png".to_string());
        urls.insert("https://example.com/admin/dashboard".to_string());
        urls.insert("https://example.com/ADMIN/login.php".to_string());

        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 3);
        assert!(filtered.contains(&"https://example.com/image.jpg".to_string()));
        assert!(filtered.contains(&"https://example.com/photo.png".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/dashboard".to_string()));
    }

    #[test]
    fn test_combined_filters() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["html".to_string()]);
        filter.with_exclude_patterns(vec!["admin".to_string()]);

        let mut urls = create_test_urls();
        urls.insert("https://example.com/admin/index.html".to_string());

        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/index.html".to_string()));
        assert!(
            filtered.contains(&"https://example.com/very/long/path/to/resource/file.html".to_string())
        );
    }

    #[test]
    fn test_fallback_url_parsing() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["aspx".to_string()]);

        let mut urls = HashSet::new();
        urls.insert("mailto:user@example.com".to_string());
        urls.insert("ftp://example.com/file.txt".to_string());
        urls.insert("/path/to/page.aspx?id=1".to_string());

        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains(&"/path/to/page.aspx?id=1".to_string()));
    }

    #[test]
    fn test_preset_merging() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_extensions(vec!["log".to_string()]);
        filter.apply_presets(&["no-images".to_string()]);

        let mut urls = create_test_urls();
        urls.insert("https://example.com/error.log".to_string());

        let filtered = filter.apply_filters(&urls);

        assert!(!filtered.contains(&"https://example.com/image.png".to_string()));
        assert!(!filtered.contains(&"https://example.com/error.log".to_string()));
    }
}
