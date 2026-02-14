use std::collections::HashSet;
use std::path::Path;
use url::Url;

use super::preset::FilterPreset;

/// URL Filter for filtering URLs based on extensions, patterns, length, etc.
#[derive(Default)]
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
                self.extensions
                    .extend(preset.get_extensions().into_iter().map(|s| s.to_lowercase()));
                self.exclude_extensions.extend(
                    preset
                        .get_exclude_extensions()
                        .into_iter()
                        .map(|s| s.to_lowercase()),
                );
                self.patterns
                    .extend(preset.get_patterns().into_iter().map(|s| s.to_lowercase()));
                self.exclude_patterns.extend(
                    preset
                        .get_exclude_patterns()
                        .into_iter()
                        .map(|s| s.to_lowercase()),
                );
            }
        }
        self
    }

    /// Set extensions to include
    pub fn with_extensions(&mut self, extensions: Vec<String>) -> &mut Self {
        // Merge with existing extensions instead of replacing
        self.extensions
            .extend(extensions.into_iter().map(|s| s.to_lowercase()));
        self
    }

    /// Set extensions to exclude
    pub fn with_exclude_extensions(&mut self, exclude_extensions: Vec<String>) -> &mut Self {
        self.exclude_extensions
            .extend(exclude_extensions.into_iter().map(|s| s.to_lowercase()));
        self
    }

    /// Set patterns to include
    pub fn with_patterns(&mut self, patterns: Vec<String>) -> &mut Self {
        // Merge with existing patterns instead of replacing
        self.patterns
            .extend(patterns.into_iter().map(|s| s.to_lowercase()));
        self
    }

    /// Set patterns to exclude
    pub fn with_exclude_patterns(&mut self, exclude_patterns: Vec<String>) -> &mut Self {
        // Merge with existing exclude_patterns instead of replacing
        self.exclude_patterns
            .extend(exclude_patterns.into_iter().map(|s| s.to_lowercase()));
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
                        // Extract extension from the last path segment
                        Path::new(path)
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|s| s.to_lowercase())
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
                                filename_parts
                                    .last()
                                    .unwrap()
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

            // Compute url_lower once per URL iteration if needed
            let mut url_lower = None;

            // Check exclusions first
            if !self.exclude_extensions.is_empty() {
                if let Some(ext) = &extension {
                    if self
                        .exclude_extensions
                        .iter()
                        .any(|excluded_ext| excluded_ext == ext)
                    {
                        continue;
                    }
                }
            }

            if !self.exclude_patterns.is_empty() {
                let url_lower_str = url_lower.get_or_insert_with(|| url.to_lowercase());
                if self
                    .exclude_patterns
                    .iter()
                    .any(|pattern| url_lower_str.contains(pattern))
                {
                    continue;
                }
            }

            // Then check inclusions
            let mut include = true;

            if !self.extensions.is_empty() {
                if let Some(ext) = &extension {
                    include = self.extensions.iter().any(|included_ext| included_ext == ext);
                } else {
                    include = false; // No extension found but extensions filter is set
                }
            }

            if include && !self.patterns.is_empty() {
                let url_lower_str = url_lower.get_or_insert_with(|| url.to_lowercase());
                include = self
                    .patterns
                    .iter()
                    .any(|pattern| url_lower_str.contains(pattern));
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
}
