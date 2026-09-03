//! `--extract-js-endpoints`: mine JavaScript bodies for the endpoints they call.
//!
//! `-e js` collects thousands of bundle URLs and never looks inside them, yet
//! in a modern web app the API surface lives almost entirely in those bundles
//! as string literals — `fetch("/api/v2/users")`, `axios.post("/graphql")`,
//! `` `${base}/orders/${id}` `` — and appears nowhere in the HTML. This tester
//! re-fetches each collected URL that looks like JavaScript, scans the body
//! with a small set of regexes, and hands the paths back so they join the
//! result set alongside everything else (after the same filters and host
//! validation, see `ExtractedLinkFilter`).
//!
//! Regex-mining a minified bundle is a fire hose of garbage unless the output
//! is aggressively pruned, so most of this file is [`is_noise`]. Every rule
//! there names the thing it suppresses and why.

use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use tokio::sync::OnceCell;
use url::Url;

use super::Tester;
use crate::network::client::{read_body_capped, HttpClientConfig};
use crate::network::RateLimiter;

/// Cap on bytes read from one script before scanning.
///
/// The same guard, for the same reason, as the link extractor: the archives
/// hand back whatever they recorded, and a single multi-gigabyte URL must not
/// be buffered whole. 10 MiB comfortably covers the largest real bundles (a
/// 3–5 MiB vendor chunk is already extreme).
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Path extensions that mark a URL as JavaScript regardless of `Content-Type`.
///
/// `.ts` is included because served TypeScript (Deno, Vite dev servers,
/// `.d.ts` on CDNs) carries the same string literals; `.mjs`/`.jsx` for the
/// same reason.
const JS_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "tsx"];

/// Path extensions that are certainly *not* script and carry no inline
/// `<script>` either. These are skipped before any request goes out, so a run
/// over a list full of images and fonts does not re-download all of them just
/// to find nothing.
const NON_SCRIPT_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "ico", "bmp", "tif", "tiff", "css", "scss",
    "less", "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4", "webm", "ogg", "wav", "m4a", "mov",
    "avi", "pdf", "zip", "gz", "tgz", "tar", "rar", "7z", "bz2", "xz", "exe", "dmg", "apk", "json",
    "xml", "txt", "csv", "map", "wasm", "rss", "atom",
];

/// How a fetched body should be scanned, decided from `Content-Type` and the
/// URL's extension.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum BodyKind {
    /// Scan the whole body as script.
    Script,
    /// Scan only the inline `<script>` blocks.
    Html,
    /// Nothing here can carry endpoints; don't read the body.
    Skip,
}

/// The extension of the last path segment of `url`, lower-cased, if any.
fn path_extension(url: &Url) -> Option<String> {
    let last = url.path_segments()?.next_back()?;
    let (_, ext) = last.rsplit_once('.')?;
    // `.htaccess`-style names and trailing dots are not extensions.
    if ext.is_empty() || ext.len() > 5 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Whether the URL is worth a request at all. Only URLs whose extension is
/// definitely non-script are dropped up front; a missing or unknown extension
/// might be anything (`/app/main`, `/bundle`), and the response headers decide.
fn worth_fetching(url: &Url) -> bool {
    match path_extension(url) {
        Some(ext) => !NON_SCRIPT_EXTENSIONS.contains(&ext.as_str()),
        None => true,
    }
}

/// Classify a response by its `Content-Type`, falling back to the extension.
///
/// The extension wins over an explicitly *wrong* type only in one direction:
/// a `.js` URL served as `text/plain` or `application/octet-stream` (both
/// common on misconfigured static hosts) is still scanned as script. An HTML
/// type means only inline `<script>` blocks are scanned — running the script
/// regexes over the markup itself would match every `href` and `src` the link
/// extractor already collects properly.
fn classify(headers: &reqwest::header::HeaderMap, url: &Url) -> BodyKind {
    let ext_is_js = path_extension(url)
        .map(|e| JS_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false);

    let ct = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase());

    match ct.as_deref() {
        Some(ct) if ct.contains("javascript") || ct.contains("ecmascript") => BodyKind::Script,
        Some(ct) if ct.contains("typescript") || ct.contains("text/jsx") => BodyKind::Script,
        Some(ct) if ct.contains("html") => BodyKind::Html,
        // Loose or absent types: trust the extension.
        Some(ct) if ct.contains("text/plain") || ct.contains("octet-stream") => {
            if ext_is_js {
                BodyKind::Script
            } else {
                BodyKind::Skip
            }
        }
        None => {
            if ext_is_js {
                BodyKind::Script
            } else {
                // No header and no JS extension: could be HTML with inline
                // script, could be anything. Scanning as HTML costs nothing if
                // no `<script>` is present.
                BodyKind::Html
            }
        }
        // An explicit, unrelated type (image, css, json, ...) is skipped even
        // when the extension says `.js`: the server knows what it served.
        Some(_) => BodyKind::Skip,
    }
}

/// Where a candidate came from. A string that is *the argument of a request
/// call* is known to be a URL and gets a looser noise policy than a string
/// that merely looks like one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// Any quoted string or template-literal prefix that has a URL shape.
    Literal,
    /// First URL argument of `fetch(` / `axios.*(` / `XMLHttpRequest.open(`.
    RequestCall,
}

