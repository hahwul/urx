/// Standard filter presets for common URL filtering scenarios
pub enum FilterPreset {
    /// Excludes common web resource files (js, css, ico, ttf, etc.)
    NoResource,
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
            "no-resource" | "no-resources" => Some(FilterPreset::NoResource),
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
            FilterPreset::NoResource => {
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
