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
    /// Only includes URLs that look like leaked secrets or VCS metadata
    OnlySecrets,
    /// Only includes URLs that look like backups or archived copies
    OnlyBackup,
    /// Only includes URLs that look like configuration files
    OnlyConfig,
    /// Only includes URLs that look like API surfaces
    OnlyApi,
}

/// One "does this URL look interesting?" rule that is *not* expressible as a
/// file extension.
///
/// The security presets need this because the things they hunt for are named
/// by their path, not by an extension `Path::extension()` can see: `/.env`
/// has no extension at all (it is a dotfile), `/.git/config` is a directory
/// marker, and a backup is often just an ordinary name with a `~` glued on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRule {
    /// Matches when the lower-cased URL contains this (lower-case) substring.
    Contains(String),
    /// Matches when the lower-cased URL *path* ends with this (lower-case)
    /// suffix. Anchored at the end of the path rather than the end of the
    /// whole URL so a query string (`/index.php~?v=1`) does not defeat it.
    PathEndsWith(String),
}

impl PathRule {
    fn contains(s: &str) -> Self {
        PathRule::Contains(s.to_string())
    }

    fn ends_with(s: &str) -> Self {
        PathRule::PathEndsWith(s.to_string())
    }
}

fn rules(contains: &[&str], ends_with: &[&str]) -> Vec<PathRule> {
    contains
        .iter()
        .map(|s| PathRule::contains(s))
        .chain(ends_with.iter().map(|s| PathRule::ends_with(s)))
        .collect()
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

/// Extensions that only ever name key material or a credential store.
///
/// Deliberately narrow: `only-secrets` is a hunting preset, and a false
/// positive here costs a manual review of every certificate on the target.
const SECRET_EXTENSIONS: &[&str] = &[
    "pem", "key", "p12", "pfx", "jks", "keystore", "kdbx", "ppk", "asc", "gpg", "pgp", "kdb",
    "der", "csr",
];

/// Path shapes that mean "a secret was published", none of which an extension
/// can express — `/.env` and `/.git/config` are the canonical examples.
fn secret_path_rules() -> Vec<PathRule> {
    rules(
        &[
            "/.env",
            "/.git/",
            "/.gitconfig",
            "/.git-credentials",
            "/.svn/",
            "/.hg/",
            "/.bzr/",
            "/.ds_store",
            "/.aws/",
            "/.ssh/",
            "/.netrc",
            "/.npmrc",
            "/.pypirc",
            "/.docker/config.json",
            "/.htpasswd",
            "id_rsa",
            "id_dsa",
            "id_ecdsa",
            "id_ed25519",
            "credentials",
            "secrets",
            "/.bash_history",
            "/.zsh_history",
            "/.kube/config",
        ],
        &["/.env", "/.git", "/.svn", "/secrets"],
    )
}

/// Extensions that name a copy of something rather than the thing itself.
const BACKUP_EXTENSIONS: &[&str] = &[
    "bak", "bkp", "bck", "old", "orig", "save", "backup", "swp", "swo", "swn", "tmp", "temp",
    "copy", "sql", "dump", "dmp", "tar", "tgz", "tbz", "tbz2", "txz", "gz", "bz2", "xz", "zip",
    "rar", "7z", "war", "sav",
];

/// Backup markers that live in the name rather than the extension.
///
/// The trailing `~` — the editor convention that produced `index.php~` — is a
/// *suffix* rule on purpose. As a substring it would also flag every
/// `/~user/` home directory, which is not a backup at all.
fn backup_path_rules() -> Vec<PathRule> {
    rules(
        &[
            ".bak",
            ".old",
            ".orig",
            ".save",
            ".backup",
            ".tar.gz",
            ".sql.gz",
            "/backup",
            "backup/",
            "backup.",
            "_backup",
            "-backup",
            "/.well-known/backup",
            "/old/",
            "_old.",
            "-old.",
            ".swp",
        ],
        &["~", ".bak", ".old", "/backup", "/backups", ".orig", ".save"],
    )
}

/// Extensions that name a configuration file.
const CONFIG_EXTENSIONS: &[&str] = &[
    "conf",
    "config",
    "cfg",
    "ini",
    "yaml",
    "yml",
    "toml",
    "properties",
    "env",
    "plist",
    "hcl",
    "tfvars",
    "nomad",
    "cnf",
    "rc",
];

/// Configuration files that are recognised by name, not by extension.
fn config_path_rules() -> Vec<PathRule> {
    rules(
        &[
            "web.config",
            "app.config",
            "/.htaccess",
            "/.editorconfig",
            "/.babelrc",
            "/.eslintrc",
            "/.prettierrc",
            "/.dockerignore",
            "/dockerfile",
            "docker-compose",
            "/.npmrc",
            "/settings.py",
            "/wp-config",
            "/appsettings",
            "/.well-known/security.txt",
        ],
        &["/dockerfile", "/makefile", "/procfile", "/config", "/.env"],
    )
}

/// Extensions that describe a machine interface.
const API_EXTENSIONS: &[&str] = &["wsdl", "wadl", "asmx", "svc"];

/// The API surface is almost entirely a path shape, so this is where the
/// preset does its real work.
fn api_path_rules() -> Vec<PathRule> {
    rules(
        &[
            "/api/",
            "/api.",
            "/api?",
            "/apis/",
            "/rest/",
            "/restapi",
            "/rpc/",
            "/jsonrpc",
            "/soap",
            "/graphql",
            "/gql",
            "/v1/",
            "/v2/",
            "/v3/",
            "/v4/",
            "/swagger",
            "swagger.json",
            "swagger.yaml",
            "/openapi",
            "openapi.json",
            "openapi.yaml",
            "api-docs",
            "/wp-json",
            "/graphiql",
            "/.well-known/openapi",
        ],
        &[
            "/api", "/graphql", "/gql", "/rest", "/v1", "/v2", "/v3", "/v4", "/rpc",
        ],
    )
}

/// Canonical preset name for each variant, in the order `--help` should list
/// them. Used to validate `--preset` and to name the alternatives in the error.
pub const PRESET_IDS: [&str; 17] = [
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
    "only-secrets",
    "only-backup",
    "only-config",
    "only-api",
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
            "only-secret" | "only-secrets" => Some(FilterPreset::OnlySecrets),
            "only-backup" | "only-backups" => Some(FilterPreset::OnlyBackup),
            "only-config" | "only-configs" => Some(FilterPreset::OnlyConfig),
            "only-api" | "only-apis" => Some(FilterPreset::OnlyApi),
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
            | FilterPreset::OnlyImages
            | FilterPreset::OnlySecrets
            | FilterPreset::OnlyBackup
            | FilterPreset::OnlyConfig
            | FilterPreset::OnlyApi => vec![],
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
            FilterPreset::OnlySecrets => SECRET_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyBackup => BACKUP_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyConfig => CONFIG_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::OnlyApi => API_EXTENSIONS.iter().map(|&s| s.to_string()).collect(),
            FilterPreset::NoResources
            | FilterPreset::NoImages
            | FilterPreset::NoFonts
            | FilterPreset::NoDocuments
            | FilterPreset::NoVideos
            | FilterPreset::NoAudio => vec![],
        }
    }

    /// Get excluded patterns for this preset
    ///
    /// These are merged into the filter's `--exclude-patterns` list, so a URL
    /// containing any of them is dropped outright.
    pub fn get_exclude_patterns(&self) -> Vec<String> {
        vec![]
    }

    /// Get included patterns for this preset
    ///
    /// These land in the filter's `--patterns` list, which is *ANDed* with the
    /// extension list — a URL has to satisfy both. That is the wrong shape for
    /// the security presets (a backup is `report.bak` **or** `index.php~`), so
    /// they express their path matching through [`FilterPreset::get_path_rules`]
    /// instead and this stays empty for every preset.
    pub fn get_patterns(&self) -> Vec<String> {
        vec![]
    }

    /// Path shapes this preset accepts, ORed with [`FilterPreset::get_extensions`].
    ///
    /// A URL qualifies for an `only-*` preset when it matches the extension
    /// list *or* one of these rules. The two have to be alternatives rather
    /// than requirements: `only-backup` must keep both `db.sql` (extension)
    /// and `index.php~` (path shape), and neither carries the other's marker.
    pub fn get_path_rules(&self) -> Vec<PathRule> {
        match self {
            FilterPreset::OnlySecrets => secret_path_rules(),
            FilterPreset::OnlyBackup => backup_path_rules(),
            FilterPreset::OnlyConfig => config_path_rules(),
            FilterPreset::OnlyApi => api_path_rules(),
            // The extension-family presets predate path rules and must keep
            // behaving exactly as they did.
            FilterPreset::NoResources
            | FilterPreset::NoImages
            | FilterPreset::NoFonts
            | FilterPreset::NoDocuments
            | FilterPreset::NoVideos
            | FilterPreset::NoAudio
            | FilterPreset::OnlyJs
            | FilterPreset::OnlyStyle
            | FilterPreset::OnlyFonts
            | FilterPreset::OnlyDocuments
            | FilterPreset::OnlyVideos
            | FilterPreset::OnlyAudio
            | FilterPreset::OnlyImages => vec![],
        }
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
    fn test_security_presets_parse_including_singular_aliases() {
        for (name, alias) in [
            ("only-secrets", "only-secret"),
            ("only-backup", "only-backups"),
            ("only-config", "only-configs"),
            ("only-api", "only-apis"),
        ] {
            let a = FilterPreset::from_str(name).unwrap_or_else(|| panic!("{name}"));
            let b = FilterPreset::from_str(alias).unwrap_or_else(|| panic!("{alias}"));
            assert_eq!(a.get_extensions(), b.get_extensions(), "{name}");
            assert_eq!(a.get_path_rules(), b.get_path_rules(), "{name}");
        }
        // Case insensitivity works for the new names too.
        assert!(matches!(
            FilterPreset::from_str("Only-Secrets"),
            Some(FilterPreset::OnlySecrets)
        ));
    }

    #[test]
    fn test_security_presets_only_include() {
        // They are `only-*` presets: like every other one, they narrow with an
        // inclusion list and exclude nothing.
        for preset in [
            FilterPreset::OnlySecrets,
            FilterPreset::OnlyBackup,
            FilterPreset::OnlyConfig,
            FilterPreset::OnlyApi,
        ] {
            assert!(preset.get_exclude_extensions().is_empty());
            assert!(preset.get_exclude_patterns().is_empty());
            assert!(!preset.get_path_rules().is_empty());
        }
    }

    #[test]
    fn test_only_the_security_presets_define_path_rules() {
        // Regression guard: the 13 original presets predate path rules, and
        // adding one to any of them would change what it matches.
        for id in PRESET_IDS {
            let preset = FilterPreset::from_str(id).expect(id);
            let is_new = matches!(
                preset,
                FilterPreset::OnlySecrets
                    | FilterPreset::OnlyBackup
                    | FilterPreset::OnlyConfig
                    | FilterPreset::OnlyApi
            );
            assert_eq!(!preset.get_path_rules().is_empty(), is_new, "{id}");
        }
    }

    #[test]
    fn test_path_rules_are_lower_case() {
        // They are compared against a lower-cased URL, so an upper-case
        // character in a rule could never match anything.
        for id in PRESET_IDS {
            for rule in FilterPreset::from_str(id).expect(id).get_path_rules() {
                let literal = match &rule {
                    PathRule::Contains(s) | PathRule::PathEndsWith(s) => s.clone(),
                };
                assert_eq!(literal, literal.to_lowercase(), "{id}: {literal}");
            }
        }
    }

    #[test]
    fn test_preset_extension_lists_have_no_leading_dots() {
        // `Path::extension()` never reports the dot, so ".bak" in a table
        // would be a token that can never match.
        for id in PRESET_IDS {
            let preset = FilterPreset::from_str(id).expect(id);
            for ext in preset
                .get_extensions()
                .into_iter()
                .chain(preset.get_exclude_extensions())
            {
                assert!(!ext.starts_with('.'), "{id}: {ext}");
                assert!(!ext.is_empty(), "{id}: empty extension");
            }
        }
    }

    #[test]
    fn test_security_presets_cover_the_documented_families() {
        let secrets = FilterPreset::OnlySecrets;
        assert!(secrets.get_extensions().contains(&"pem".to_string()));
        assert!(secrets
            .get_path_rules()
            .contains(&PathRule::Contains("/.git/".to_string())));

        let backup = FilterPreset::OnlyBackup;
        for ext in [
            "bak", "old", "swp", "orig", "save", "backup", "zip", "sql", "dump",
        ] {
            assert!(
                backup.get_extensions().contains(&ext.to_string()),
                "only-backup is missing {ext}"
            );
        }
        assert!(backup
            .get_path_rules()
            .contains(&PathRule::PathEndsWith("~".to_string())));

        let config = FilterPreset::OnlyConfig;
        for ext in ["conf", "config", "ini", "yaml", "yml", "toml", "properties"] {
            assert!(
                config.get_extensions().contains(&ext.to_string()),
                "only-config is missing {ext}"
            );
        }

        let api = FilterPreset::OnlyApi;
        assert!(api.get_extensions().contains(&"wsdl".to_string()));
        for needle in ["/api/", "/v1/", "/graphql", "/swagger", "/openapi"] {
            assert!(
                api.get_path_rules()
                    .contains(&PathRule::Contains(needle.to_string())),
                "only-api is missing {needle}"
            );
        }
    }

    #[test]
    fn test_validate_presets_accepts_the_security_names() {
        assert!(validate_presets(&[
            "only-secrets".to_string(),
            "only-backup".to_string(),
            "only-config".to_string(),
            "only-api".to_string(),
        ])
        .is_ok());

        // ...and an unknown name is still an error that names the alternatives.
        let err = validate_presets(&["only-secretz".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("only-secrets"), "{err}");
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