/// The regexes, compiled once for the process (`Regex::new` is far too
/// expensive to repeat per body, and a run can scan thousands of bundles).
struct Patterns {
    /// A quoted string — `"…"`, `'…'`, or `` `…` `` — whose content has a URL
    /// shape, terminated by either a closing quote or the first `${`
    /// interpolation of a template literal. The interpolation case is what
    /// turns `` `/api/users/${id}` `` into `/api/users/`.
    ///
    /// The character class deliberately excludes quotes, whitespace, `<>` (tag
    /// fragments), `{}` (interpolations / object literals), `()` `|` `^` `$`
    /// `*` `\` (regex sources like `"/^\\/api/(\\d+)"` fail to terminate and
    /// so never match at all, which is the intent).
    ///
    /// Alternatives, in order:
    /// 1. absolute or protocol-relative URL
    /// 2. origin-relative path `/…`
    /// 3. dot-relative path `./…` / `../…`
    /// 4. bare relative `seg/seg…` (must contain a `/`), optionally with a query
    literal: Regex,
    /// The URL argument of a request call. `fetch("…")`, `axios.get("…")`,
    /// `axios({url:"…"})`, `$.ajax("…")`, `xhr.open("GET", "…")`.
    /// Group 1 is the URL for every alternative.
    request_call: Regex,
    /// `<script>` blocks of an HTML document, so inline code on a page gets
    /// the same treatment as an external bundle. `(?is)` so a `<SCRIPT>` with
    /// attributes and newlines inside still matches.
    inline_script: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    // Everything allowed inside a candidate. Shared by both patterns.
    const BODY: &str = r#"[^"'`\s<>{}()|^$*\\]"#;
    let url_shape = format!(
        r"(?:(?:https?:)?//{b}+|/{b}*|\.\.?/{b}*|[A-Za-z0-9_.\-]+(?:/[A-Za-z0-9_.\-]+)+(?:\?{b}*)?)",
        b = BODY
    );
    // A request call's argument is a URL by construction, so a single bare
    // word (`fetch("users")`) is accepted there and nowhere else.
    let call_shape = format!(
        r"(?:{url_shape}|[A-Za-z0-9_\-][A-Za-z0-9_.\-]*(?:\?{b}*)?)",
        b = BODY
    );
    // A string must close with the quote it opened with. `regex` has no
    // back-references, so the three quote kinds are spelled out as
    // alternatives; [`quoted`] picks whichever group matched. Without this,
    // `.replace(/\\'/g,"'")` yields the "string" `'/g,"` — a real find from a
    // real bundle.
    let quoted = |shape: &str| format!(r#"(?:"({shape})"|'({shape})'|`({shape})(?:`|\$\{{))"#);
    Patterns {
        literal: Regex::new(&quoted(&url_shape)).expect("literal pattern is valid"),
        request_call: Regex::new(&format!(
            r#"(?:\bfetch|\baxios(?:\.(?:get|post|put|patch|delete|head|options|request))?|\$\.(?:ajax|get|post|getJSON)|\.open\s*\(\s*["'`][A-Za-z]+["'`]\s*,|\burl\s*:)\s*\(?\s*(?:[\w$.]+(?:\(\))?\s*\+\s*)*{}"#,
            quoted(&call_shape)
        ))
        .expect("request-call pattern is valid"),
        inline_script: Regex::new(r"(?is)<script\b[^>]*>(.*?)</script\s*>")
            .expect("inline-script pattern is valid"),
    }
});

/// The string a [`Patterns`] match captured, whichever quote kind closed it.
fn quoted<'t>(cap: &regex::Captures<'t>) -> Option<regex::Match<'t>> {
    (1..=3).find_map(|i| cap.get(i))
}

/// Hosts whose URLs are boilerplate in nearly every bundle and never an
/// endpoint of the target: XML namespaces and schema identifiers. Strict mode
/// would drop them anyway; this keeps them out of `--no-strict` output too.
const BOILERPLATE_HOSTS: &[&str] = &["www.w3.org", "schemas.xmlsoap.org", "schema.org"];

/// MIME top-level types. `"image/png"`, `"application/json"`,
/// `"text/plain"` are the single most common false positive: they have exactly
/// the `seg/seg` shape of a bare relative path.
const MIME_TOP_LEVELS: &[&str] = &[
    "application",
    "audio",
    "font",
    "image",
    "message",
    "model",
    "multipart",
    "text",
    "video",
    "chemical",
    "x-conference",
];

