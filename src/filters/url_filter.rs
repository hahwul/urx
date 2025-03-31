use std::collections::HashSet;

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
                self.exclude_extensions.extend(preset.get_exclude_extensions());
                self.patterns.extend(preset.get_patterns());
                self.exclude_patterns.extend(preset.get_exclude_patterns());
            }
        }
        self
    }

    /// Set extensions to include
    pub fn with_extensions(&mut self, extensions: Vec<String>) -> &mut Self {
        self.extensions = extensions;
        self
    }

    /// Set extensions to exclude
    pub fn with_exclude_extensions(&mut self, exclude_extensions: Vec<String>) -> &mut Self {
        self.exclude_extensions = exclude_extensions;
        self
    }

    /// Set patterns to include
    pub fn with_patterns(&mut self, patterns: Vec<String>) -> &mut Self {
        self.patterns = patterns;
        self
    }

    /// Set patterns to exclude
    pub fn with_exclude_patterns(&mut self, exclude_patterns: Vec<String>) -> &mut Self {
        self.exclude_patterns = exclude_patterns;
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

            // Check exclusions first
            if !self.exclude_extensions.is_empty() {
                let url_lower = url.to_lowercase();
                if self.exclude_extensions.iter().any(|ext| {
                    url_lower.ends_with(&format!(".{}", ext.to_lowercase()))
                        || url_lower.contains(&format!(".{}?", ext.to_lowercase()))
                }) {
                    continue;
                }
            }

            if !self.exclude_patterns.is_empty() {
                let url_lower = url.to_lowercase();
                if self.exclude_patterns.iter().any(|pattern| {
                    url_lower.contains(&pattern.to_lowercase())
                }) {
                    continue;
                }
            }

            // Then check inclusions
            let mut include = true;

            if !self.extensions.is_empty() {
                let url_lower = url.to_lowercase();
                include = self.extensions.iter().any(|ext| {
                    url_lower.ends_with(&format!(".{}", ext.to_lowercase()))
                        || url_lower.contains(&format!(".{}?", ext.to_lowercase()))
                });
            }

            if include && !self.patterns.is_empty() {
                let url_lower = url.to_lowercase();
                include = self.patterns.iter().any(|pattern| {
                    url_lower.contains(&pattern.to_lowercase())
                });
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