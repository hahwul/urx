/// Standard filter presets for common URL filtering scenarios
pub enum FilterPreset {
    /// Excludes common web resource files (js, css, ico, ttf, etc.)
    NoResource,
    /// Excludes image files (png, jpg, jpeg, gif, svg, etc.)
    NoImages,
    /// Only includes JavaScript files
    OnlyJs,
}

impl FilterPreset {
    /// Parse a preset string into a FilterPreset enum
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "no-resource" | "no-resources" => Some(FilterPreset::NoResource),
            "no-images" => Some(FilterPreset::NoImages),
            "only-js" => Some(FilterPreset::OnlyJs),
            _ => None,
        }
    }

    /// Get excluded extensions for this preset
    pub fn get_exclude_extensions(&self) -> Vec<String> {
        match self {
            FilterPreset::NoResource => vec![
                "js".to_string(),
                "css".to_string(),
                "ico".to_string(),
                "ttf".to_string(),
                "woff".to_string(),
                "woff2".to_string(),
                "eot".to_string(),
            ],
            FilterPreset::NoImages => vec![
                "png".to_string(),
                "jpg".to_string(),
                "jpeg".to_string(),
                "gif".to_string(),
                "svg".to_string(),
                "webp".to_string(),
                "bmp".to_string(),
                "ico".to_string(),
            ],
            FilterPreset::OnlyJs => vec![],
        }
    }

    /// Get included extensions for this preset
    pub fn get_extensions(&self) -> Vec<String> {
        match self {
            FilterPreset::OnlyJs => vec!["js".to_string()],
            _ => vec![],
        }
    }

    /// Get excluded patterns for this preset
    pub fn get_exclude_patterns(&self) -> Vec<String> {
        vec![]
    }

    /// Get included patterns for this preset
    pub fn get_patterns(&self) -> Vec<String> {
        vec![]
    }
}