/// Whether a candidate is one of the known garbage shapes and should be
/// dropped before resolution. Every rule here exists because a real minified
/// bundle produced that shape; the comments say which.
fn is_noise(candidate: &str, origin: Origin) -> bool {
    let c = candidate;

    // Sourcemap directives: `//# sourceMappingURL=app.js.map` and the `//@`
    // spelling. Bundlers emit the prefix as a string literal too.
    if c.contains("sourceMappingURL") || c.starts_with("//#") || c.starts_with("//@") {
        return true;
    }

    // Trailing punctuation no URL ends with. `.replace(/"/g,"&quot;")` reads
    // as the string `"/g,"` — the regex's own quote opens it — and there is
    // no quote-matching that can tell it apart. A path ending in `,`, `;` or
    // `:` is never a real one.
    if c.ends_with([',', ';', ':']) {
        return true;
    }

    // Comment fragments and CSS comment delimiters that survived as literals:
    // `"//"`, `"/*"`, `"*/"`, `"/**"`. (`*` is excluded from the char class,
    // so these are `"//"` and `"/"`-only strings in practice.) `fetch("/")`
    // is the one legitimate all-slash string: the call proves it is a request.
    let trimmed = c.trim_matches('/');
    if trimmed.is_empty() && !(origin == Origin::RequestCall && c == "/") {
        return true;
    }

    // Anything starting with a scheme or `//` is an absolute URL; judge it by
    // its host.
    if c.starts_with("http://") || c.starts_with("https://") || c.starts_with("//") {
        let host = c
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("//")
            .split(['/', '?', '#', ':'])
            .next()
            .unwrap_or("");
        // `"//"` followed by something that is not a hostname is a comment
        // fragment (`"//*"`, `"//foo"`), not a protocol-relative URL. A real
        // host has a dot (or is `localhost`) and only hostname characters.
        let hostname_chars = host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        if host.is_empty()
            || !hostname_chars
            || !(host.contains('.') || host.eq_ignore_ascii_case("localhost"))
        {
            return true;
        }
        if BOILERPLATE_HOSTS.contains(&host.to_ascii_lowercase().as_str()) {
            return true;
        }
        return false;
    }

    // From here on the candidate is a path. Split into segments, ignoring the
    // query string for the structural checks.
    let path_part = c.split(['?', '#']).next().unwrap_or(c);
    let segments: Vec<&str> = path_part.split('/').filter(|s| !s.is_empty()).collect();

    // `"/"`, `"/x"`: too short to be anything but a router root or a single
    // letter. Minified code is full of `"/"` (path joins, regex-free splits).
    // Request-call arguments are exempt: `fetch("/")` is a real request.
    if origin == Origin::Literal && path_part.trim_start_matches('/').len() < 2 {
        return true;
    }

    // Only dot segments: `"./"`, `"../"`, `"../../"` — path-join helpers, not
    // endpoints.
    if !segments.is_empty() && segments.iter().all(|s| *s == "." || *s == "..") {
        return true;
    }

    // MIME types: `image/png`, `application/x-www-form-urlencoded`,
    // `text/javascript`, `*/*`. Two segments, first is a registered top-level
    // type. Note `text/event-stream` and friends fall here too — correct,
    // they are header values, not paths.
    if segments.len() == 2 && !c.starts_with('/') {
        let top = segments[0].to_ascii_lowercase();
        if MIME_TOP_LEVELS.contains(&top.as_str()) || top == "*" {
            return true;
        }
    }

    // Bare extension: `".js"`, `"/.png"` — file-type checks, not paths.
    if segments.len() == 1 && segments[0].starts_with('.') && !segments[0].starts_with("..") {
        return true;
    }

    for seg in &segments {
        // Base64 blobs: inline images and fonts show up as
        // `data:…;base64,` payloads and as bare `"iVBORw0KGgo…"` strings. A
        // path segment 24+ chars long containing no separator (`.`, `-`, `_`)
        // but mixed-case letters *and* digits, or `+`/`=` at all, is base64,
        // not a route. Hex content hashes (`3f2a1b…`) are lower-case only and
        // survive this; hashed filenames carry a `.` and survive it too.
        if seg.contains('+') || seg.contains('=') {
            return true;
        }
        if seg.len() >= 24 && !seg.contains(['.', '-', '_']) {
            let upper = seg.bytes().any(|b| b.is_ascii_uppercase());
            let lower = seg.bytes().any(|b| b.is_ascii_lowercase());
            let digit = seg.bytes().any(|b| b.is_ascii_digit());
            if upper && lower && digit {
                return true;
            }
        }
    }

    // CSS shorthand and numeric ratios: `"12px/1.5"` (font), `"16/9"`
    // (aspect-ratio), `"1/2"`, `"100%/auto"`. Every segment is numeric with an
    // optional unit or keyword and nothing looks like a word — no letters
    // beyond a unit suffix.
    let all_numeric_ish = segments.iter().all(|s| {
        let digits = s
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'.')
            .count();
        digits > 0 && s.len() - digits <= 4
    });
    if !segments.is_empty() && all_numeric_ish {
        return true;
    }

    // Date/time format strings: `"MM/DD/YYYY"`, `"dd/mm/yy"`, `"HH/mm"`.
    // Every segment consists solely of format letters.
    let all_format_letters = segments.iter().all(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| matches!(b.to_ascii_lowercase(), b'd' | b'm' | b'y' | b'h' | b's'))
    });
    if !segments.is_empty() && all_format_letters {
        return true;
    }

    // Bare relative paths (no leading `/`, `./`, `../`) are the noisiest class:
    // `"a/b"` matches locale tags (`en/US`), lodash paths (`lodash/fp`),
    // module specifiers (`react/jsx-runtime`), unit fractions. Unless the
    // string was the argument of a request call, demand more evidence: at
    // least one segment of three or more characters *and* either three-plus
    // segments, a query string, or a file-like final segment with an
    // extension. `api/users` alone does not pass — in practice such strings
    // are overwhelmingly package paths, and the real call sites use `/api/…`.
    if origin == Origin::Literal && !c.starts_with('/') && !c.starts_with('.') {
        let long_seg = segments.iter().any(|s| s.len() >= 3);
        let has_query = c.contains('?');
        let file_like = segments
            .last()
            .map(|s| s.contains('.') && !s.ends_with('.'))
            .unwrap_or(false);
        if !(long_seg && (segments.len() >= 3 || has_query || file_like)) {
            return true;
        }
    }

    // Dot-relative strings without a fetchable extension — `"./utils"`,
    // `"../compressions"`, `"./zlib/deflate"` — are module specifiers:
    // browserify/CommonJS `require("./x")` calls and the `{"./x":12}` module
    // maps they leave in a bundle. A real Element bundle contributed ~50 of
    // these from jszip and pako alone. The ones that *do* name a file
    // (`import("./chunk-ab12.js")`, `"./assets/logo.svg"`) are kept: they are
    // URLs the browser fetches. A request-call argument is trusted either way.
    if origin == Origin::Literal && c.starts_with('.') && !has_asset_extension(path_part) {
        return true;
    }

    // Package and source-tree paths: `@scope/pkg/dist/x.js`, `node_modules/…`,
    // `lib/esm/…`, and — as webpack module keys — `./src/vector/init.tsx`,
    // `../../node_modules/.pnpm/katex@0.18.4/…`. These are import specifiers
    // and build-time file names, never URLs the app requests. Judged on the
    // first non-dot segment so the dot-relative spellings are caught too.
    if origin == Origin::Literal && !c.starts_with('/') {
        if let Some(first) = segments.iter().find(|s| **s != "." && **s != "..") {
            if first.starts_with('@')
                || matches!(
                    *first,
                    "node_modules" | "lib" | "dist" | "esm" | "cjs" | "src" | "packages"
                )
            {
                return true;
            }
        }
    }

    false
}

