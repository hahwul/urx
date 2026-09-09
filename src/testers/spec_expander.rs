//! `--expand-specs`: read the API descriptions the run already collected.
//!
//! An `--preset only-api` sweep finds `/swagger.json`, `/v3/api-docs` and
//! `/openapi.json` and then does nothing with them. `--extract-links` parses
//! HTML; `--extract-js-endpoints` deliberately drops `application/json`
//! bodies (see its `classify`); `--archive-body` runs the HTML parser over
//! whatever the archive returns. So the single most information-dense file on
//! the target is collected as one URL and never opened.
//!
//! One such document lists every documented route, which makes the exchange
//! rate far better than mining bundles: one request buys hundreds of
//! endpoints, already exact and already parameterised, instead of a megabyte
//! of minified strings that then has to be filtered down.
//!
//! Supported inputs, all as JSON: OpenAPI 3.x, Swagger 2.0, and GraphQL
//! introspection responses. YAML needs a parser urx does not currently
//! depend on; a YAML document is recognised and skipped rather than
//! mis-parsed.

use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::OnceCell;
use url::Url;

use super::shared::path_extension;
use super::Tester;
use crate::network::client::{read_body_capped, HttpClientConfig};
use crate::network::RateLimiter;

/// Cap on bytes read from one document before parsing.
///
/// The same guard, for the same reason, as the link and JS extractors: the
/// URL list comes from archives that will hand back whatever they recorded,
/// and no single response may be buffered whole. 10 MiB is far above any real
/// specification (a very large one is a few hundred KiB).
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Extensions a specification is served with. A URL carrying any *other*
/// extension is not one whatever its name says — `swagger-ui.html` is the
/// viewer, `swagger-ui-bundle.js` is its code — so the request is never made.
const SPEC_EXTENSIONS: &[&str] = &["json", "yaml", "yml"];

/// Path fragments that name an API description.
///
/// Matched as substrings of the lower-cased path so that every spelling in
/// the wild is covered by one entry: `openapi` catches `/openapi.json`,
/// `/.well-known/openapi` and `/v1/openapi.yaml`; `swagger` catches
/// `/swagger.json` and `/swagger/v1/swagger.json`; `api-docs` catches
/// springdoc's `/v2/api-docs` and `/v3/api-docs`.
///
/// This is the first of the two signals the tester uses. It is deliberately a
/// *name* test only — cheap, and made before any request — with
/// [`classify`] applying the second signal, `Content-Type`, to the response.
const SPEC_MARKERS: &[&str] = &[
    "swagger",
    "openapi",
    "api-docs",
    "apidocs",
    "api_docs",
    "graphql",
    "introspection",
];

/// File-name stems that name the *file* rather than the endpoint, so a
/// GraphQL schema saved under one is attributed to its directory.
const GENERIC_SCHEMA_STEMS: &[&str] = &["schema", "introspection", "index", "graphql-schema"];

/// Whether the URL is worth a request as a specification document.
///
/// Unlike the JS extractor — which fetches anything not obviously non-script,
/// because a bundle can be called anything — a specification is a named file,
/// and the name is the only cheap evidence there is. Requiring a marker keeps
/// the tester from re-requesting the entire result set to find three files.
fn looks_like_spec(url: &Url) -> bool {
    if let Some(ext) = path_extension(url) {
        if !SPEC_EXTENSIONS.contains(&ext.as_str()) {
            return false;
        }
    }
    let path = url.path().to_ascii_lowercase();
    SPEC_MARKERS.iter().any(|marker| path.contains(marker))
}

/// How a fetched body should be parsed, decided from `Content-Type` and the
/// URL's extension.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum BodyKind {
    Json,
    /// Recognised, but urx has no YAML parser; see the module docs.
    Yaml,
    /// Type and extension are both uninformative — decide from the body.
    Sniff,
    /// Nothing here can be a specification; don't read the body.
    Skip,
}

