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
    /// Excludes audio files (mp3, wav, flac, etc.)
    NoAudio,
    /// Only includes font files
    OnlyFonts,
    /// Only includes document files
    OnlyDocuments,
    /// Only includes video files
    OnlyVideos,
    /// Only includes audio files
    OnlyAudio,
    /// Only includes image files
    OnlyImages,
}

/// Common file extensions for various resource types
///
/// `webm` is deliberately absent: it is a video/audio container with no image
/// variant (the image format is `webp`, listed below). Having it here made
/// `--preset no-images` silently delete video URLs and `--preset only-images`
/// return them as images. It lives in [`VIDEO_EXTENSIONS`], where it belongs.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "tiff", "tif", "heic", "heif", "raw",
    "psd", "ai", "eps", "avif", "jfif", "jp2", "jpx", "apng", "cr2", "nef", "orf", "arw", "dng",
    "pgm", "pbm", "ppm", "pnm", "exr", "xcf", "pcx", "tga", "emf", "wmf", "jxr", "hdp", "wdp",
    "cur", "dcm", "wbmp", "j2k", "art", "jng", "3fr", "ari", "srf", "sr2", "bay", "crw", "kdc",
    "erf", "mrw", "rw2", "pef", "dicom", "djvu", "fpx", "hdr", "mng", "ora", "pic", "rgb", "rgba",
    "xbm", "xpm", "dpx", "fits", "flif", "img", "mpo", "psb",
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

/// Canonical preset name for each variant, in the order `--help` should list
/// them. Used to validate `--preset` and to name the alternatives in the error.
pub const PRESET_IDS: [&str; 13] = [
    "no-resources",
    "no-images",
    "no-fonts",
    "no-documents",
    "no-videos",
    "no-audio",
    "only-js",
    "only-style",
    "only-fonts",
    "only-documents",
    "only-videos",
    "only-audio",
    "only-images",
];

/// Reject `--preset` values that name nothing.
///
/// An unrecognised preset used to be dropped in silence (see
/// [`crate::filters::UrlFilter::apply_presets`]), so a typo like `only-image`
/// or `onlyjs` produced an unfiltered run that looked like the filter had simply
/// matched everything. Unknown provider ids already fail loudly; presets now do
/// the same.
pub fn validate_presets(presets: &[String]) -> anyhow::Result<()> {
    let unknown: Vec<&str> = presets
        .iter()
        .filter(|p| FilterPreset::from_str(p).is_none())
        .map(String::as_str)
        .collect();

    if unknown.is_empty() {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "Unknown preset(s) in --preset: {}. Allowed values: {}",
        unknown.join(", "),
        PRESET_IDS.join(", ")
    ))
}