/// Whether the last path segment carries a file extension that a browser
/// would fetch: a script chunk, stylesheet, image, font, data file.
fn has_asset_extension(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = last.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=5).contains(&ext.len())
        && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        && (JS_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
            || NON_SCRIPT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
            || matches!(
                ext.to_ascii_lowercase().as_str(),
                "html" | "htm" | "php" | "aspx"
            ))
}

/// How far (in bytes) after a `/`-terminated literal a `+ "..."` still counts
/// as continuing that path. `"/users/" + encodeURIComponent(id) + "/avatar"`
/// fits comfortably; two unrelated strings a statement apart do not.
const CONCAT_WINDOW: usize = 64;

/// Whether the literal starting at `start` follows a `+`, i.e. is the right
/// operand of a concatenation.
fn is_concat_suffix(script: &str, start: usize) -> bool {
    script[..start]
        .bytes()
        .rev()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| b == b'+')
}

/// Resolve a surviving candidate to an absolute URL against the script's URL.
///
/// Which base to use depends on the candidate's shape, and the choice is *not*
/// the same as for an HTML `href`:
///
/// - Absolute (`https://…`) and protocol-relative (`//…`) need no base beyond
///   the scheme.
/// - Origin-relative (`/api/x`) resolves against the script's origin. This is
///   exactly what the browser does at runtime regardless of which page loaded
///   the script, so it is the one case that is unambiguous.
/// - Dot-relative paths that name a file (`./chunk-ab12.js`,
///   `../assets/logo.svg`) are ES-module imports and asset references, which
///   resolve against the *importing module's* URL — so they take the script's
///   directory as base, like an `href` would. (Any other dot-relative string
///   is a CommonJS module specifier and has already been dropped as noise.)
/// - Bare relative paths (`api/v1/x`), and dot-relative request-call
///   arguments (`fetch("./api")`), resolve at runtime against the *document*
///   URL — the page that loaded the bundle — which urx does not know.
///   Resolving against the bundle's own directory (`/static/js/`) would be
///   actively wrong: no app serves its API from under its asset path. The
///   origin root is the least-wrong guess (most SPAs are served from `/`),
///   and is what LinkFinder does as well.
fn resolve(base: &Url, candidate: &str) -> Option<String> {
    let resolved = if candidate.starts_with("http://")
        || candidate.starts_with("https://")
        || candidate.starts_with("//")
        || candidate.starts_with('/')
        || (candidate.starts_with('.') && has_asset_extension(candidate))
    {
        base.join(candidate).ok()?
    } else {
        let mut root = base.clone();
        root.set_path("/");
        root.set_query(None);
        root.set_fragment(None);
        root.join(candidate).ok()?
    };
    // A fragment is never sent to the server; drop it so `/app#/route` and
    // `/app` collapse.
    let mut resolved = resolved;
    resolved.set_fragment(None);
    Some(resolved.to_string())
}

/// JavaScript endpoint extractor: fetches script bodies and mines them for
/// the paths and URLs they reference.
#[derive(Clone)]
pub struct JsEndpointExtractor {
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    /// Upper bound on scripts fetched across the whole run; `0` is unlimited.
    max_files: usize,
    /// Fetches performed so far, shared across `clone_box` clones so the cap
    /// is global rather than per worker.
    fetched: Arc<AtomicUsize>,
    /// `--rate-limit`, shared across clones for the same reason. Testers did
    /// not previously honour the rate limit at all; this one does because it
    /// re-requests a large slice of the result set from the target itself.
    rate_limiter: Option<RateLimiter>,
    /// One HTTP client, built lazily and shared across clones — see the same
    /// field on `LinkExtractor`.
    client: Arc<OnceCell<Client>>,
}

impl JsEndpointExtractor {
    /// Default cap on scripts fetched per run. Each one is a full request and
    /// up to 10 MiB of body; an `-e js` list for a large site runs to the
    /// thousands, and nobody wants that unbounded by accident.
    pub const DEFAULT_MAX_FILES: usize = 500;

    pub fn new() -> Self {
        JsEndpointExtractor {
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            max_files: Self::DEFAULT_MAX_FILES,
            fetched: Arc::new(AtomicUsize::new(0)),
            rate_limiter: None,
            client: Arc::new(OnceCell::new()),
        }
    }

    /// Cap the number of scripts fetched; `0` means no cap.
    pub fn with_max_files(&mut self, max: usize) {
        self.max_files = max;
    }

    /// Pace requests at `requests_per_sec`, as `--rate-limit` does for
    /// providers.
    pub fn with_rate_limit(&mut self, requests_per_sec: Option<f32>) {
        self.rate_limiter = RateLimiter::from_rate(requests_per_sec);
    }

    /// Scripts fetched so far.
    #[cfg(test)]
    fn fetched(&self) -> usize {
        self.fetched.load(Ordering::Relaxed)
    }

    fn client_config(&self) -> HttpClientConfig {
        HttpClientConfig {
            timeout: self.timeout,
            insecure: self.insecure,
            random_agent: self.random_agent,
            proxy: self.proxy.clone(),
            proxy_auth: self.proxy_auth.clone(),
        }
    }

    async fn client(&self) -> Result<&Client> {
        self.client
            .get_or_try_init(|| async { self.client_config().build_client() })
            .await
    }

