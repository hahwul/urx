/// Standard filter presets for common URL filtering scenarios
pub enum FilterPreset {
    /// Excludes common web resource files (js, css, ico, ttf, etc.)
    NoResources,
    /// Excludes image files (png, jpg, jpeg, gif, svg, etc.)
    NoImages,
    /// Only includes JavaScript files
    OnlyJs,
    /// Only includes style files (css, scss, sass, etc.)
    OnlyStyle,
    /// Excludes font files (ttf, otf, woff, etc.)
    NoFonts,
    /// Excludes document files (pdf, doc, docx, etc.)
    NoDocuments,
    /// Excludes video files (mp4, mkv, avi, etc.)
    NoVideos,
    /// Only includes font files
    OnlyFonts,
    /// Only includes document files
    OnlyDocuments,
    /// Only includes video files
    OnlyVideos,
    /// Only includes image files
    OnlyImages,
}

/// Common file extensions for various resource types
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "tiff", "tif", "heic", "heif", "raw",
    "psd", "ai", "eps", "avif", "jfif", "jp2", "jpx", "apng", "cr2", "nef", "orf", "arw", "dng",
    "webm", "pgm", "pbm", "ppm", "pnm", "exr", "xcf", "pcx", "tga", "emf", "wmf", "jxr", "hdp",
    "wdp", "cur", "dcm", "wbmp", "j2k", "art", "jng", "3fr", "ari", "srf", "sr2", "bay", "crw",
    "kdc", "erf", "mrw", "rw2", "pef", "dicom", "djvu", "fpx", "hdr", "mng", "ora", "pic", "rgb",
    "rgba", "webm", "webp", "xbm", "xpm", "dpx", "fits", "flif", "img", "mpo", "psb",
];

const FONT_EXTENSIONS: &[&str] = &[
    "ttf", "otf", "woff", "woff2", "eot", "fon", "fnt", "svg", "ttc", "dfont", "pfa", "pfb",
];

const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "csv", "rtf", "odt", "ods", "odp",
    "epub", "mobi", "azw3", "fb2", "djvu", "epub3", "xps",
];

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "flac", "aac", "ogg", "wma", "m4a", "opus", "aiff", "alac", "dsd", "dff", "dsf",
    "pcm", "aifc", "au", "snd", "caf", "ra", "ram",
];

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "mpeg", "mpg", "3gp", "3g2", "m4v", "f4v",
    "f4p", "f4a", "f4b", "asf", "rmvb", "rm", "dat", "ts", "vob",
];

const JS_EXTENSIONS: &[&str] = &[
    "js", "ts", "jsx", "tsx", "mjs", "cjs", "vue", "json", "coffee", "es6", "es", "svelte",
    "astro", "njk", "map",
];

const STYLE_EXTENSIONS: &[&str] = &[
    "css", "scss", "sass", "less", "stylus", "postcss", "pcss", "cssm", "cssx", "cssb",
];

