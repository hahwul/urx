use std::sync::LazyLock;

use regex::Regex;
use url::Url;

/// Standard filter presets for common URL filtering scenarios
#[derive(Clone, Copy)]
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
    /// Only includes URLs that carry a credential in the URL itself
    OnlyTokens,
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
    /// Matches when the lower-cased URL, from the path on (path, query and
    /// fragment), contains this (lower-case) substring. The host is not
    /// searched: `https://api.…` is not an API path.
    Contains(String),
    /// Matches when the lower-cased URL *path* ends with this (lower-case)
    /// suffix. Anchored at the end of the path rather than the end of the
    /// whole URL so a query string (`/index.php~?v=1`) does not defeat it.
    PathEndsWith(String),
    /// Matches when the URL itself carries a credential — see
    /// [`carries_credential`]. Not a substring rule: it needs the parameter
    /// *names* and the original letter case, neither of which survives the
    /// lower-cased haystack the two rules above search.
    CredentialInUrl,
}

fn rules(contains: &[&str], ends_with: &[&str]) -> Vec<PathRule> {
    let contains = contains.iter().map(|s| PathRule::Contains(s.to_string()));
    let ends = ends_with
        .iter()
        .map(|s| PathRule::PathEndsWith(s.to_string()));
    contains.chain(ends).collect()
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
    "erf", "mrw", "rw2", "pef", "dicom", "fpx", "hdr", "mng", "ora", "pic", "rgb", "rgba", "xbm",
    "xpm", "dpx", "fits", "flif", "img", "mpo", "psb",
];

const FONT_EXTENSIONS: &[&str] = &[
    "ttf", "otf", "woff", "woff2", "eot", "fon", "fnt", "ttc", "dfont", "pfa", "pfb",
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
    "js", "jsx", "tsx", "mjs", "cjs", "vue", "json", "coffee", "es6", "es", "svelte", "astro",
    "njk", "map",
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

/// Query parameter names that carry a token, key or signature, compared as a
/// *suffix* of the name reduced to its lower-cased ASCII letters and digits —
/// so `access_token`, `accessToken`, `access-token` and `user[access_token]`
/// are all `…accesstoken`.
///
/// The last five are the distinctive parameters of a pre-signed cloud storage
/// URL (S3 v4 and v2, GCS, CloudFront). A bare `signature` is deliberately
/// absent: image CDNs sign every thumbnail URL with one.
const TOKEN_PARAM_SUFFIXES: &[&str] = &[
    "token",
    "secret",
    "apikey",
    "accesskey",
    "secretkey",
    "privatekey",
    "authkey",
    "subscriptionkey",
    "authorization",
    "bearer",
    "jwt",
    "amzsignature",
    "googsignature",
    "accesskeyid",
    "keypairid",
    "googleaccessid",
];

/// `…token` names that are not credentials: anti-CSRF nonces and pagination
/// cursors. Left in, they would be most of what the preset returns.
// ponytail: a suffix list, so `fb_page_token` is lost to `pagetoken`. Split
// names into words if a real credential turns up under one of these.
const NOT_TOKEN_SUFFIXES: &[&str] = &[
    "csrftoken",
    "xsrftoken",
    "csrfmiddlewaretoken",
    "authenticitytoken",
    "requestverificationtoken",
    "forgerytoken",
    "formtoken",
    "securitytoken",
    "edittoken",
    "pagetoken",
    "nexttoken",
    "continuationtoken",
    "paginationtoken",
    "skiptoken",
    "synctoken",
];

const PASSWORD_PARAM_SUFFIXES: &[&str] = &["password", "passwd", "pwd", "passphrase"];

/// Values that make a password-named parameter a switch (`show_password=1`).
const FLAG_VALUES: &[&str] = &["0", "1", "true", "false", "yes", "no", "on", "off"];

/// Shortest value taken for a token: `token=1` and `auth=true` are flags.
const MIN_TOKEN_LEN: usize = 8;

/// A token shorter than this must contain a digit. A random string that short
/// without one is rare; `undefined`, `facebook` and `YOUR_API_KEY` are not.
const LONG_TOKEN_LEN: usize = 20;

/// Credentials recognisable by their own shape, wherever they sit in the URL:
/// a JWT (as often a path segment — `/verify/eyJ…` — as a parameter) and a
/// Google API key (which travels as a plain `key=`, a name far too generic to
/// list above). Case-sensitive, so it is run on the URL as collected.
// ponytail: two shapes, the ones a parameter name cannot catch. Not a secret
// scanner — pipe the output to trufflehog/gitleaks for vendor token formats.
static TOKEN_SHAPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"eyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.|AIza[0-9A-Za-z_-]{35}")
        .expect("TOKEN_SHAPES is a valid regex")
});