    /// Reserve one slot under the fetch cap, or `false` if the cap is spent.
    fn try_reserve_fetch(&self) -> bool {
        if self.max_files == 0 {
            self.fetched.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        self.fetched
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < self.max_files).then_some(n + 1)
            })
            .is_ok()
    }

    /// Mine `script` for endpoints, resolved against `base` and deduplicated
    /// in first-seen order.
    pub fn extract_endpoints(base: &Url, script: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();

        let mut push = |raw: &str, origin: Origin| {
            let raw = raw.trim();
            if raw.is_empty() || is_noise(raw, origin) {
                return;
            }
            if let Some(url) = resolve(base, raw) {
                if seen.insert(url.clone()) {
                    out.push(url);
                }
            }
        };

        // Request-call arguments first: they carry the looser policy, and a
        // string seen here is then already deduplicated when the general
        // literal pass meets it again.
        for cap in PATTERNS.request_call.captures_iter(script) {
            if let Some(m) = quoted(&cap) {
                push(m.as_str(), Origin::RequestCall);
            }
        }
        // End offset of the most recent literal that ended in `/`, i.e. a
        // path *prefix* awaiting concatenation. See below.
        let mut open_prefix_end: Option<usize> = None;
        for cap in PATTERNS.literal.captures_iter(script) {
            let (Some(whole), Some(m)) = (cap.get(0), quoted(&cap)) else {
                continue;
            };
            // The right-hand side of a concatenation that continues a path —
            // `"/users/" + id + "/avatar"` — is a path *suffix*; emitting
            // `/avatar` on its own is noise. But `baseUrl + "/api/v2"` is the
            // commonest way real endpoints are written, so a `+` alone is not
            // enough: the suffix is dropped only when a literal ending in `/`
            // (the prefix) appeared just before it. The prefix itself is
            // kept, exactly like a template literal's static prefix.
            // (`regex` has no look-behind, so the surrounding bytes are
            // inspected by hand.)
            let continues_prefix = open_prefix_end
                .is_some_and(|end| whole.start().saturating_sub(end) <= CONCAT_WINDOW)
                && is_concat_suffix(script, whole.start());
            open_prefix_end = m.as_str().ends_with('/').then_some(whole.end());
            if continues_prefix {
                continue;
            }
            push(m.as_str(), Origin::Literal);
        }

        out
    }

    /// Mine only the inline `<script>` blocks of an HTML document.
    pub fn extract_inline_endpoints(base: &Url, html: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for cap in PATTERNS.inline_script.captures_iter(html) {
            if let Some(block) = cap.get(1) {
                for url in Self::extract_endpoints(base, block.as_str()) {
                    if seen.insert(url.clone()) {
                        out.push(url);
                    }
                }
            }
        }
        out
    }
}

impl Default for JsEndpointExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Tester for JsEndpointExtractor {
    fn clone_box(&self) -> Box<dyn Tester> {
        Box::new(self.clone())
    }

    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let base_url =
                Url::parse(url).map_err(|_| anyhow::anyhow!("Failed to parse URL: {}", url))?;

            if !worth_fetching(&base_url) {
                return Ok(Vec::new());
            }
            // The cap is on requests actually made, so it is checked after the
            // free extension test and before the request.
            if !self.try_reserve_fetch() {
                return Ok(Vec::new());
            }

            let client = self.client().await?;
            let mut last_error = None;