impl FilterPreset {
    /// Parse a preset string into a FilterPreset enum
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "no-resource" | "no-resources" => Some(FilterPreset::NoResources),
            "no-image" | "no-images" => Some(FilterPreset::NoImages),
            "no-font" | "no-fonts" => Some(FilterPreset::NoFonts),
            "no-document" | "no-documents" => Some(FilterPreset::NoDocuments),
            "no-video" | "no-videos" => Some(FilterPreset::NoVideos),
            "only-js" => Some(FilterPreset::OnlyJs),
            "only-style" | "only-styles" => Some(FilterPreset::OnlyStyle),
            "only-fonts" => Some(FilterPreset::OnlyFonts),
            "only-documents" => Some(FilterPreset::OnlyDocuments),
            "only-videos" => Some(FilterPreset::OnlyVideos),
            "only-images" => Some(FilterPreset::OnlyImages),
            _ => None,
        }
    }

    /// Get excluded extensions for this preset
    pub fn get_exclude_extensions(&self) -> Vec<String> {
        match self {
            FilterPreset::NoResources => {
                let mut extensions = Vec::new();
                extensions.extend(IMAGE_EXTENSIONS.iter().map(|&s| s.to_string()));
                extensions.extend(FONT_EXTENSIONS.iter().map(|&s| s.to_string()));
                extensions.extend(DOCUMENT_EXTENSIONS.iter().map(|&s| s.to_string()));
                extensions.extend(AUDIO_EXTENSIONS.iter().map(|&s| s.to_string()));
                extensions.extend(VIDEO_EXTENSIONS.iter().map(|&s| s.to_string()));
                extensions.extend(JS_EXTENSIONS.iter().map(|&s| s.to_string()));
                extensions.extend(STYLE_EXTENSIONS.iter().map(|&s| s.to_string()));
                extensions
            }
            FilterPreset::NoImages => IMAGE_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::NoFonts => FONT_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::NoDocuments => {
                DOCUMENT_EXTENSIONS.iter().map(|&s| s.to_string()).collect()
            }
            FilterPreset::NoVideos => VIDEO_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyJs | FilterPreset::OnlyStyle => vec![],
            FilterPreset::OnlyFonts => FONT_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyDocuments => {
                DOCUMENT_EXTENSIONS.iter().map(|&s| s.to_string()).collect()
            }
            FilterPreset::OnlyVideos => VIDEO_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyImages => IMAGE_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
        }
    }

    /// Get included extensions for this preset
    pub fn get_extensions(&self) -> Vec<String> {
        match self {
            FilterPreset::OnlyJs => JS_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyStyle => STYLE_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_preset_from_str() {
        // Test valid preset values
        assert!(matches!(
            FilterPreset::from_str("no-resources"),
            Some(FilterPreset::NoResources)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-resource"),
            Some(FilterPreset::NoResources)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-images"),
            Some(FilterPreset::NoImages)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-image"),
            Some(FilterPreset::NoImages)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-fonts"),
            Some(FilterPreset::NoFonts)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-font"),
            Some(FilterPreset::NoFonts)
        ));
        assert!(matches!(
            FilterPreset::from_str("only-js"),
            Some(FilterPreset::OnlyJs)
        ));
        assert!(matches!(
            FilterPreset::from_str("only-style"),
            Some(FilterPreset::OnlyStyle)
        ));
        assert!(matches!(
            FilterPreset::from_str("only-styles"),
            Some(FilterPreset::OnlyStyle)
        ));

        // Test case insensitivity
        assert!(matches!(
            FilterPreset::from_str("No-Resources"),
            Some(FilterPreset::NoResources)
        ));
        assert!(matches!(
            FilterPreset::from_str("ONLY-JS"),
            Some(FilterPreset::OnlyJs)
        ));

        // Test invalid preset values
        assert!(FilterPreset::from_str("invalid-preset").is_none());
        assert!(FilterPreset::from_str("").is_none());
    }

    #[test]
    fn test_no_resources_preset() {
        let preset = FilterPreset::NoResources;
        let extensions = preset.get_extensions();
        let exclude_extensions = preset.get_exclude_extensions();

        // NoResources should not include any extensions
        assert!(extensions.is_empty());

        // NoResources should exclude various resource types
        assert!(exclude_extensions.contains(&"js".to_string()));
        assert!(exclude_extensions.contains(&"css".to_string()));
        assert!(exclude_extensions.contains(&"png".to_string()));
        assert!(exclude_extensions.contains(&"pdf".to_string()));
        assert!(exclude_extensions.contains(&"woff".to_string()));
        assert!(exclude_extensions.contains(&"mp4".to_string()));
    }

    #[test]
    fn test_no_images_preset() {
        let preset = FilterPreset::NoImages;
        let exclude_extensions = preset.get_exclude_extensions();

        // Should exclude all image extensions
        assert!(exclude_extensions.contains(&"png".to_string()));
        assert!(exclude_extensions.contains(&"jpg".to_string()));
        assert!(exclude_extensions.contains(&"jpeg".to_string()));
        assert!(exclude_extensions.contains(&"gif".to_string()));
        assert!(exclude_extensions.contains(&"svg".to_string()));
        assert!(exclude_extensions.contains(&"webp".to_string()));

        // Should not exclude non-image extensions
        let js_found = exclude_extensions.iter().any(|ext| ext == "js");
        let css_found = exclude_extensions.iter().any(|ext| ext == "css");
        assert!(!js_found);
        assert!(!css_found);
    }

    #[test]
    fn test_only_js_preset() {
        let preset = FilterPreset::OnlyJs;
        let extensions = preset.get_extensions();
        let exclude_extensions = preset.get_exclude_extensions();

        // Should include JS extensions
        assert!(extensions.contains(&"js".to_string()));
        assert!(extensions.contains(&"jsx".to_string()));
        assert!(extensions.contains(&"ts".to_string()));
        assert!(extensions.contains(&"tsx".to_string()));

        // Should not exclude any extensions
        assert!(exclude_extensions.is_empty());
    }

    #[test]
    fn test_only_style_preset() {
        let preset = FilterPreset::OnlyStyle;
        let extensions = preset.get_extensions();

        // Should include CSS extensions
        assert!(extensions.contains(&"css".to_string()));
        assert!(extensions.contains(&"scss".to_string()));
        assert!(extensions.contains(&"sass".to_string()));
        assert!(extensions.contains(&"less".to_string()));
    }

    #[test]
    fn test_filter_preset_patterns() {
        // Test that patterns are empty by default
        for preset in [
            FilterPreset::NoResources,
            FilterPreset::NoImages,
            FilterPreset::OnlyJs,
            FilterPreset::OnlyStyle,
        ] {
            assert!(preset.get_patterns().is_empty());
            assert!(preset.get_exclude_patterns().is_empty());
        }
    }
}