/// Classify a response by its `Content-Type`, falling back to the extension
/// and then to the body itself.
///
/// The second of the two target signals. `Content-Type` wins over the name
/// whenever it says something definite: a `swagger.json` served as `text/html`
/// is a login redirect or an error page, not the document. It loses only for
/// the deliberately vague types (`text/plain`, `application/octet-stream`)
/// that static hosts hand out for `.json` — the same one-directional rule
/// `js_endpoint_extractor::classify` applies to `.js`.
fn classify(headers: &reqwest::header::HeaderMap, url: &Url) -> BodyKind {
    let ext_kind = match path_extension(url).as_deref() {
        Some("json") => Some(BodyKind::Json),
        Some("yaml" | "yml") => Some(BodyKind::Yaml),
        _ => None,
    };

    let ct = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase());

    match ct.as_deref() {
        // `application/json`, and the registered specification types
        // `application/vnd.oai.openapi+json` / `application/schema+json`.
        Some(ct) if ct.contains("json") => BodyKind::Json,
        Some(ct) if ct.contains("yaml") || ct.contains("yml") => BodyKind::Yaml,
        // Swagger UI's own page, an error page, an SSO redirect: HTML is
        // never the document.
        Some(ct) if ct.contains("html") => BodyKind::Skip,
        // Loose types: trust the name, then the body. `/v3/api-docs` with no
        // extension and `text/plain` is a real springdoc configuration.
        Some(ct) if ct.contains("text/plain") || ct.contains("octet-stream") => {
            ext_kind.unwrap_or(BodyKind::Sniff)
        }
        None => ext_kind.unwrap_or(BodyKind::Sniff),
        // An explicit, unrelated type (image, css, javascript, ...) is
        // skipped even when the name says `swagger.json`: the server knows
        // what it served.
        Some(_) => BodyKind::Skip,
    }
}

/// Resolve [`BodyKind::Sniff`] from the body: a JSON document's first
/// non-whitespace byte is `{`. Anything else is YAML or not a document at
/// all, and either way there is nothing this tester can read.
fn sniff(body: &str) -> BodyKind {
    match body.trim_start().as_bytes().first() {
        Some(b'{') => BodyKind::Json,
        _ => BodyKind::Yaml,
    }
}

/// `url` without query or fragment and without a trailing slash: the string a
/// documented path is appended to.
fn base_prefix(url: &Url) -> String {
    let mut url = url.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.as_str().trim_end_matches('/').to_string()
}

/// `url`'s origin as a URL, i.e. everything below `/` discarded.
fn root_of(url: &Url) -> Url {
    let mut root = url.clone();
    root.set_path("/");
    root.set_query(None);
    root.set_fragment(None);
    root
}

/// `url`'s host with its port, as a Swagger 2.0 `host` value would spell it.
fn authority(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// Replace `{name}` placeholders in a server URL with the variable's default.
///
/// OpenAPI requires a `default` on every server variable, so a well-formed
/// document always resolves; an `enum`'s first value covers the documents
/// that omit it anyway.
fn substitute_variables(raw: &str, variables: Option<&Value>) -> String {
    let Some(vars) = variables.and_then(Value::as_object) else {
        return raw.to_string();
    };
    let mut out = raw.to_string();
    for (name, var) in vars {
        let value = var
            .get("default")
            .and_then(Value::as_str)
            .or_else(|| var.get("enum")?.as_array()?.first()?.as_str());
        if let Some(value) = value {
            out = out.replace(&format!("{{{name}}}"), value);
        }
    }
    out
}

/// The base URLs of an OpenAPI 3 `servers` array.
///
/// A relative `url` ("/api/v3", "v3") is relative to the document's own
/// location, which is what `spec_url.join` does. A server whose template
/// variables did not all resolve is dropped rather than emitted with a
/// percent-encoded `%7Bcustomer%7D` in the host: it is not a base, and the
/// caller falls back to the document's origin.
fn servers_bases(spec_url: &Url, servers: &Value) -> Vec<String> {
    let Some(list) = servers.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|server| {
            let raw = server.get("url").and_then(Value::as_str)?;
            let resolved = substitute_variables(raw, server.get("variables"));
            if resolved.contains('{') {
                return None;
            }
            let url = spec_url.join(&resolved).ok()?;
            matches!(url.scheme(), "http" | "https").then(|| base_prefix(&url))
        })
        .collect()
}