/// The last word of a parameter name, lower-cased: `db_pass`, `userPass` and
/// `user[pass]` all end in `pass`, and `bypass` does not.
fn last_word(name: &str) -> String {
    let name = name.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
    let (mut start, mut after_lower) = (0, false);
    for (i, c) in name.char_indices() {
        if !c.is_ascii_alphanumeric() {
            start = i + c.len_utf8();
        } else if c.is_ascii_uppercase() && after_lower {
            start = i;
        }
        after_lower = c.is_ascii_lowercase();
    }
    name[start..].to_ascii_lowercase()
}

/// Whether one decoded `name=value` pair is a credential.
fn is_credential_param(name: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    // Laravel and Symfony call their CSRF field `_token` / `form[_token]`,
    // which the squashing below would turn into a plain `token`.
    let unbracketed = name.trim_end_matches(']');
    if unbracketed
        .strip_suffix("_token")
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('['))
    {
        return false;
    }

    let squashed: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let ends_with_any = |suffixes: &[&str]| suffixes.iter().any(|s| squashed.ends_with(s));
    // `pass` and `auth` are whole words: as suffixes they are `bypass` and `oauth`.
    let word = last_word(name);

    if word == "pass" || ends_with_any(PASSWORD_PARAM_SUFFIXES) {
        // A password has no shape worth assuming, beyond not being a switch.
        return !FLAG_VALUES.contains(&value.to_ascii_lowercase().as_str());
    }
    let len = value.chars().count();
    len >= MIN_TOKEN_LEN
        && (len >= LONG_TOKEN_LEN || value.contains(|c: char| c.is_ascii_digit()))
        && (word == "auth" || ends_with_any(TOKEN_PARAM_SUFFIXES))
        && !ends_with_any(NOT_TOKEN_SUFFIXES)
}

/// Whether `url` carries a credential in the URL itself — the `only-tokens`
/// preset.
///
/// Four things qualify: a password in the userinfo (`https://user:pw@host/`),
/// a parameter whose *name* says credential (`access_token=…`, `api_key=…`,
/// `password=…`, `X-Amz-Signature=…`), an Azure SAS signature (`sig=`
/// alongside `sv=`; `sig` alone signs thumbnails), and a value with a
/// credential's shape under any name ([`TOKEN_SHAPES`]).
/// Parameters are read from the query *and* the fragment: the OAuth implicit
/// flow returns `#access_token=…`, and SPAs route `#/reset?token=…`. A value
/// that is itself a URL (`redirect=https%3A%2F%2F…%3Ftoken%3D…`) is searched
/// the same way.
pub fn carries_credential(url: &str) -> bool {
    if TOKEN_SHAPES.is_match(url) {
        return true;
    }
    if url.contains('@')
        && Url::parse(url).is_ok_and(|u| u.password().is_some_and(|p| !p.is_empty()))
    {
        return true;
    }
    let (mut sas_sig, mut sas_version) = (false, false);
    for part in url.split(['?', '#']).skip(1) {
        for (name, value) in url::form_urlencoded::parse(part.as_bytes()) {
            // Decoding only ever shortens, so the recursion terminates.
            if is_credential_param(&name, &value)
                || (value.contains(['?', '#']) && carries_credential(&value))
            {
                return true;
            }
            // Archived hrefs are often HTML-escaped: `&amp;sig=` reads as `amp;sig`.
            let plain = name.strip_prefix("amp;").unwrap_or(&name);
            if plain.eq_ignore_ascii_case("sig") {
                sas_sig |= !value.is_empty();
            } else if plain.eq_ignore_ascii_case("sv") {
                sas_version = true;
            }
        }
    }
    sas_sig && sas_version
}