            for attempt in 0..=self.retries {
                if let Some(limiter) = &self.rate_limiter {
                    limiter.acquire().await;
                }
                match client.get(url).send().await {
                    Ok(response) => {
                        // An error page's inline script is the site's chrome,
                        // not something this URL revealed.
                        if !response.status().is_success() {
                            return Ok(Vec::new());
                        }
                        let kind = classify(response.headers(), &base_url);
                        if kind == BodyKind::Skip {
                            return Ok(Vec::new());
                        }
                        let body = read_body_capped(response, MAX_BODY_BYTES).await?;
                        return Ok(match kind {
                            BodyKind::Script => Self::extract_endpoints(&base_url, &body),
                            BodyKind::Html => Self::extract_inline_endpoints(&base_url, &body),
                            BodyKind::Skip => Vec::new(),
                        });
                    }
                    Err(e) => {
                        last_error = Some(e);
                        if attempt < self.retries {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                    }
                }
            }

            Err(anyhow::anyhow!(
                "Failed to extract JS endpoints from {}: {:?}",
                url,
                last_error
            ))
        })
    }

    fn with_timeout(&mut self, seconds: u64) {
        self.timeout = seconds;
    }

    fn with_retries(&mut self, count: u32) {
        self.retries = count;
    }

    fn with_random_agent(&mut self, enabled: bool) {
        self.random_agent = enabled;
    }

    fn with_insecure(&mut self, enabled: bool) {
        self.insecure = enabled;
    }

    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://app.example.com/static/js/main.3f2a1b.js").unwrap()
    }

    fn extract(script: &str) -> Vec<String> {
        JsEndpointExtractor::extract_endpoints(&base(), script)
    }

    // ---- what must be found ------------------------------------------------

    #[test]
    fn test_quoted_paths_and_full_urls_are_found() {
        let script = r#"
            const a = "/api/v2/users";
            const b = '/graphql';
            const c = "https://api.example.com/v1/orders?limit=10";
            const d = '//cdn.example.com/assets/app.js';
        "#;
        assert_eq!(
            extract(script),
            vec![
                "https://app.example.com/api/v2/users",
                "https://app.example.com/graphql",
                "https://api.example.com/v1/orders?limit=10",
                "https://cdn.example.com/assets/app.js",
            ]
        );
    }

    #[test]
    fn test_request_call_arguments_are_found() {
        let script = r#"
            fetch("/api/session", {method:"POST"});
            axios.get('/api/me');
            axios.post("/api/login", body);
            axios({url: "/api/config"});
            $.ajax("/legacy/endpoint.php");
            xhr.open("GET", "/api/xhr");
            fetch("/");
            fetch("users");
        "#;
        let got = extract(script);
        for expected in [
            "https://app.example.com/api/session",
            "https://app.example.com/api/me",
            "https://app.example.com/api/login",
            "https://app.example.com/api/config",
            "https://app.example.com/legacy/endpoint.php",
            "https://app.example.com/api/xhr",
            // Request-call arguments bypass the "too short" and "bare
            // relative" rules: the call proves it is a URL.
            "https://app.example.com/",
            "https://app.example.com/users",
        ] {
            assert!(got.contains(&expected.to_string()), "{expected} in {got:?}");
        }
    }

    #[test]
    fn test_concatenation_suffixes_are_dropped_but_base_plus_path_is_kept() {
        let script = r#"
            a = "/users/" + encodeURIComponent(id) + "/avatar";
            b = baseUrl + "/_matrix/identity/v2";
            c = dv() + "/createStream";
            fetch(e + "/api/via-fetch");
            d = "unrelated"; e2 = x + "/api/after-gap";
        "#;
        assert_eq!(
            extract(script),
            vec![
                "https://app.example.com/api/via-fetch",
                "https://app.example.com/users/",
                "https://app.example.com/_matrix/identity/v2",
                "https://app.example.com/createStream",
                "https://app.example.com/api/after-gap",
            ]
        );
    }

    #[test]
    fn test_template_literal_static_prefix_is_kept() {
        let script = "const u = `/api/users/${id}/posts`; fetch(`${base}/x`);";
        let got = extract(script);
        assert_eq!(got, vec!["https://app.example.com/api/users/"]);
    }

    #[test]
    fn test_minified_code_yields_its_endpoints() {
        // A realistic minified fragment: no whitespace, chained calls,
        // interleaved noise.
        let script = r#"var e=n(7);e.get("/api/v1/profile").then(function(t){return fetch("/api/v1/profile/"+t.id+"/avatar",{headers:{"Content-Type":"application/json"}})}),o.post('/api/v1/logout'),r.open("POST","/upload"),i=`/api/v1/search?q=${q}`;"#;
        assert_eq!(
            extract(script),
            // Request-call arguments come first, then the remaining literals
            // in source order. `"/avatar"` is a concatenation suffix and is
            // dropped.
            vec![
                "https://app.example.com/api/v1/profile/",
                "https://app.example.com/upload",
                "https://app.example.com/api/v1/profile",
                "https://app.example.com/api/v1/logout",
                "https://app.example.com/api/v1/search?q=",
            ]
        );
    }

    #[test]
    fn test_bare_relative_paths_need_evidence() {
        // Three segments, a query, or a file-like tail are enough evidence;
        // two bare words are not (they are package paths and locale tags).
        let script = r#"
            a = "api/v1/users";
            b = "api/users?page=1";
            c = "assets/config.json";
            d = "api/users";
            e = "react/jsx-runtime";
            f = "lodash/fp";
            g = "en/US";
        "#;
        assert_eq!(
            extract(script),
            vec![
                "https://app.example.com/api/v1/users",
                "https://app.example.com/api/users?page=1",
                "https://app.example.com/assets/config.json",
            ]
        );
    }

    #[test]
    fn test_inline_scripts_are_mined_but_markup_is_not() {
        let html = r#"
            <html><head>
              <link href="/site.css" rel="stylesheet">
              <script src="/vendor.js"></script>
              <SCRIPT type="text/javascript">
                window.__API__ = "/api/bootstrap";
                fetch('/api/flags');
              </SCRIPT>
            </head><body><a href="/about">about</a></body></html>
        "#;
        let base = Url::parse("https://app.example.com/index.html").unwrap();
        assert_eq!(
            JsEndpointExtractor::extract_inline_endpoints(&base, html),
            vec![
                "https://app.example.com/api/flags",
                "https://app.example.com/api/bootstrap",
            ]
        );
    }

    // ---- what must NOT be found --------------------------------------------

    #[test]
    fn test_mime_types_are_not_endpoints() {
        let script = r#"
            h.set("Content-Type", "application/json");
            accept("image/png"); accept("text/plain"); accept("*/*");
            m = "application/x-www-form-urlencoded";
            v = "video/mp4"; f = "font/woff2"; t = "text/event-stream";
        "#;
        assert!(extract(script).is_empty(), "{:?}", extract(script));
    }

    #[test]
    fn test_short_fragments_and_comment_pieces_are_dropped() {
        let script = r#"
            p.split("/"); s = "//"; t = "/*"; u = "*/"; v = "//*";
            w = "/x"; x = "./"; y = "../"; z = "../../";
            sm = "//# sourceMappingURL="; sm2 = "\n//# sourceMappingURL=main.js.map";
            ext = ".js"; ext2 = "/.png";
        "#;
        assert!(extract(script).is_empty(), "{:?}", extract(script));
    }

    #[test]
    fn test_css_values_and_number_ratios_are_dropped() {
        let script = r#"
            font: "12px/1.5"; ar = "16/9"; half = "1/2"; big = "100%/auto";
            date = "MM/DD/YYYY"; d2 = "dd/mm/yy"; d3 = "HH/mm";
        "#;
        assert!(extract(script).is_empty(), "{:?}", extract(script));
    }

    #[test]
    fn test_base64_and_data_uris_are_dropped() {
        let script = r#"
            img = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk";
            raw = "/iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk";
            b = "AAAA/BBBB+CCCC=";
            k = "/api/v1/things/AbCdEf1234567890AbCdEf1234567890";
        "#;
        assert!(extract(script).is_empty(), "{:?}", extract(script));
    }

    #[test]
    fn test_hex_hashes_and_hashed_filenames_survive_the_base64_rule() {
        let script = r#"
            a = "/static/3f2a1bc4d5e6f7a8b9c0d1e2f3a4b5c6/chunk.js";
            b = "/assets/vendor.8f3a9c1b.js";
        "#;
        assert_eq!(
            extract(script),
            vec![
                "https://app.example.com/static/3f2a1bc4d5e6f7a8b9c0d1e2f3a4b5c6/chunk.js",
                "https://app.example.com/assets/vendor.8f3a9c1b.js",
            ]
        );
    }

    #[test]
    fn test_regex_sources_and_tag_fragments_do_not_match() {
        let script = r#"
            r1 = /^\/api\/(\d+)$/; r2 = "/^\\/api/(\\d+)"; r3 = "/[a-z]+/i";
            h = "</div>"; h2 = "<br/>"; h3 = "/>";
            o = "{/}"; t = `/${a}`;
        "#;
        assert!(extract(script).is_empty(), "{:?}", extract(script));
    }

    #[test]
    fn test_xml_namespaces_and_package_specifiers_are_dropped() {
        let script = r#"
            ns = "http://www.w3.org/2000/svg"; xl = "http://www.w3.org/1999/xlink";
            sch = "https://schema.org/Person";
            imp = "@babel/runtime/helpers/x.js"; nm = "node_modules/react/index.js";
            lib = "lib/esm/index.js";
            wp1 = "./src/vector/init.tsx";
            wp2 = "../../node_modules/.pnpm/katex@0.18.4/node_modules/katex/dist/katex.css";
        "#;
        assert!(extract(script).is_empty(), "{:?}", extract(script));
    }

    #[test]
    fn test_realistic_bundle_fixture_high_signal_low_noise() {
        // A composite of shapes lifted from real webpack/vite output. The
        // point is the ratio: every endpoint comes out, nothing else does.
        let script = r#"!function(){"use strict";var e={"Content-Type":"application/json",Accept:"*/*"};function t(t){return fetch("/api/v3/accounts/"+t,{headers:e})}var n=`/api/v3/accounts/${a}/tokens`,r="image/svg+xml",o="http://www.w3.org/2000/svg",i=".js",a="/",s="//",c="12px/1.5",l="MM/DD/YYYY",u="data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==",d="react/jsx-runtime",p=/\/api\/(\d+)/,f="/static/media/logo.6ce24c58.svg",h="https://api.example.com/v3/health";axios.post("/api/v3/login",{u:1});window.location="/dashboard";new WebSocket("wss://ws.example.com/socket");x.open("PUT","/api/v3/settings")}();
//# sourceMappingURL=main.3f2a1b.js.map"#;
        let got = extract(script);
        let expected = [
            "https://app.example.com/api/v3/login",
            "https://app.example.com/api/v3/settings",
            "https://app.example.com/api/v3/accounts/",
            "https://app.example.com/static/media/logo.6ce24c58.svg",
            "https://api.example.com/v3/health",
            "https://app.example.com/dashboard",
        ];
        for e in expected {
            assert!(got.contains(&e.to_string()), "missing {e} in {got:?}");
        }
        assert_eq!(got.len(), expected.len(), "unexpected extras in {got:?}");
    }

    // ---- resolution ----------------------------------------------------------

    #[test]
    fn test_relative_paths_resolve_against_the_origin_not_the_bundle_dir() {
        // `/api/x` is origin-relative by definition. `x/y/z` and a request
        // call's `./x` resolve against the (unknown) page URL at runtime; the
        // bundle directory `/static/js/` is the one place they are certainly
        // not, so the origin root is used instead.
        let script = r#"a = "/api/x"; fetch("./api/y"); axios.get("../api/z"); d = "api/v1/w";"#;
        assert_eq!(
            extract(script),
            vec![
                "https://app.example.com/api/y",
                "https://app.example.com/api/z",
                "https://app.example.com/api/x",
                "https://app.example.com/api/v1/w",
            ]
        );
    }

    #[test]
    fn test_module_imports_resolve_against_the_module_and_specifiers_are_dropped() {
        // `import("./chunk.js")` is a URL the browser fetches relative to the
        // importing module. `require("./utils")` and browserify's
        // `{"./utils":32}` maps are not URLs at all.
        let script = r#"
            import("./chunk-ab12.js"); const l = "../assets/logo.svg";
            var n = e("./zlib/deflate"), i = e("../compressions"), u = e("./utils");
            }, {"./utils/common":41, "./zlib/deflate":46, "./ArrayReader":17}]
        "#;
        assert_eq!(
            extract(script),
            vec![
                "https://app.example.com/static/js/chunk-ab12.js",
                "https://app.example.com/static/assets/logo.svg",
            ]
        );
    }

    #[test]
    fn test_strings_must_close_with_their_own_quote() {
        // From a real bundle: `.replace(/\\'/g,"'")` used to yield `'/g,"`.
        let script = r#"e.replace(/\'/g,"'").replace(/"/g, '"'); x = "/real'quote"; y.replace(/"/g,"&quot;");"#;
        assert!(extract(script).is_empty(), "{:?}", extract(script));
    }

    #[test]
    fn test_protocol_relative_takes_the_scripts_scheme() {
        let http = Url::parse("http://plain.example.com/a.js").unwrap();
        let got = JsEndpointExtractor::extract_endpoints(&http, r#"x = "//cdn.example.com/a";"#);
        assert_eq!(got, vec!["http://cdn.example.com/a"]);
    }

    #[test]
    fn test_fragments_are_stripped_and_duplicates_collapsed() {
        let script = r#"a = "/app#/home"; b = "/app"; c = '/app'; d = "/app#/settings";"#;
        assert_eq!(extract(script), vec!["https://app.example.com/app"]);
    }

    // ---- classification --------------------------------------------------------

    #[test]
    fn test_classify_by_content_type_and_extension() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
        let with = |v: &'static str| {
            let mut h = HeaderMap::new();
            h.insert(CONTENT_TYPE, HeaderValue::from_static(v));
            h
        };
        let js = Url::parse("https://x.test/a.js").unwrap();
        let html = Url::parse("https://x.test/page").unwrap();
        let png = Url::parse("https://x.test/a.png").unwrap();

        assert_eq!(
            classify(&with("application/javascript"), &html),
            BodyKind::Script
        );
        assert_eq!(
            classify(&with("text/javascript; charset=utf-8"), &html),
            BodyKind::Script
        );
        assert_eq!(classify(&with("text/html"), &html), BodyKind::Html);
        // Misconfigured hosts: the extension rescues the body.
        assert_eq!(classify(&with("text/plain"), &js), BodyKind::Script);
        assert_eq!(
            classify(&with("application/octet-stream"), &js),
            BodyKind::Script
        );
        assert_eq!(classify(&with("text/plain"), &html), BodyKind::Skip);
        // An explicit unrelated type wins over the extension.
        assert_eq!(classify(&with("image/png"), &js), BodyKind::Skip);
        assert_eq!(classify(&with("application/json"), &html), BodyKind::Skip);
        // No header: extension, else assume markup.
        assert_eq!(classify(&HeaderMap::new(), &js), BodyKind::Script);
        assert_eq!(classify(&HeaderMap::new(), &html), BodyKind::Html);

        assert!(worth_fetching(&js));
        assert!(worth_fetching(&html));
        assert!(!worth_fetching(&png));
        assert!(!worth_fetching(
            &Url::parse("https://x.test/a.js.map").unwrap()
        ));
    }

    // ---- HTTP path ----------------------------------------------------------------

    #[tokio::test]
    async fn test_fetched_script_yields_endpoints() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/app.js")
            .with_status(200)
            .with_header("content-type", "application/javascript")
            .with_body(r#"fetch("/api/one");var x="/api/two","image/png";"#)
            .create_async()
            .await;

        let extractor = JsEndpointExtractor::new();
        let got = extractor
            .test_url(&format!("{}/app.js", server.url()))
            .await
            .unwrap();
        let b = server.url();
        assert_eq!(got, vec![format!("{b}/api/one"), format!("{b}/api/two")]);
    }

    #[tokio::test]
    async fn test_non_script_extensions_are_never_requested() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/logo.png")
            .with_status(200)
            .expect(0)
            .create_async()
            .await;
        let extractor = JsEndpointExtractor::new();
        let got = extractor
            .test_url(&format!("{}/logo.png", server.url()))
            .await
            .unwrap();
        assert!(got.is_empty());
        assert_eq!(extractor.fetched(), 0);
        m.assert();
    }

    #[tokio::test]
    async fn test_error_responses_and_unrelated_types_are_skipped() {
        let mut server = mockito::Server::new_async().await;
        let _e = server
            .mock("GET", "/gone.js")
            .with_status(404)
            .with_header("content-type", "text/html")
            .with_body(r#"<script>fetch("/api/from-error-page")</script>"#)
            .create_async()
            .await;
        let _j = server
            .mock("GET", "/data")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"url":"/api/from-json"}"#)
            .create_async()
            .await;
        let extractor = JsEndpointExtractor::new();
        for path in ["/gone.js", "/data"] {
            let got = extractor
                .test_url(&format!("{}{path}", server.url()))
                .await
                .unwrap();
            assert!(got.is_empty(), "{path}: {got:?}");
        }
    }

    #[tokio::test]
    async fn test_html_pages_only_yield_inline_script_endpoints() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/index.html")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<a href="/about">x</a><script>window.api="/api/inline";</script>"#)
            .create_async()
            .await;
        let extractor = JsEndpointExtractor::new();
        let got = extractor
            .test_url(&format!("{}/index.html", server.url()))
            .await
            .unwrap();
        assert_eq!(got, vec![format!("{}/api/inline", server.url())]);
    }

    #[tokio::test]
    async fn test_fetch_cap_is_global_across_clones() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", mockito::Matcher::Regex(r"^/\d+\.js$".into()))
            .with_status(200)
            .with_header("content-type", "application/javascript")
            .with_body(r#"fetch("/api/x")"#)
            .expect(2)
            .create_async()
            .await;

        let mut extractor = JsEndpointExtractor::new();
        extractor.with_max_files(2);
        let clone = extractor.clone_box();

        let first = extractor
            .test_url(&format!("{}/1.js", server.url()))
            .await
            .unwrap();
        let second = clone
            .test_url(&format!("{}/2.js", server.url()))
            .await
            .unwrap();
        let third = extractor
            .test_url(&format!("{}/3.js", server.url()))
            .await
            .unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(third.is_empty(), "third fetch must be refused by the cap");
        assert_eq!(extractor.fetched(), 2);
        m.assert();
    }

    #[tokio::test]
    async fn test_zero_cap_means_unlimited() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Regex(r"^/\d+\.js$".into()))
            .with_status(200)
            .with_header("content-type", "application/javascript")
            .with_body(r#"fetch("/api/x")"#)
            .expect(3)
            .create_async()
            .await;
        let mut extractor = JsEndpointExtractor::new();
        extractor.with_max_files(0);
        for i in 1..=3 {
            let got = extractor
                .test_url(&format!("{}/{i}.js", server.url()))
                .await
                .unwrap();
            assert_eq!(got.len(), 1);
        }
    }

    #[test]
    fn test_settings_apply() {
        let mut e = JsEndpointExtractor::new();
        assert_eq!(e.max_files, JsEndpointExtractor::DEFAULT_MAX_FILES);
        e.with_timeout(7);
        e.with_retries(1);
        e.with_random_agent(true);
        e.with_insecure(true);
        e.with_proxy(Some("http://p:1".into()));
        e.with_proxy_auth(Some("u:p".into()));
        e.with_rate_limit(Some(2.0));
        assert_eq!(e.timeout, 7);
        assert_eq!(e.retries, 1);
        assert!(e.random_agent && e.insecure);
        assert_eq!(e.proxy.as_deref(), Some("http://p:1"));
        assert_eq!(e.proxy_auth.as_deref(), Some("u:p"));
        assert!(e.rate_limiter.is_some());
        e.with_rate_limit(None);
        assert!(e.rate_limiter.is_none());
    }
}