/// The base URLs of a Swagger 2.0 document: `schemes` × `host` + `basePath`.
///
/// Each part falls back to the corresponding part of the document's own URL
/// when omitted, which is what the specification says to do.
fn swagger2_bases(spec_url: &Url, doc: &Value) -> Vec<String> {
    let host = doc
        .get("host")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| authority(spec_url));
    // `host` is an authority, never a URL: a value with a scheme or a path in
    // it is malformed and would build a nonsense base.
    if host.is_empty() || host.contains('/') {
        return Vec::new();
    }
    let base_path = doc.get("basePath").and_then(Value::as_str).unwrap_or("");
    let declared: Vec<&str> = doc
        .get("schemes")
        .and_then(Value::as_array)
        .map(|s| s.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let schemes = if declared.is_empty() {
        vec![spec_url.scheme()]
    } else {
        declared
    };
    schemes
        .into_iter()
        // `schemes` also admits `ws`/`wss`. urx collects URLs something can
        // request over HTTP; a WebSocket endpoint is not one.
        .filter(|scheme| matches!(*scheme, "http" | "https"))
        .filter_map(|scheme| Url::parse(&format!("{scheme}://{host}{base_path}")).ok())
        .map(|url| base_prefix(&url))
        .collect()
}

/// Every base the document's paths hang off.
fn spec_bases(spec_url: &Url, doc: &Value) -> Vec<String> {
    // A document carrying both forms is malformed; `servers` (OpenAPI 3, the
    // newer form) wins.
    if let Some(servers) = doc.get("servers") {
        let bases = servers_bases(spec_url, servers);
        if !bases.is_empty() {
            return bases;
        }
    }
    let bases = swagger2_bases(spec_url, doc);
    if !bases.is_empty() {
        return bases;
    }
    // "If the servers property is not provided, or is an empty array, the
    // default value would be a Server Object with a url value of /" — and a
    // relative server URL resolves against the document's own location.
    vec![base_prefix(&root_of(spec_url))]
}

/// `base` + `path`, textually.
///
/// Deliberately *not* `Url::join`: a documented path carries its template
/// parameters (`/users/{id}`), and `url::Url` percent-encodes `{` and `}` in
/// a path, which would turn every parameter into `%7Bid%7D`. What makes the
/// output worth reading is that it is the route as the document writes it, so
/// the URL is assembled as a string. (`--normalize-url`, if the user asks for
/// it, re-parses and encodes it downstream — that is the user's choice.)
fn join_path(base: &str, path: &str) -> String {
    format!("{base}{path}")
}

/// Expand an OpenAPI 3.x / Swagger 2.0 document into one URL per documented
/// path per base.
fn openapi_urls(spec_url: &Url, doc: &Value) -> Vec<String> {
    let Some(paths) = doc.get("paths").and_then(Value::as_object) else {
        return Vec::new();
    };
    let document_bases = spec_bases(spec_url, doc);
    let mut out = Vec::new();
    for (path, item) in paths {
        // `x-`-prefixed extensions and `$ref` live in the same object as the
        // routes; a real path always starts with `/`.
        if !path.starts_with('/') {
            continue;
        }
        // OpenAPI 3 lets a path item override the document's servers, and
        // gateways that front several backends actually use it.
        let bases = item
            .get("servers")
            .map(|servers| servers_bases(spec_url, servers))
            .filter(|bases| !bases.is_empty())
            .unwrap_or_else(|| document_bases.clone());
        for base in &bases {
            out.push(join_path(base, path));
        }
    }
    out
}

/// The `__schema` object of an introspection response, wherever it sits.
///
/// A raw `POST /graphql` response wraps it as `{"data": {"__schema": …}}`;
/// tools that save a schema to a file usually unwrap it first.
fn introspection_schema(doc: &Value) -> Option<&Value> {
    doc.get("data")
        .and_then(|data| data.get("__schema"))
        .or_else(|| doc.get("__schema"))
}

/// The root operation types, and the GraphQL document keyword each one needs
/// when written into a `?query=`.
const ROOT_TYPES: &[(&str, &str)] = &[
    ("queryType", ""),
    ("mutationType", "mutation"),
    ("subscriptionType", "subscription"),
];