/// Every preset with its canonical `--preset` name and the other spelling it
/// accepts, in the order `--help` should list them.
///
/// Both singular and plural spellings are accepted for every preset, so
/// `only-image` and `only-images` mean the same thing.
const PRESETS: [(FilterPreset, &str, &str); 18] = [
    (FilterPreset::NoResources, "no-resources", "no-resource"),
    (FilterPreset::NoImages, "no-images", "no-image"),
    (FilterPreset::NoFonts, "no-fonts", "no-font"),
    (FilterPreset::NoDocuments, "no-documents", "no-document"),
    (FilterPreset::NoVideos, "no-videos", "no-video"),
    (FilterPreset::NoAudio, "no-audio", "no-audios"),
    (FilterPreset::OnlyJs, "only-js", "only-js"),
    (FilterPreset::OnlyStyle, "only-style", "only-styles"),
    (FilterPreset::OnlyFonts, "only-fonts", "only-font"),
    (
        FilterPreset::OnlyDocuments,
        "only-documents",
        "only-document",
    ),
    (FilterPreset::OnlyVideos, "only-videos", "only-video"),
    (FilterPreset::OnlyAudio, "only-audio", "only-audios"),
    (FilterPreset::OnlyImages, "only-images", "only-image"),
    (FilterPreset::OnlySecrets, "only-secrets", "only-secret"),
    (FilterPreset::OnlyBackup, "only-backup", "only-backups"),
    (FilterPreset::OnlyConfig, "only-config", "only-configs"),
    (FilterPreset::OnlyApi, "only-api", "only-apis"),
    (FilterPreset::OnlyTokens, "only-tokens", "only-token"),
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
    let ids: Vec<&str> = PRESETS.iter().map(|(_, id, _)| *id).collect();
    Err(anyhow::anyhow!(
        "Unknown preset(s) in --preset: {}. Allowed values: {}, or a name defined under [presets] in the config file",
        unknown.join(", "),
        ids.join(", ")
    ))
}

type Extensions = &'static [&'static str];

impl FilterPreset {
    /// Parse a preset string (case-insensitive, either spelling).
    pub fn from_str(s: &str) -> Option<Self> {
        let s = s.to_lowercase();
        PRESETS
            .iter()
            .find(|(_, id, alias)| s == *id || s == *alias)
            .map(|(preset, _, _)| *preset)
    }

    /// What this preset does: the extensions it drops, the extensions it
    /// keeps, and the path shapes it keeps.
    ///
    /// Every `only-*` preset *keeps* its family and drops nothing. Returning
    /// e.g. the image extensions as *exclusions* made `--preset only-images`
    /// drop every image and keep everything else — the exact inverse of its
    /// name. Only the security presets have path rules; the extension-family
    /// presets predate them and must keep behaving exactly as they did.
    fn spec(&self) -> (&'static [Extensions], Extensions, Vec<PathRule>) {
        use FilterPreset::*;
        match self {
            NoResources => (
                &[
                    IMAGE_EXTENSIONS,
                    FONT_EXTENSIONS,
                    DOCUMENT_EXTENSIONS,
                    AUDIO_EXTENSIONS,
                    VIDEO_EXTENSIONS,
                    JS_EXTENSIONS,
                    STYLE_EXTENSIONS,
                ],
                &[],
                vec![],
            ),
            NoImages => (&[IMAGE_EXTENSIONS], &[], vec![]),
            NoFonts => (&[FONT_EXTENSIONS], &[], vec![]),
            NoDocuments => (&[DOCUMENT_EXTENSIONS], &[], vec![]),
            NoVideos => (&[VIDEO_EXTENSIONS], &[], vec![]),
            NoAudio => (&[AUDIO_EXTENSIONS], &[], vec![]),
            OnlyJs => (&[], JS_EXTENSIONS, vec![]),
            OnlyStyle => (&[], STYLE_EXTENSIONS, vec![]),
            OnlyFonts => (&[], FONT_EXTENSIONS, vec![]),
            OnlyDocuments => (&[], DOCUMENT_EXTENSIONS, vec![]),
            OnlyVideos => (&[], VIDEO_EXTENSIONS, vec![]),
            OnlyAudio => (&[], AUDIO_EXTENSIONS, vec![]),
            OnlyImages => (&[], IMAGE_EXTENSIONS, vec![]),
            OnlySecrets => (&[], SECRET_EXTENSIONS, secret_path_rules()),
            OnlyBackup => (&[], BACKUP_EXTENSIONS, backup_path_rules()),
            OnlyConfig => (&[], CONFIG_EXTENSIONS, config_path_rules()),
            OnlyApi => (&[], API_EXTENSIONS, api_path_rules()),
            OnlyTokens => (&[], &[], vec![PathRule::CredentialInUrl]),
        }
    }