impl FilterPreset {
    /// Parse a preset string into a FilterPreset enum
    ///
    /// Both singular and plural spellings are accepted for every preset, so
    /// `only-image` and `only-images` mean the same thing — the `no-*` family
    /// already worked that way and the `only-*` family did not.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "no-resource" | "no-resources" => Some(FilterPreset::NoResources),
            "no-image" | "no-images" => Some(FilterPreset::NoImages),
            "no-font" | "no-fonts" => Some(FilterPreset::NoFonts),
            "no-document" | "no-documents" => Some(FilterPreset::NoDocuments),
            "no-video" | "no-videos" => Some(FilterPreset::NoVideos),
            "no-audio" | "no-audios" => Some(FilterPreset::NoAudio),
            "only-js" => Some(FilterPreset::OnlyJs),
            "only-style" | "only-styles" => Some(FilterPreset::OnlyStyle),
            "only-font" | "only-fonts" => Some(FilterPreset::OnlyFonts),
            "only-document" | "only-documents" => Some(FilterPreset::OnlyDocuments),
            "only-video" | "only-videos" => Some(FilterPreset::OnlyVideos),
            "only-audio" | "only-audios" => Some(FilterPreset::OnlyAudio),
            "only-image" | "only-images" => Some(FilterPreset::OnlyImages),
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
            FilterPreset::NoAudio => AUDIO_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            // An `only-*` preset narrows the result set with an *inclusion* list
            // (see `get_extensions`); it excludes nothing.
            FilterPreset::OnlyJs
            | FilterPreset::OnlyStyle
            | FilterPreset::OnlyFonts
            | FilterPreset::OnlyDocuments
            | FilterPreset::OnlyVideos
            | FilterPreset::OnlyAudio
            | FilterPreset::OnlyImages => vec![],
        }
    }

    /// Get included extensions for this preset
    ///
    /// Every `only-*` preset belongs here, not in
    /// [`FilterPreset::get_exclude_extensions`]. Returning e.g. the image
    /// extensions as *exclusions* made `--preset only-images` drop every image
    /// and keep everything else — the exact inverse of its name.
    pub fn get_extensions(&self) -> Vec<String> {
        match self {
            FilterPreset::OnlyJs => JS_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyStyle => STYLE_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyFonts => FONT_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyDocuments => {
                DOCUMENT_EXTENSIONS.iter().map(|&s| s.to_string()).collect()
            }
            FilterPreset::OnlyVideos => VIDEO_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyAudio => AUDIO_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyImages => IMAGE_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::NoResources
            | FilterPreset::NoImages
            | FilterPreset::NoFonts
            | FilterPreset::NoDocuments
            | FilterPreset::NoVideos
            | FilterPreset::NoAudio => vec![],
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
    fn test_webm_is_a_video_not_an_image() {
        // Regression: `webm` sat in IMAGE_EXTENSIONS next to `webp`, so
        // `--preset no-images` silently deleted WebM *video* URLs and
        // `--preset only-images` handed them back as images.
        let images = FilterPreset::NoImages.get_exclude_extensions();
        assert!(!images.contains(&"webm".to_string()), "{images:?}");
        assert!(images.contains(&"webp".to_string()));

        assert!(!FilterPreset::OnlyImages
            .get_extensions()
            .contains(&"webm".to_string()));

        // It is still classified as a video on both video presets...
        assert!(FilterPreset::NoVideos
            .get_exclude_extensions()
            .contains(&"webm".to_string()));
        assert!(FilterPreset::OnlyVideos
            .get_extensions()
            .contains(&"webm".to_string()));
        // ...and no-resources, which excludes every family, still covers it.
        assert!(FilterPreset::NoResources
            .get_exclude_extensions()
            .contains(&"webm".to_string()));
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
    fn test_every_only_preset_includes_rather_than_excludes() {
        // Regression: only-fonts/-documents/-videos/-audio/-images returned their
        // extensions from get_exclude_extensions() with an empty get_extensions(),
        // so each one dropped exactly the family it claimed to keep. only-js and
        // only-style were the only two wired up correctly.
        let cases = [
            (FilterPreset::OnlyJs, "js"),
            (FilterPreset::OnlyStyle, "css"),
            (FilterPreset::OnlyFonts, "woff2"),
            (FilterPreset::OnlyDocuments, "pdf"),
            (FilterPreset::OnlyVideos, "mp4"),
            (FilterPreset::OnlyAudio, "mp3"),
            (FilterPreset::OnlyImages, "png"),
        ];

        for (preset, sample) in cases {
            let included = preset.get_extensions();
            let excluded = preset.get_exclude_extensions();
            assert!(
                included.contains(&sample.to_string()),
                "{sample} must be an *inclusion* for this only-* preset"
            );
            assert!(
                excluded.is_empty(),
                "an only-* preset must exclude nothing, got {excluded:?}"
            );
        }
    }

    #[test]
    fn test_only_images_keeps_images_end_to_end() {
        // The inversion was only visible through UrlFilter, which feeds
        // get_extensions() into the include list and get_exclude_extensions()
        // into the exclude list — so assert the actual filtering decision.
        use super::super::UrlFilter;
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["only-images".to_string()]);

        assert!(filter.matches("https://example.com/logo.png"));
        assert!(filter.matches("https://example.com/hero.jpg"));
        assert!(!filter.matches("https://example.com/app.js"));
        assert!(!filter.matches("https://example.com/index.html"));
    }

    #[test]
    fn test_no_images_still_drops_images_end_to_end() {
        // The `no-*` family was always correct; pin it so the fix above can't
        // flip it by accident.
        use super::super::UrlFilter;
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["no-images".to_string()]);

        assert!(!filter.matches("https://example.com/logo.png"));
        assert!(filter.matches("https://example.com/app.js"));
    }

    #[test]
    fn test_singular_and_plural_only_aliases_agree() {
        // The no-* family already accepted both spellings; only-* did not, so
        // `--preset only-image` silently did nothing.
        for (singular, plural) in [
            ("only-font", "only-fonts"),
            ("only-document", "only-documents"),
            ("only-video", "only-videos"),
            ("only-image", "only-images"),
        ] {
            let a = FilterPreset::from_str(singular).expect(singular);
            let b = FilterPreset::from_str(plural).expect(plural);
            assert_eq!(a.get_extensions(), b.get_extensions(), "{singular}");
        }
    }

    #[test]
    fn test_validate_presets_rejects_unknown_names() {
        assert!(validate_presets(&[]).is_ok());
        assert!(validate_presets(&["only-js".to_string(), "no-images".to_string()]).is_ok());

        let err = validate_presets(&["only-js".to_string(), "onlyjs".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("onlyjs"), "{err}");
        // The error names the alternatives rather than leaving the user guessing.
        assert!(err.contains("only-images"), "{err}");
    }

    #[test]
    fn test_preset_ids_cover_every_variant() {
        // PRESET_IDS drives the error message, so a new preset that isn't listed
        // there would be accepted by from_str yet absent from the help text.
        for id in PRESET_IDS {
            assert!(
                FilterPreset::from_str(id).is_some(),
                "{id} is listed but not parseable"
            );
        }
    }

    #[test]
    fn test_image_extension_list_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        let dupes: Vec<&&str> = IMAGE_EXTENSIONS
            .iter()
            .filter(|e| !seen.insert(**e))
            .collect();
        assert!(dupes.is_empty(), "duplicate image extensions: {dupes:?}");
    }

    #[test]
    fn test_filter_preset_patterns() {
        // Test that patterns are empty by default
        for preset in [
            FilterPreset::NoResources,
            FilterPreset::NoImages,
            FilterPreset::NoAudio,
            FilterPreset::OnlyJs,
            FilterPreset::OnlyStyle,
        ] {
            assert!(preset.get_patterns().is_empty());
            assert!(preset.get_exclude_patterns().is_empty());
        }
    }

    #[test]
    fn test_no_fonts_preset() {
        let preset = FilterPreset::NoFonts;
        let exclude_extensions = preset.get_exclude_extensions();

        // Should exclude all font extensions
        assert!(exclude_extensions.contains(&"ttf".to_string()));
        assert!(exclude_extensions.contains(&"otf".to_string()));
        assert!(exclude_extensions.contains(&"woff".to_string()));
        assert!(exclude_extensions.contains(&"woff2".to_string()));
        assert!(exclude_extensions.contains(&"eot".to_string()));

        // Should not include any extensions
        assert!(preset.get_extensions().is_empty());
    }

    #[test]
    fn test_no_documents_preset() {
        let preset = FilterPreset::NoDocuments;
        let exclude_extensions = preset.get_exclude_extensions();

        // Should exclude all document extensions
        assert!(exclude_extensions.contains(&"pdf".to_string()));
        assert!(exclude_extensions.contains(&"doc".to_string()));
        assert!(exclude_extensions.contains(&"docx".to_string()));
        assert!(exclude_extensions.contains(&"xls".to_string()));
        assert!(exclude_extensions.contains(&"xlsx".to_string()));
        assert!(exclude_extensions.contains(&"ppt".to_string()));
        assert!(exclude_extensions.contains(&"pptx".to_string()));

        // Should not include any extensions
        assert!(preset.get_extensions().is_empty());
    }

    #[test]
    fn test_no_videos_preset() {
        let preset = FilterPreset::NoVideos;
        let exclude_extensions = preset.get_exclude_extensions();

        // Should exclude all video extensions
        assert!(exclude_extensions.contains(&"mp4".to_string()));
        assert!(exclude_extensions.contains(&"mkv".to_string()));
        assert!(exclude_extensions.contains(&"avi".to_string()));
        assert!(exclude_extensions.contains(&"mov".to_string()));
        assert!(exclude_extensions.contains(&"wmv".to_string()));
        assert!(exclude_extensions.contains(&"webm".to_string()));

        // Should not include any extensions
        assert!(preset.get_extensions().is_empty());
    }

    #[test]
    fn test_no_audio_preset() {
        let preset = FilterPreset::NoAudio;
        let exclude_extensions = preset.get_exclude_extensions();

        // Should exclude all audio extensions
        assert!(exclude_extensions.contains(&"mp3".to_string()));
        assert!(exclude_extensions.contains(&"wav".to_string()));
        assert!(exclude_extensions.contains(&"flac".to_string()));
        assert!(exclude_extensions.contains(&"aac".to_string()));
        assert!(exclude_extensions.contains(&"ogg".to_string()));
        assert!(exclude_extensions.contains(&"m4a".to_string()));

        // Should not include any extensions
        assert!(preset.get_extensions().is_empty());
    }

    #[test]
    fn test_only_fonts_preset() {
        let preset = FilterPreset::OnlyFonts;
        let extensions = preset.get_extensions();

        // `only-fonts` keeps font files, so they belong in the *inclusion*
        // list. Storing them as exclusions made the preset drop exactly
        // what it promised to keep.
        assert!(extensions.contains(&"ttf".to_string()));
        assert!(extensions.contains(&"otf".to_string()));
        assert!(extensions.contains(&"woff".to_string()));
        assert!(extensions.contains(&"woff2".to_string()));

        // ...and it excludes nothing.
        assert!(preset.get_exclude_extensions().is_empty());
    }

    #[test]
    fn test_only_documents_preset() {
        let preset = FilterPreset::OnlyDocuments;
        let extensions = preset.get_extensions();

        // `only-documents` keeps document files, so they belong in the *inclusion*
        // list. Storing them as exclusions made the preset drop exactly
        // what it promised to keep.
        assert!(extensions.contains(&"pdf".to_string()));
        assert!(extensions.contains(&"doc".to_string()));
        assert!(extensions.contains(&"docx".to_string()));

        // ...and it excludes nothing.
        assert!(preset.get_exclude_extensions().is_empty());
    }

    #[test]
    fn test_only_videos_preset() {
        let preset = FilterPreset::OnlyVideos;
        let extensions = preset.get_extensions();

        // `only-videos` keeps video files, so they belong in the *inclusion*
        // list. Storing them as exclusions made the preset drop exactly
        // what it promised to keep.
        assert!(extensions.contains(&"mp4".to_string()));
        assert!(extensions.contains(&"mkv".to_string()));
        assert!(extensions.contains(&"avi".to_string()));

        // ...and it excludes nothing.
        assert!(preset.get_exclude_extensions().is_empty());
    }

    #[test]
    fn test_only_audio_preset() {
        let preset = FilterPreset::OnlyAudio;
        let extensions = preset.get_extensions();

        // `only-audios` keeps audio files, so they belong in the *inclusion*
        // list. Storing them as exclusions made the preset drop exactly
        // what it promised to keep.
        assert!(extensions.contains(&"mp3".to_string()));
        assert!(extensions.contains(&"wav".to_string()));
        assert!(extensions.contains(&"flac".to_string()));
        assert!(extensions.contains(&"aac".to_string()));

        // ...and it excludes nothing.
        assert!(preset.get_exclude_extensions().is_empty());
    }

    #[test]
    fn test_only_images_preset() {
        let preset = FilterPreset::OnlyImages;
        let extensions = preset.get_extensions();

        // `only-images` keeps image files, so they belong in the *inclusion*
        // list. Storing them as exclusions made the preset drop exactly
        // what it promised to keep.
        assert!(extensions.contains(&"png".to_string()));
        assert!(extensions.contains(&"jpg".to_string()));
        assert!(extensions.contains(&"jpeg".to_string()));
        assert!(extensions.contains(&"gif".to_string()));
        assert!(extensions.contains(&"svg".to_string()));

        // ...and it excludes nothing.
        assert!(preset.get_exclude_extensions().is_empty());
    }

    #[test]
    fn test_filter_preset_from_str_no_documents() {
        assert!(matches!(
            FilterPreset::from_str("no-documents"),
            Some(FilterPreset::NoDocuments)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-document"),
            Some(FilterPreset::NoDocuments)
        ));
    }

    #[test]
    fn test_filter_preset_from_str_no_videos() {
        assert!(matches!(
            FilterPreset::from_str("no-videos"),
            Some(FilterPreset::NoVideos)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-video"),
            Some(FilterPreset::NoVideos)
        ));
    }

    #[test]
    fn test_filter_preset_from_str_no_audio() {
        assert!(matches!(
            FilterPreset::from_str("no-audio"),
            Some(FilterPreset::NoAudio)
        ));
        assert!(matches!(
            FilterPreset::from_str("no-audios"),
            Some(FilterPreset::NoAudio)
        ));
    }

    #[test]
    fn test_filter_preset_from_str_only_fonts() {
        assert!(matches!(
            FilterPreset::from_str("only-fonts"),
            Some(FilterPreset::OnlyFonts)
        ));
    }

    #[test]
    fn test_filter_preset_from_str_only_documents() {
        assert!(matches!(
            FilterPreset::from_str("only-documents"),
            Some(FilterPreset::OnlyDocuments)
        ));
    }

    #[test]
    fn test_filter_preset_from_str_only_videos() {
        assert!(matches!(
            FilterPreset::from_str("only-videos"),
            Some(FilterPreset::OnlyVideos)
        ));
    }

    #[test]
    fn test_filter_preset_from_str_only_audio() {
        assert!(matches!(
            FilterPreset::from_str("only-audio"),
            Some(FilterPreset::OnlyAudio)
        ));
        assert!(matches!(
            FilterPreset::from_str("only-audios"),
            Some(FilterPreset::OnlyAudio)
        ));
    }

    #[test]
    fn test_filter_preset_from_str_only_images() {
        assert!(matches!(
            FilterPreset::from_str("only-images"),
            Some(FilterPreset::OnlyImages)
        ));
    }
}