/// Where the schema's operations are served.
///
/// In the common case the introspection response *was* the endpoint's
/// response, so the fetched URL is the endpoint minus its query. When it is a
/// saved copy instead, the file name is resolved away: `/graphql.json` is the
/// `/graphql` endpoint, and `/graphql/schema.json` names its directory.
fn graphql_endpoint(spec_url: &Url) -> String {
    let mut url = spec_url.clone();
    url.set_query(None);
    url.set_fragment(None);
    if path_extension(&url).is_none() {
        return url.to_string();
    }
    let path = url.path().to_string();
    let cut = path.rfind('/').map_or(0, |i| i + 1);
    let stem = path[cut..].rsplit_once('.').map_or("", |(stem, _)| stem);
    let replacement = if GENERIC_SCHEMA_STEMS.contains(&stem.to_ascii_lowercase().as_str()) {
        ""
    } else {
        stem
    };
    let mut new_path = format!("{}{replacement}", &path[..cut]);
    if new_path.len() > 1 {
        new_path = new_path.trim_end_matches('/').to_string();
    }
    url.set_path(&new_path);
    url.to_string()
}

/// Reduce an introspection response to one URL per documented operation.
///
/// A GraphQL API has a single endpoint, so the *URL* surface of its schema is
/// not a set of paths — it is the set of operations that one endpoint
/// accepts. GraphQL-over-GET spells an operation as `?query=…`, which is both
/// a request a server may genuinely answer and, in a result list, a legible
/// name for what the schema exposes. Braces stay unencoded for the same
/// reason OpenAPI's `{id}` does.
fn graphql_operations(spec_url: &Url, schema: &Value) -> Vec<String> {
    let endpoint = graphql_endpoint(spec_url);
    let types = schema.get("types").and_then(Value::as_array);
    let mut out = Vec::new();
    for (root, keyword) in ROOT_TYPES {
        let Some(type_name) = schema
            .get(root)
            .and_then(|root| root.get("name"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let fields = types.and_then(|types| {
            types
                .iter()
                .find(|t| t.get("name").and_then(Value::as_str) == Some(type_name))
                .and_then(|t| t.get("fields"))
                .and_then(Value::as_array)
        });
        for field in fields.into_iter().flatten() {
            let Some(name) = field.get("name").and_then(Value::as_str) else {
                continue;
            };
            // Introspection's own meta-fields (`__schema`, `__type`) are on
            // every schema and say nothing about this one.
            if name.starts_with("__") || name.is_empty() {
                continue;
            }
            out.push(format!("{endpoint}?query={keyword}{{{name}}}"));
        }
    }
    out
}

/// Keep the first occurrence of each URL, in order.
fn dedup(urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    urls.into_iter()
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

/// API specification expander: fetches the specification documents among the
/// collected URLs and expands the endpoints they describe.
#[derive(Clone)]
pub struct SpecExpander {
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    /// Upper bound on documents fetched across the whole run; `0` is
    /// unlimited.
    max_files: usize,
    /// Fetches performed so far, shared across `clone_box` clones so the cap
    /// is global rather than per worker.
    fetched: Arc<AtomicUsize>,
    /// `--rate-limit`, shared across clones for the same reason: this tester
    /// re-requests URLs from the target itself.
    rate_limiter: Option<RateLimiter>,
    /// One HTTP client, built lazily and shared across clones.
    client: Arc<OnceCell<Client>>,
}

impl SpecExpander {
    /// Default cap on specification documents fetched per run.
    ///
    /// An order of magnitude below the JS extractor's 500 because the
    /// candidate set is smaller by construction: [`looks_like_spec`] admits a
    /// handful of named files per host, not every bundle in the list.
    pub const DEFAULT_MAX_FILES: usize = 50;

    pub fn new() -> Self {
        SpecExpander {
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

    /// Cap the number of documents fetched; `0` means no cap.
    pub fn with_max_files(&mut self, max: usize) {
        self.max_files = max;
    }

    /// Pace requests at `requests_per_sec`, as `--rate-limit` does for
    /// providers.
    pub fn with_rate_limit(&mut self, requests_per_sec: Option<f32>) {
        self.rate_limiter = RateLimiter::from_rate(requests_per_sec);
    }

    /// Documents fetched so far.
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

    /// Expand one parsed document into the absolute URLs it describes,
    /// deduplicated in first-seen order.
    ///
    /// `spec_url` is where the document was fetched from: relative `servers`
    /// entries resolve against it, it supplies the parts a Swagger 2.0
    /// document omits, and it is the fallback base when the document names no
    /// server at all.
    ///
    /// The document's own shape decides how it is read — a `__schema` is an
    /// introspection response, a `paths` object is OpenAPI or Swagger — so a
    /// document served under an unexpected name still parses, and one that is
    /// neither yields nothing rather than guesswork.
    pub fn expand(spec_url: &Url, doc: &Value) -> Vec<String> {
        if let Some(schema) = introspection_schema(doc) {
            return dedup(graphql_operations(spec_url, schema));
        }
        dedup(openapi_urls(spec_url, doc))
    }
}

impl Default for SpecExpander {
    fn default() -> Self {
        Self::new()
    }
}

impl Tester for SpecExpander {
    fn clone_box(&self) -> Box<dyn Tester> {
        Box::new(self.clone())
    }

    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let spec_url =
                Url::parse(url).map_err(|_| anyhow::anyhow!("Failed to parse URL: {}", url))?;

            if !looks_like_spec(&spec_url) {
                return Ok(Vec::new());
            }
            // The cap is on requests actually made, so it is checked after the
            // free name test and before the request.
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
                        // An error page under `/swagger.json` describes
                        // nothing; only a served document does.
                        if !response.status().is_success() {
                            return Ok(Vec::new());
                        }
                        let kind = classify(response.headers(), &spec_url);
                        if kind == BodyKind::Skip {
                            return Ok(Vec::new());
                        }
                        let body = read_body_capped(response, MAX_BODY_BYTES).await?;
                        let kind = if kind == BodyKind::Sniff {
                            sniff(&body)
                        } else {
                            kind
                        };
                        if kind != BodyKind::Json {
                            return Ok(Vec::new());
                        }
                        // A document truncated by the size cap, or one that is
                        // not JSON after all, is not a failure of the run —
                        // there is simply nothing to expand.
                        return Ok(match serde_json::from_str::<Value>(&body) {
                            Ok(doc) => Self::expand(&spec_url, &doc),
                            Err(_) => Vec::new(),
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
                "Failed to expand API specification {}: {:?}",
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
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
    use serde_json::json;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn expand(spec_url: &str, doc: Value) -> Vec<String> {
        SpecExpander::expand(&url(spec_url), &doc)
    }

    // ---- target selection --------------------------------------------------

    #[test]
    fn test_only_specification_shaped_urls_are_fetched() {
        for candidate in [
            "https://x.com/swagger.json",
            "https://x.com/openapi.yaml",
            "https://x.com/OpenAPI.YML",
            "https://x.com/v2/api-docs",
            "https://x.com/v3/api-docs",
            "https://x.com/swagger/v1/swagger.json",
            "https://x.com/.well-known/openapi",
            "https://x.com/graphql",
            "https://x.com/graphql/schema.json",
            "https://x.com/api/introspection.json",
        ] {
            assert!(looks_like_spec(&url(candidate)), "{candidate}");
        }

        for candidate in [
            // Right name, wrong file: the Swagger UI viewer and its bundle.
            "https://x.com/swagger-ui.html",
            "https://x.com/swagger-ui-bundle.js",
            "https://x.com/openapi.css",
            // Right extension, no marker: every other JSON on the host.
            "https://x.com/config.json",
            "https://x.com/api/users",
            "https://x.com/",
        ] {
            assert!(!looks_like_spec(&url(candidate)), "{candidate}");
        }
    }

    #[test]
    fn test_classify_uses_content_type_then_name_then_body() {
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
            headers
        };
        let json = url("https://x.com/swagger.json");
        let yaml = url("https://x.com/openapi.yaml");
        let bare = url("https://x.com/v3/api-docs");

        assert_eq!(classify(&with("application/json"), &bare), BodyKind::Json);
        assert_eq!(
            classify(&with("application/vnd.oai.openapi+json;version=3.0"), &bare),
            BodyKind::Json
        );
        assert_eq!(classify(&with("text/yaml"), &bare), BodyKind::Yaml);
        // A definite, unrelated type beats the name — the server knows what it
        // served.
        assert_eq!(classify(&with("text/html"), &json), BodyKind::Skip);
        assert_eq!(classify(&with("image/png"), &json), BodyKind::Skip);
        assert_eq!(
            classify(&with("application/javascript"), &json),
            BodyKind::Skip
        );
        // Vague types and missing headers fall back to the name, then the body.
        assert_eq!(classify(&with("text/plain"), &json), BodyKind::Json);
        assert_eq!(
            classify(&with("application/octet-stream"), &yaml),
            BodyKind::Yaml
        );
        assert_eq!(classify(&with("text/plain"), &bare), BodyKind::Sniff);
        assert_eq!(classify(&HeaderMap::new(), &json), BodyKind::Json);
        assert_eq!(classify(&HeaderMap::new(), &bare), BodyKind::Sniff);

        assert_eq!(sniff("  \n{\"openapi\":\"3.0.0\"}"), BodyKind::Json);
        assert_eq!(sniff("openapi: 3.0.0\n"), BodyKind::Yaml);
    }

    // ---- OpenAPI 3.x -------------------------------------------------------

    #[test]
    fn test_openapi3_servers_and_paths_become_absolute_urls() {
        let doc = json!({
            "openapi": "3.0.3",
            "servers": [{"url": "https://api.example.com/v3"}],
            "paths": {
                "/users": {"get": {}},
                "/users/{id}": {"get": {}},
                "/orders/{orderId}/items": {"post": {}},
            }
        });
        let mut got = expand("https://example.com/openapi.json", doc);
        got.sort();
        assert_eq!(
            got,
            vec![
                // The path template is emitted exactly as the document writes
                // it — no `%7Bid%7D`.
                "https://api.example.com/v3/orders/{orderId}/items",
                "https://api.example.com/v3/users",
                "https://api.example.com/v3/users/{id}",
            ]
        );
    }

    #[test]
    fn test_openapi3_relative_and_multiple_servers() {
        let doc = json!({
            "openapi": "3.1.0",
            "servers": [
                {"url": "/api/v2"},
                {"url": "https://staging.example.com/api/v2"},
                // Not something a scanner can request.
                {"url": "ws://example.com/socket"},
            ],
            "paths": {"/ping": {}}
        });
        let got = expand("https://example.com/docs/openapi.json", doc);
        assert_eq!(
            got,
            vec![
                "https://example.com/api/v2/ping",
                "https://staging.example.com/api/v2/ping",
            ]
        );
    }

    #[test]
    fn test_openapi3_server_variables_resolve_from_defaults() {
        let doc = json!({
            "openapi": "3.0.0",
            "servers": [{
                "url": "https://{region}.example.com/{stage}",
                "variables": {
                    "region": {"default": "eu"},
                    "stage": {"enum": ["v1", "v2"]},
                }
            }],
            "paths": {"/health": {}}
        });
        assert_eq!(
            expand("https://example.com/openapi.json", doc),
            vec!["https://eu.example.com/v1/health"]
        );
    }

    #[test]
    fn test_unresolvable_server_falls_back_to_the_documents_own_origin() {
        // No `variables`, so `{customer}` never resolves and the server is not
        // a base at all. Emitting the routes against the host that served the
        // document beats emitting nothing.
        let doc = json!({
            "openapi": "3.0.0",
            "servers": [{"url": "https://{customer}.example.com/v1"}],
            "paths": {"/admin": {}}
        });
        assert_eq!(
            expand("https://docs.example.com/spec/openapi.json", doc),
            vec!["https://docs.example.com/admin"]
        );
    }

    #[test]
    fn test_path_level_servers_override_the_documents() {
        let doc = json!({
            "openapi": "3.0.0",
            "servers": [{"url": "https://api.example.com"}],
            "paths": {
                "/users": {"get": {}},
                "/legacy": {"servers": [{"url": "https://old.example.com/v1"}], "get": {}},
            }
        });
        let mut got = expand("https://example.com/openapi.json", doc);
        got.sort();
        assert_eq!(
            got,
            vec![
                "https://api.example.com/users",
                "https://old.example.com/v1/legacy",
            ]
        );
    }

    // ---- Swagger 2.0 -------------------------------------------------------

    #[test]
    fn test_swagger2_host_base_path_and_schemes() {
        let doc = json!({
            "swagger": "2.0",
            "host": "api.example.com:8443",
            "basePath": "/v2",
            "schemes": ["https", "http", "wss"],
            "paths": {"/pet/{petId}": {"get": {}}}
        });
        let mut got = expand("https://example.com/v2/api-docs", doc);
        got.sort();
        assert_eq!(
            got,
            vec![
                "http://api.example.com:8443/v2/pet/{petId}",
                "https://api.example.com:8443/v2/pet/{petId}",
            ]
        );
    }

    #[test]
    fn test_swagger2_omitted_parts_come_from_the_documents_url() {
        let doc = json!({
            "swagger": "2.0",
            "paths": {"/status": {}}
        });
        assert_eq!(
            expand("http://intranet.example.com:8080/v2/api-docs", doc),
            vec!["http://intranet.example.com:8080/status"]
        );
    }

    // ---- shape and noise ---------------------------------------------------

    #[test]
    fn test_non_path_keys_and_documents_without_paths_yield_nothing() {
        let doc = json!({
            "openapi": "3.0.0",
            "paths": {
                "x-internal-note": {},
                "$ref": "other.json",
                "/real": {},
            }
        });
        assert_eq!(
            expand("https://example.com/openapi.json", doc),
            vec!["https://example.com/real"]
        );

        // Not a specification at all: a plain configuration document served
        // under a specification-shaped name.
        assert_eq!(
            expand(
                "https://example.com/openapi.json",
                json!({"name": "app", "version": 3})
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_duplicate_urls_are_emitted_once() {
        // Two servers that resolve to the same base.
        let doc = json!({
            "openapi": "3.0.0",
            "servers": [{"url": "https://api.example.com/"}, {"url": "https://api.example.com"}],
            "paths": {"/one": {}}
        });
        assert_eq!(
            expand("https://example.com/openapi.json", doc),
            vec!["https://api.example.com/one"]
        );
    }

    // ---- GraphQL introspection --------------------------------------------

    fn introspection() -> Value {
        json!({
            "data": {"__schema": {
                "queryType": {"name": "Query"},
                "mutationType": {"name": "Mutation"},
                "types": [
                    {"name": "Query", "fields": [
                        {"name": "me"}, {"name": "users"}, {"name": "__schema"},
                    ]},
                    {"name": "Mutation", "fields": [{"name": "deleteUser"}]},
                    {"name": "User", "fields": [{"name": "email"}]},
                ]
            }}
        })
    }

    #[test]
    fn test_graphql_introspection_becomes_one_url_per_operation() {
        assert_eq!(
            expand(
                "https://example.com/graphql?query=%7B__schema%7D",
                introspection()
            ),
            vec![
                "https://example.com/graphql?query={me}",
                "https://example.com/graphql?query={users}",
                "https://example.com/graphql?query=mutation{deleteUser}",
            ]
        );
    }

    #[test]
    fn test_graphql_schema_saved_as_a_file_is_attributed_to_its_endpoint() {
        // Both the unwrapped shape and the file-name resolution.
        let schema = introspection()["data"].clone();
        for (spec_url, endpoint) in [
            (
                "https://example.com/graphql/schema.json",
                "https://example.com/graphql",
            ),
            (
                "https://example.com/graphql.json",
                "https://example.com/graphql",
            ),
            (
                "https://example.com/api/graphql",
                "https://example.com/api/graphql",
            ),
        ] {
            let got = expand(spec_url, schema.clone());
            assert_eq!(got[0], format!("{endpoint}?query={{me}}"), "{spec_url}");
        }
    }

    // ---- transport ---------------------------------------------------------

    #[tokio::test]
    async fn test_fetches_and_expands_over_http() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/swagger.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"swagger":"2.0","basePath":"/v1","paths":{"/a":{},"/b/{id}":{}}}"#)
            .create_async()
            .await;
        let expander = SpecExpander::new();
        let mut got = expander
            .test_url(&format!("{}/swagger.json", server.url()))
            .await
            .unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                format!("{}/v1/a", server.url()),
                format!("{}/v1/b/{{id}}", server.url()),
            ]
        );
        assert_eq!(expander.fetched(), 1);
    }

    #[tokio::test]
    async fn test_unrelated_urls_error_pages_and_html_cost_nothing_or_yield_nothing() {
        let mut server = mockito::Server::new_async().await;
        // Never requested: no specification marker in the name.
        let untouched = server
            .mock("GET", "/app.json")
            .expect(0)
            .create_async()
            .await;
        let _missing = server
            .mock("GET", "/v3/api-docs")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(r#"{"paths":{"/from-an-error-page":{}}}"#)
            .create_async()
            .await;
        let _ui = server
            .mock("GET", "/swagger-ui/swagger.json")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>login</body></html>")
            .create_async()
            .await;
        let _garbage = server
            .mock("GET", "/openapi.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{not json at all")
            .create_async()
            .await;

        let expander = SpecExpander::new();
        for path in [
            "/app.json",
            "/v3/api-docs",
            "/swagger-ui/swagger.json",
            "/openapi.json",
        ] {
            let got = expander
                .test_url(&format!("{}{path}", server.url()))
                .await
                .unwrap();
            assert!(got.is_empty(), "{path}: {got:?}");
        }
        // Only the three specification-shaped URLs were requested.
        assert_eq!(expander.fetched(), 3);
        untouched.assert();
    }

    #[tokio::test]
    async fn test_untyped_body_is_sniffed_and_yaml_is_skipped() {
        let mut server = mockito::Server::new_async().await;
        let _json = server
            .mock("GET", "/v3/api-docs")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body(r#"{"openapi":"3.0.0","paths":{"/sniffed":{}}}"#)
            .create_async()
            .await;
        let _yaml = server
            .mock("GET", "/openapi.yaml")
            .with_status(200)
            .with_header("content-type", "text/yaml")
            .with_body("openapi: 3.0.0\npaths:\n  /nope: {}\n")
            .create_async()
            .await;

        let expander = SpecExpander::new();
        assert_eq!(
            expander
                .test_url(&format!("{}/v3/api-docs", server.url()))
                .await
                .unwrap(),
            vec![format!("{}/sniffed", server.url())]
        );
        assert!(expander
            .test_url(&format!("{}/openapi.yaml", server.url()))
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_fetch_cap_is_global_across_clones() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/\d+/swagger\.json$".into()),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"openapi":"3.0.0","paths":{"/a":{}}}"#)
            .expect(2)
            .create_async()
            .await;

        let mut expander = SpecExpander::new();
        expander.with_max_files(2);
        let clone = expander.clone_box();

        for (n, tester) in [(1, &expander as &dyn Tester), (2, clone.as_ref())] {
            let got = tester
                .test_url(&format!("{}/{n}/swagger.json", server.url()))
                .await
                .unwrap();
            assert_eq!(got.len(), 1);
        }
        let third = expander
            .test_url(&format!("{}/3/swagger.json", server.url()))
            .await
            .unwrap();
        assert!(third.is_empty(), "third fetch must be refused by the cap");
        assert_eq!(expander.fetched(), 2);
        m.assert();
    }

    #[test]
    fn test_settings_apply() {
        let mut e = SpecExpander::new();
        assert_eq!(e.max_files, SpecExpander::DEFAULT_MAX_FILES);
        e.with_timeout(7);
        e.with_retries(1);
        e.with_random_agent(true);
        e.with_insecure(true);
        e.with_proxy(Some("http://p:1".into()));
        e.with_proxy_auth(Some("u:p".into()));
        e.with_rate_limit(Some(2.0));
        e.with_max_files(0);
        assert_eq!(e.timeout, 7);
        assert_eq!(e.retries, 1);
        assert!(e.random_agent && e.insecure);
        assert_eq!(e.proxy.as_deref(), Some("http://p:1"));
        assert_eq!(e.proxy_auth.as_deref(), Some("u:p"));
        assert!(e.rate_limiter.is_some());
        assert_eq!(e.max_files, 0);
        e.with_rate_limit(None);
        assert!(e.rate_limiter.is_none());
    }
}