    /// Get excluded extensions for this preset
    pub fn get_exclude_extensions(&self) -> Vec<String> {
        self.spec()
            .0
            .concat()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Get included extensions for this preset
    pub fn get_extensions(&self) -> Vec<String> {
        self.spec().1.iter().map(|s| s.to_string()).collect()
    }

    /// Path shapes this preset accepts, ORed with [`FilterPreset::get_extensions`].
    ///
    /// A URL qualifies for an `only-*` preset when it matches the extension
    /// list *or* one of these rules. The two have to be alternatives rather
    /// than requirements: `only-backup` must keep both `db.sql` (extension)
    /// and `index.php~` (path shape), and neither carries the other's marker.
    /// (`--patterns` would not do: it is *ANDed* with the extension list.)
    pub fn get_path_rules(&self) -> Vec<PathRule> {
        self.spec().2
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
    fn test_media_families_do_not_overlap() {
        // Regression: `svg` was a font too, `ts` JS too and `djvu` an image
        // too, so `no-fonts` dropped logos and `only-js` returned HLS
        // segments. Each `no-X`/`only-X` must act on its own family only.
        // `ts` stays a video: in archives it is an MPEG-TS segment far more
        // often than a served TypeScript source.
        let families = [
            IMAGE_EXTENSIONS,
            FONT_EXTENSIONS,
            DOCUMENT_EXTENSIONS,
            AUDIO_EXTENSIONS,
            VIDEO_EXTENSIONS,
            JS_EXTENSIONS,
            STYLE_EXTENSIONS,
        ];
        let mut seen = std::collections::HashSet::new();
        for ext in families.iter().flat_map(|f| f.iter()) {
            assert!(seen.insert(ext), "{ext} is in two families");
        }
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
        // `ts` is an MPEG-TS segment, a video; `tsx` is unambiguous.
        assert!(!extensions.contains(&"ts".to_string()));
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
    fn test_security_presets_parse_including_singular_aliases() {
        for (name, alias) in [
            ("only-secrets", "only-secret"),
            ("only-backup", "only-backups"),
            ("only-config", "only-configs"),
            ("only-api", "only-apis"),
            ("only-tokens", "only-token"),
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
            FilterPreset::OnlyTokens,
        ] {
            assert!(preset.get_exclude_extensions().is_empty());
            assert!(!preset.get_path_rules().is_empty());
        }
    }

    #[test]
    fn test_only_the_security_presets_define_path_rules() {
        // Regression guard: the 13 original presets predate path rules, and
        // adding one to any of them would change what it matches.
        for (_, id, _) in PRESETS {
            let preset = FilterPreset::from_str(id).expect(id);
            let is_new = matches!(
                preset,
                FilterPreset::OnlySecrets
                    | FilterPreset::OnlyBackup
                    | FilterPreset::OnlyConfig
                    | FilterPreset::OnlyApi
                    | FilterPreset::OnlyTokens
            );
            assert_eq!(!preset.get_path_rules().is_empty(), is_new, "{id}");
        }
    }

    #[test]
    fn test_path_rules_are_lower_case() {
        // They are compared against a lower-cased URL, so an upper-case
        // character in a rule could never match anything.
        for (_, id, _) in PRESETS {
            for rule in FilterPreset::from_str(id).expect(id).get_path_rules() {
                let literal = match &rule {
                    PathRule::Contains(s) | PathRule::PathEndsWith(s) => s.clone(),
                    PathRule::CredentialInUrl => continue,
                };
                assert_eq!(literal, literal.to_lowercase(), "{id}: {literal}");
            }
        }
    }

    #[test]
    fn test_preset_extension_lists_have_no_leading_dots() {
        // `Path::extension()` never reports the dot, so ".bak" in a table
        // would be a token that can never match.
        for (_, id, _) in PRESETS {
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
    fn test_carries_credential() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJl";
        let jwt_in_path = format!("https://example.com/verify/{jwt}");
        let jwt_in_value = format!("https://example.com/sso?state={jwt}");
        let yes = [
            // Named parameters, in every spelling of the name.
            "https://example.com/cb?access_token=ya29.a0AfH6SMBx",
            "https://example.com/cb?accessToken=ya29.a0AfH6SMBx",
            "https://example.com/v1/items?page=2&api-key=0123456789abcdef",
            "https://example.com/reset?user%5Breset_password_token%5D=aB3dE6gH9j",
            "https://example.com/data.json?auth=Fz8kQm2LxP0wRt5v",
            "https://example.com/a?x-auth=0123456789abcdef",
            // No digit, but far too long to be a word.
            "https://example.com/a?client_secret=QwErTyUiOpAsDfGhJkLzXcVb",
            // Passwords have no minimum length.
            "https://example.com/login.php?user=admin&pass=1234",
            "https://example.com/login?Password=hunter2",
            "https://example.com/install.php?db_user=root&db_pass=hunter2",
            "https://example.com/a?userPass=hunter2",
            // Userinfo.
            "https://deploy:s3cr3t@git.example.com/repo.git",
            // Fragments: OAuth implicit flow and hash routes.
            "https://example.com/cb#access_token=ya29.a0AfH6SMBx&token_type=bearer",
            "https://example.com/#/reset?token=aB3dE6gH9jK2",
            // Pre-signed storage URLs.
            "https://example.com/f.pdf?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Signature=0123abcd4567",
            "https://example.com/f.pdf?AWSAccessKeyId=AKIAIOSFODNN7EXAMPLE&Expires=1&Signature=x",
            "https://example.com/f.pdf?sv=2020-08-04&se=2030-01-01&sig=dGVzdHNpZw%3D%3D",
            "https://example.com/f.pdf?sv=2020-08-04&amp;se=2030-01-01&amp;sig=dGVzdHNpZw%3D%3D",
            // A credential inside a percent-encoded URL value.
            "https://example.com/go?redirect=https%3A%2F%2Fexample.com%2Fcb%3Faccess_token%3Dya29.a0AfH6SMBx",
            // Shapes, under a name that says nothing — or no name at all.
            jwt_in_path.as_str(),
            jwt_in_value.as_str(),
            "https://example.com/map?key=AIzaSyA-0123456789abcdefghijklmnopqrstu",
        ];
        for url in yes {
            assert!(carries_credential(url), "should match: {url}");
        }

        let no = [
            "https://example.com/",
            "https://example.com/search?q=token",
            "https://example.com/tokens/list",
            // Flags and empty values, not credentials.
            "https://example.com/a?token=1",
            "https://example.com/a?auth=true",
            "https://example.com/a?token=",
            "https://example.com/a?password=",
            "https://example.com/a?show_password=1",
            // Words and placeholders: long enough, but no digit.
            "https://example.com/a?token=undefined",
            "https://example.com/a?auth=facebook",
            "https://example.com/a?api_key=YOUR_API_KEY",
            // CSRF nonces and pagination cursors.
            "https://example.com/a?csrf_token=0123456789abcdef",
            "https://example.com/a?authenticity_token=0123456789abcdef",
            "https://example.com/logout?_token=Zx9AbCdEfGh1234567",
            "https://example.com/a?form%5B_token%5D=Zx9AbCdEfGh1234567",
            "https://example.com/a?nextPageToken=CAESBQj0123456789",
            "https://example.com/a?$skiptoken=X0123456789abcdef",
            // A user name alone is not a credential.
            "https://deploy@git.example.com/repo.git",
            // Suffix lookalikes.
            "https://example.com/a?bypass=0123456789abcdef",
            "https://example.com/a?oauth=facebook-login",
            // A thumbnail signature is not an Azure SAS.
            "https://example.com/img.jpg?w=200&sig=0123456789abcdef",
            // The host is not a parameter.
            "https://token.example.com/password/reset",
        ];
        for url in no {
            assert!(!carries_credential(url), "should not match: {url}");
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
