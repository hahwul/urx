use std::collections::{HashMap, HashSet};
use url::Url;

/// Utility for transforming and manipulating URL collections
///
/// Provides methods for merging, filtering, and extracting parts of URLs.
pub struct UrlTransformer {
    merge_endpoint: bool,
    show_only_host: bool,
    show_only_path: bool,
    show_only_param: bool,
    normalize_url: bool,
    dedup_similar: bool,
    // --- output-views ---
    param_view: ParamView,
}

/// A view that replaces the URL list with an inventory derived from it.
///
/// Every variant needs the *whole* result set — a union of parameter names, or
/// one representative per parameter signature — which is why the streaming path
/// rejects them the same way it rejects `--dedup-similar`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ParamView {
    /// No inventory view; the URL list is emitted as usual.
    #[default]
    None,
    /// `--params`: every query parameter name seen, once each.
    Names,
    /// `--params-by-endpoint`: one line per endpoint with the union of the
    /// parameter names observed on it.
    ByEndpoint,
    /// `--fuzz-placeholder VALUE`: one ready-to-fuzz URL per parameter
    /// signature, every query value replaced by `VALUE`.
    Fuzz(String),
}

/// What [`UrlTransformer::transform_with_stats`] collapsed, so `--verbose` can
/// say how much the run actually shrank instead of leaving the user to diff
/// two line counts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TransformStats {
    /// URLs dropped by `--dedup-similar` as near-duplicates of a kept one.
    pub similar_collapsed: usize,
}

impl UrlTransformer {
    /// Creates a new URL transformer with default settings
    pub fn new() -> Self {
        UrlTransformer {
            merge_endpoint: false,
            show_only_host: false,
            show_only_path: false,
            show_only_param: false,
            normalize_url: false,
            dedup_similar: false,
            param_view: ParamView::None,
        }
    }

    /// Enables or disables merging of endpoints with the same path but different parameters
    pub fn with_merge_endpoint(&mut self, merge: bool) -> &mut Self {
        self.merge_endpoint = merge;
        self
    }

    /// When enabled, shows only the hostname part of URLs
    pub fn with_show_only_host(&mut self, show: bool) -> &mut Self {
        self.show_only_host = show;
        self
    }

    /// When enabled, shows only the path part of URLs
    pub fn with_show_only_path(&mut self, show: bool) -> &mut Self {
        self.show_only_path = show;
        self
    }

    /// When enabled, shows only the query parameters of URLs
    pub fn with_show_only_param(&mut self, show: bool) -> &mut Self {
        self.show_only_param = show;
        self
    }

    /// When enabled, normalizes URLs for better deduplication
    /// Sorts query parameters alphabetically and normalizes paths
    pub fn with_normalize_url(&mut self, normalize: bool) -> &mut Self {
        self.normalize_url = normalize;
        self
    }

    /// When enabled, collapses URLs that differ only in variable-looking path
    /// segments (ids, UUIDs, hashes, dates) and query *values*
    pub fn with_dedup_similar(&mut self, dedup: bool) -> &mut Self {
        self.dedup_similar = dedup;
        self
    }

    // --- output-views ---
    /// Selects the parameter inventory view that replaces the URL list, if any.
    pub fn with_param_view(&mut self, view: ParamView) -> &mut Self {
        self.param_view = view;
        self
    }

    /// Transforms a list of URLs according to the configured settings, and
    /// reports what the cross-URL stages removed.
    ///
    /// Every stage is independent and applies only when its own flag is set, so
    /// `--normalize-url`, `--merge-endpoint`, and `--dedup-similar` compose in
    /// any combination. The order is fixed and increasing in aggressiveness:
    /// normalising first gives merging a canonical query to work with, and
    /// merging first gives the similarity pass one URL per endpoint instead of
    /// several.
    pub fn transform_with_stats(&self, urls: Vec<String>) -> (Vec<String>, TransformStats) {
        let mut transformed_urls = urls;
        let mut stats = TransformStats::default();

        // Normalize URLs if requested (should happen before merging)
        if self.normalize_url {
            transformed_urls = self.normalize_urls(transformed_urls);
        }

        // Merge endpoints if requested
        if self.merge_endpoint {
            transformed_urls = self.merge_endpoints(transformed_urls);
        }

        // Collapse near-duplicates if requested
        if self.dedup_similar {
            let before = transformed_urls.len();
            transformed_urls = dedup_similar_urls(transformed_urls);
            stats.similar_collapsed = before.saturating_sub(transformed_urls.len());
        }

        // Extract URL parts if any show_only option is enabled
        if self.show_only_host || self.show_only_path || self.show_only_param {
            transformed_urls = self.extract_url_parts(transformed_urls);
        }

        // --- output-views ---
        // An inventory view replaces the list outright. It runs last so it sees
        // whatever --normalize-url / --merge-endpoint / --dedup-similar left,
        // and it is mutually exclusive with the show_only_* views (enforced by
        // clap) so the two can never both rewrite the same list.
        if self.param_view != ParamView::None {
            transformed_urls = apply_param_view(&self.param_view, &transformed_urls);
        }

        (transformed_urls, stats)
    }

    /// Transform a single URL the way [`UrlTransformer::transform`] would,
    /// minus `merge_endpoint` (which is inherently cross-URL and so has no
    /// single-URL equivalent). Returns `None` when the URL carries nothing for
    /// the configured `show_only_*` view — e.g. `--show-only-param` on a URL
    /// with no query.
    ///
    /// Used by streaming output, where each URL must be decided on arrival.
    pub fn transform_one(&self, url: &str) -> Option<String> {
        let normalized = if self.normalize_url {
            self.normalize_one(url)
        } else {
            url.to_string()
        };

        if self.show_only_host || self.show_only_path || self.show_only_param {
            self.extract_parts_one(&normalized)
        } else {
            Some(normalized)
        }
    }

    /// Normalise one URL: drop a trailing slash and sort query parameters.
    /// Unparseable input is passed through untouched.
    fn normalize_one(&self, url_str: &str) -> String {
        match Url::parse(url_str) {
            Ok(mut url) => {
                // Normalize the path - remove trailing slash if it's not just "/"
                let path = url.path().to_string();
                if path.len() > 1 {
                    if let Some(normalized_path) = path.strip_suffix('/') {
                        url.set_path(normalized_path);
                    }
                }

                // Normalize query parameters by sorting them. We sort the *raw*
                // `key=value` tokens without decoding, so this stays a lossless
                // reordering: a bare `?foo` is not rewritten to `?foo=`, and a
                // literal '+' is not turned into '%20' (query_pairs() decodes
                // both, which silently mutates the URL the archive recorded).
                let sorted_query: Option<String> = url.query().map(|query| {
                    let mut pairs: Vec<&str> = query.split('&').filter(|s| !s.is_empty()).collect();
                    pairs.sort_unstable();
                    pairs.join("&")
                });
                if let Some(query) = sorted_query {
                    url.set_query(None);
                    if !query.is_empty() {
                        url.set_query(Some(&query));
                    }
                }

                url.to_string()
            }
            // If URL can't be parsed, keep it as is
            Err(_) => url_str.to_string(),
        }
    }

    fn normalize_urls(&self, urls: Vec<String>) -> Vec<String> {
        let mut normalized_urls: Vec<String> = urls.iter().map(|u| self.normalize_one(u)).collect();

        // Remove duplicates that might have been created during normalization
        normalized_urls.sort();
        normalized_urls.dedup();

        normalized_urls
    }

    fn merge_endpoints(&self, urls: Vec<String>) -> Vec<String> {
        let mut path_groups: HashMap<String, Vec<String>> = HashMap::new();

        for url_str in urls {
            if let Ok(url) = Url::parse(&url_str) {
                // Key on the full origin plus the path. Keying on host+path alone
                // merged endpoints that are not the same endpoint at all:
                // `http://host/api?a=1` and `https://host/api?b=2` collapsed into a
                // single URL that carried one scheme and both origins' parameters,
                // and `host:8080/api` was folded into `host/api` the same way.
                let key = format!(
                    "{}://{}{}{}",
                    url.scheme(),
                    url.host_str().unwrap_or(""),
                    url.port().map(|p| format!(":{p}")).unwrap_or_default(),
                    url.path()
                );

                path_groups.entry(key).or_default().push(url_str);
            } else {
                // If URL can't be parsed, keep it as is
                path_groups
                    .entry(url_str.clone())
                    .or_default()
                    .push(url_str);
            }
        }

        // Now create merged URLs
        let mut merged_urls = Vec::new();

        for (_, group_urls) in path_groups {
            if group_urls.len() == 1 {
                // If only one URL with this path, use it as is
                merged_urls.push(group_urls[0].clone());
            } else {
                // Merge parameters from all URLs with the same path
                if let Ok(base_url) = Url::parse(&group_urls[0]) {
                    let mut merged_url = base_url.clone();
                    let mut all_params: Vec<String> = Vec::new();
                    let mut seen_params = HashSet::new();

                    // Collect parameters from all URLs. We copy the *raw*
                    // `key=value` tokens rather than decoding via
                    // `query_pairs()`: decoding and re-joining silently rewrites
                    // the URL the archive recorded — a value containing an
                    // encoded `&` or `=` (`?next=%2Fa%3Fb%3D1`) would come back
                    // out as extra parameters, `+` would turn into a space, and
                    // a bare `?foo` would gain an `=`. Merging must only add
                    // parameters, never alter them.
                    for url_str in &group_urls {
                        if let Ok(url) = Url::parse(url_str) {
                            for pair in url
                                .query()
                                .unwrap_or("")
                                .split('&')
                                .filter(|s| !s.is_empty())
                            {
                                if seen_params.insert(pair.to_string()) {
                                    all_params.push(pair.to_string());
                                }
                            }
                        }
                    }

                    // Set merged parameters
                    if !all_params.is_empty() {
                        let query_string = all_params.join("&");

                        // Clear existing query and set merged query
                        merged_url.set_query(None);
                        merged_url.set_query(Some(&query_string));
                    }

                    merged_urls.push(merged_url.to_string());
                } else {
                    // If URL can't be parsed, use the first one
                    merged_urls.push(group_urls[0].clone());
                }
            }
        }

        // Sort again for consistency
        merged_urls.sort();
        merged_urls
    }

    /// Reduce one URL to the configured `show_only_*` view. `None` means the
    /// URL has nothing to show for that view (no host, a bare `/` path, or no
    /// query) and is therefore dropped.
    fn extract_parts_one(&self, url_str: &str) -> Option<String> {
        match Url::parse(url_str) {
            Ok(url) => {
                if self.show_only_host {
                    // An empty host carries nothing to show; emitting it would
                    // put a blank line in the output.
                    url.host_str().filter(|h| !h.is_empty()).map(String::from)
                } else if self.show_only_path {
                    if url.path() != "/" {
                        Some(url.path().to_string())
                    } else {
                        None
                    }
                } else if self.show_only_param {
                    // `https://example.com/x?` parses with a query of `Some("")`
                    // — a URL with no parameters at all. Treated as a value it
                    // emitted an empty line into the result (and, on the
                    // streaming path, reserved "" in the dedup set).
                    url.query().filter(|q| !q.is_empty()).map(String::from)
                } else {
                    Some(url_str.to_string())
                }
            }
            // If URL can't be parsed, keep it as is
            Err(_) => Some(url_str.to_string()),
        }
    }

    fn extract_url_parts(&self, urls: Vec<String>) -> Vec<String> {
        let mut extracted_parts: Vec<String> = urls
            .iter()
            .filter_map(|u| self.extract_parts_one(u))
            .collect();

        // Remove duplicates that might have been created during transformation
        extracted_parts.sort();
        extracted_parts.dedup();

        extracted_parts
    }
}

/// Written in place of a path segment that looks like a variable identifier.
///
/// One placeholder for every class of identifier on purpose: `/post/12` and
/// `/post/550e8400-e29b-41d4-a716-446655440000` are the same endpoint wearing
/// different data, and distinguishing "numeric id" from "UUID" would keep both.
const VARIABLE_SEGMENT: &str = "{id}";

/// Collapse URLs that are the same endpoint carrying different data.
///
/// `/post/1`, `/post/2` and `/post/99999` are one endpoint; an archive happily
/// reports a hundred thousand of them, which is what turns a real run into an
/// unreadable wall of output. URLs are grouped by [`similarity_key`] and one
/// representative survives per group.
///
/// The representative is the lexicographically smallest URL in its group, not
/// the first one seen: the input arrives from a `HashSet` in arbitrary order,
/// so anything else would produce a different answer on every run.
pub fn dedup_similar_urls(urls: Vec<String>) -> Vec<String> {
    let mut representatives: HashMap<String, String> = HashMap::new();

    for url in urls {
        let key = similarity_key(&url);
        match representatives.get_mut(&key) {
            Some(kept) if url < *kept => *kept = url,
            Some(_) => {}
            None => {
                representatives.insert(key, url);
            }
        }
    }

    let mut result: Vec<String> = representatives.into_values().collect();
    result.sort();
    result
}

/// The shape of a URL with its variable parts erased.
///
/// Two URLs share a key when they differ only in identifier-looking path
/// segments and in query *values* — the parameter *names* are part of the key,
/// because `?id=1` and `?debug=1` are not the same request. The fragment is
/// left out entirely: it never reaches the server.
fn similarity_key(url_str: &str) -> String {
    let Ok(url) = Url::parse(url_str) else {
        // Nothing to take apart. Its own text is the only honest key, which
        // keeps unparseable input in the output untouched.
        return url_str.to_string();
    };

    let mut key = String::with_capacity(url_str.len());
    key.push_str(url.scheme());
    key.push_str("://");
    key.push_str(url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        key.push(':');
        key.push_str(&port.to_string());
    }

    // `split('/')` on "/a/b" yields ["", "a", "b"], so re-joining restores the
    // leading slash and preserves a trailing empty segment ("/a/" stays
    // distinct from "/a").
    let segments: Vec<String> = url.path().split('/').map(normalize_segment).collect();
    key.push_str(&segments.join("/"));

    if let Some(query) = url.query() {
        let mut names: Vec<&str> = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| pair.split('=').next().unwrap_or(""))
            .collect();
        names.sort_unstable();
        names.dedup();
        key.push('?');
        key.push_str(&names.join("&"));
    }

    key
}

/// Replace one path segment with [`VARIABLE_SEGMENT`] when it looks like data
/// rather than part of the route.
fn normalize_segment(segment: &str) -> String {
    if segment.is_empty() {
        return String::new();
    }
    if is_variable_segment(segment) {
        return VARIABLE_SEGMENT.to_string();
    }
    // `article-1234.html`: the extension is part of the endpoint's shape while
    // the stem is the identifier, so only the stem is erased.
    if let Some((stem, extension)) = segment.rsplit_once('.') {
        if is_variable_segment(stem) {
            return format!("{VARIABLE_SEGMENT}.{extension}");
        }
    }
    segment.to_string()
}

/// Does this path segment look like an identifier rather than a route name?
///
/// Every rule requires the *whole* segment to match, which is what keeps
/// genuinely meaningful segments that merely contain digits — `v1`, `v2`,
/// `2fa`, `utf-8` — out of the placeholder.
fn is_variable_segment(segment: &str) -> bool {
    is_numeric(segment)
        || is_uuid(segment)
        || is_hex_hash(segment)
        || is_date_like(segment)
        || is_opaque_token(segment)
}

fn is_numeric(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit())
}

/// 8-4-4-4-12 hex, the canonical UUID spelling.
fn is_uuid(segment: &str) -> bool {
    let groups: Vec<&str> = segment.split('-').collect();
    groups.len() == 5
        && [8usize, 4, 4, 4, 12]
            .iter()
            .zip(&groups)
            .all(|(len, group)| group.len() == *len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// MD5 / SHA-1 / SHA-256 digests, the hashes that actually show up in paths.
/// Lengths are exact so an ordinary 32-letter word cannot qualify — it would
/// have to be hex-only as well.
fn is_hex_hash(segment: &str) -> bool {
    matches!(segment.len(), 32 | 40 | 64) && segment.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `2024-01-02`, `2024_01`, `2024-1-2`. Bare `2024` and `20240102` are already
/// numeric; this rule exists for the separated spellings.
fn is_date_like(segment: &str) -> bool {
    let parts: Vec<&str> = segment.split(['-', '_']).collect();
    if !(2..=3).contains(&parts.len()) {
        return false;
    }
    let Some(year) = parts[0].parse::<u32>().ok().filter(|_| parts[0].len() == 4) else {
        return false;
    };
    if !(1000..=2999).contains(&year) {
        return false;
    }
    parts[1..]
        .iter()
        .all(|part| (1..=2).contains(&part.len()) && is_numeric(part))
}

/// A long opaque token: session ids, signed blobs, base64url payloads.
///
/// Deliberately conservative. A slug like `how-to-write-a-good-changelog` is
/// the same length and the same alphabet, so mixed case *and* a digit are both
/// required — slugs are lower-case by convention, and tokens are not.
fn is_opaque_token(segment: &str) -> bool {
    if segment.len() < 20 {
        return false;
    }
    let alphabet = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'=' | b'+');
    segment.bytes().all(alphabet)
        && segment.bytes().any(|b| b.is_ascii_digit())
        && segment.bytes().any(|b| b.is_ascii_lowercase())
        && segment.bytes().any(|b| b.is_ascii_uppercase())
}

// --- output-views ---

/// Separates an endpoint from its parameter list in `--params-by-endpoint`
/// output. A single space, so `cut -d' ' -f1` gets the endpoint back: a URL
/// never contains a literal space (it would be percent-encoded).
const ENDPOINT_PARAM_SEPARATOR: char = ' ';

/// Render the inventory view `view` asks for. Always sorted and deduplicated,
/// so two runs over the same result set produce identical bytes.
fn apply_param_view(view: &ParamView, urls: &[String]) -> Vec<String> {
    match view {
        ParamView::None => urls.to_vec(),
        ParamView::Names => param_names(urls),
        ParamView::ByEndpoint => params_by_endpoint(urls),
        ParamView::Fuzz(placeholder) => fuzz_templates(urls, placeholder),
    }
}

/// The query parameter *names* of one URL, in the order they appear, without
/// repeats.
///
/// The raw `key=value` tokens are split by hand rather than through
/// `query_pairs()`, for the same reason the rest of this module does: decoding
/// rewrites what the archive actually recorded, and a parameter name is
/// precisely the thing that must survive verbatim to be worth fuzzing.
fn query_param_names(url: &Url) -> Vec<&str> {
    let mut names = Vec::new();
    for pair in url.query().unwrap_or("").split('&') {
        let name = pair.split('=').next().unwrap_or("");
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Every query parameter name in the result set, once each, sorted.
///
/// The inventory a tester actually wants first: "what does this target take?",
/// answered across the whole collection rather than one URL at a time.
pub fn param_names(urls: &[String]) -> Vec<String> {
    let mut names: HashSet<String> = HashSet::new();
    for url_str in urls {
        let Ok(url) = Url::parse(url_str) else {
            continue;
        };
        names.extend(query_param_names(&url).into_iter().map(String::from));
    }
    let mut out: Vec<String> = names.into_iter().collect();
    out.sort();
    out
}

/// One line per endpoint: the endpoint, a space, and the union of the parameter
/// names seen on it, comma-separated.
///
/// Path segments go through [`normalize_segment`], the same machinery
/// `--dedup-similar` uses, so `/post/1?a=1` and `/post/2?b=2` report as the one
/// endpoint `/post/{id}` taking `a,b` — which is the whole point of an
/// inventory. Endpoints with no parameters contribute nothing.
///
/// The endpoint is spelled out in full (`https://host/path`, not `/path`): urx
/// routinely scans several hosts in one run (`--subs`, `--domain-list`), and a
/// bare path would silently merge `a.example.com/search` with
/// `b.example.com/search` into a single line that is true of neither.
pub fn params_by_endpoint(urls: &[String]) -> Vec<String> {
    let mut by_endpoint: HashMap<String, Vec<String>> = HashMap::new();

    for url_str in urls {
        let Ok(url) = Url::parse(url_str) else {
            continue;
        };
        let names = query_param_names(&url);
        if names.is_empty() {
            continue;
        }
        let entry = by_endpoint.entry(endpoint_key(&url)).or_default();
        for name in names {
            entry.push(name.to_string());
        }
    }

    let mut out: Vec<String> = by_endpoint
        .into_iter()
        .map(|(endpoint, mut names)| {
            names.sort();
            names.dedup();
            format!("{endpoint}{ENDPOINT_PARAM_SEPARATOR}{}", names.join(","))
        })
        .collect();
    out.sort();
    out
}

/// `scheme://host[:port]` plus the path with identifier-looking segments
/// replaced — the endpoint identity [`params_by_endpoint`] groups on. The query
/// and fragment are deliberately absent: they are the varying part.
fn endpoint_key(url: &Url) -> String {
    let mut key = String::new();
    key.push_str(url.scheme());
    key.push_str("://");
    key.push_str(url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        key.push(':');
        key.push_str(&port.to_string());
    }
    let segments: Vec<String> = url.path().split('/').map(normalize_segment).collect();
    key.push_str(&segments.join("/"));
    key
}

/// One ready-to-fuzz URL per parameter signature, every query value replaced by
/// `placeholder`.
///
/// Grouping is [`similarity_key`] — the same signature `--dedup-similar`
/// collapses on, which is exactly "same endpoint shape, same parameter names" —
/// and the survivor is again the lexicographically smallest URL of its group,
/// so runs are reproducible. URLs with no parameters carry nothing to fuzz and
/// drop out, the way `--show-only-param` drops them.
///
/// The representative keeps its *real* path (no `{id}` placeholders), because
/// the output is meant to be fed straight to ffuf or dalfox and has to be a URL
/// the server will actually route.
pub fn fuzz_templates(urls: &[String], placeholder: &str) -> Vec<String> {
    let mut representatives: HashMap<String, &String> = HashMap::new();

    for url in urls {
        // A URL with no query has no signature worth fuzzing; skipping it here
        // also keeps it from winning a group it would then render as nothing.
        if !Url::parse(url).is_ok_and(|u| !query_param_names(&u).is_empty()) {
            continue;
        }
        let key = similarity_key(url);
        match representatives.get_mut(&key) {
            Some(kept) if url < *kept => *kept = url,
            Some(_) => {}
            None => {
                representatives.insert(key, url);
            }
        }
    }

    let mut out: Vec<String> = representatives
        .into_values()
        .filter_map(|url| fuzz_one(url, placeholder))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Rewrite every query value of one URL to `placeholder`. Repeated names
/// collapse to one; the fragment is dropped (it never reaches the server).
fn fuzz_one(url_str: &str, placeholder: &str) -> Option<String> {
    let mut url = Url::parse(url_str).ok()?;
    let names = query_param_names(&url);
    if names.is_empty() {
        return None;
    }
    let query = names
        .iter()
        .map(|name| format!("{name}={placeholder}"))
        .collect::<Vec<_>>()
        .join("&");
    url.set_fragment(None);
    url.set_query(Some(&query));
    Some(url.to_string())
}

/// The wordlist terms of one URL: its path segments and its query parameter
/// names, with anything that looks like data left out.
///
/// "Looks like data" is [`is_variable_segment`], the same test `--dedup-similar`
/// uses — a wordlist full of `4711`, UUIDs and session tokens is worse than no
/// wordlist, since every one of them is a word that exists on exactly one
/// target. A segment whose *stem* is an identifier (`article-1234.html`) goes
/// too: `{id}.html` is not a word either.
///
/// Case is preserved. Path segments are case-sensitive on most origins, so
/// lower-casing `WebResource.axd` would produce a word that 404s everywhere it
/// is tried; keeping both `Admin` and `admin` when a target really shows both
/// is the useful answer for a wordlist.
pub fn wordlist_terms(url_str: &str) -> Vec<String> {
    let Ok(url) = Url::parse(url_str) else {
        return Vec::new();
    };

    let mut terms: Vec<String> = Vec::new();
    let mut push = |term: &str| {
        if !term.is_empty() && !terms.iter().any(|t| t == term) {
            terms.push(term.to_string());
        }
    };

    for segment in url.path().split('/') {
        if segment.is_empty() || normalize_segment(segment).contains(VARIABLE_SEGMENT) {
            continue;
        }
        push(segment);
    }
    for name in query_param_names(&url) {
        push(name);
    }

    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dedup(urls: &[&str]) -> Vec<String> {
        let mut transformer = UrlTransformer::new();
        transformer.with_dedup_similar(true);
        transformer
            .transform_with_stats(urls.iter().map(|s| s.to_string()).collect())
            .0
    }

    #[test]
    fn test_dedup_similar_collapses_numeric_ids() {
        let out = dedup(&[
            "https://example.com/post/1",
            "https://example.com/post/2",
            "https://example.com/post/99999",
        ]);
        // One endpoint, one line — and the survivor is the smallest URL of the
        // group, not whichever one happened to arrive first.
        assert_eq!(out, vec!["https://example.com/post/1".to_string()]);
    }

    #[test]
    fn test_dedup_similar_is_deterministic_regardless_of_input_order() {
        // The batch path feeds this from a HashSet, whose iteration order
        // changes between runs; a "first seen wins" rule would make the output
        // change with it.
        let forward = dedup(&[
            "https://example.com/post/10",
            "https://example.com/post/2",
            "https://example.com/post/300",
        ]);
        let backward = dedup(&[
            "https://example.com/post/300",
            "https://example.com/post/2",
            "https://example.com/post/10",
        ]);
        assert_eq!(forward, backward);
        assert_eq!(forward, vec!["https://example.com/post/10".to_string()]);
    }

    #[test]
    fn test_dedup_similar_collapses_short_numeric_segments() {
        // A one-digit id is still an id: `/p/1` and `/p/9` are one endpoint.
        let out = dedup(&[
            "https://example.com/p/1",
            "https://example.com/p/9",
            "https://example.com/p/1/edit",
        ]);
        assert_eq!(
            out,
            vec![
                "https://example.com/p/1".to_string(),
                "https://example.com/p/1/edit".to_string(),
            ]
        );
    }

    #[test]
    fn test_dedup_similar_keeps_meaningful_segments_that_contain_digits() {
        // `v1`/`v2` are route names, not identifiers, and neither is `2fa`.
        // Only a segment that is *entirely* identifier-shaped is collapsed.
        let urls = [
            "https://example.com/api/v1/users",
            "https://example.com/api/v2/users",
            "https://example.com/account/2fa/setup",
            "https://example.com/account/mfa/setup",
        ];
        assert_eq!(dedup(&urls).len(), urls.len());
    }

    #[test]
    fn test_dedup_similar_collapses_uuids_hashes_and_dates() {
        let out = dedup(&[
            "https://example.com/u/550e8400-e29b-41d4-a716-446655440000",
            "https://example.com/u/6ba7b810-9dad-11d1-80b4-00c04fd430c8",
        ]);
        assert_eq!(out.len(), 1, "{out:?}");

        // 32/40/64-char hex: md5, sha1, sha256.
        let out = dedup(&[
            "https://example.com/f/d41d8cd98f00b204e9800998ecf8427e",
            "https://example.com/f/5d41402abc4b2a76b9719d911017c592",
        ]);
        assert_eq!(out.len(), 1, "{out:?}");

        let out = dedup(&[
            "https://example.com/blog/2024-01-02/hello",
            "https://example.com/blog/2023-11-30/hello",
        ]);
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn test_dedup_similar_collapses_long_opaque_tokens() {
        let out = dedup(&[
            "https://example.com/s/Ab3xK9zQ7mN2pR5tV8wY",
            "https://example.com/s/Zq1wE4rT7yU0iO3pA6sD",
        ]);
        assert_eq!(out.len(), 1, "{out:?}");

        // A lower-case slug of the same length and alphabet is prose, not a
        // token, and must survive: mixed case and a digit are both required.
        let slugs = [
            "https://example.com/blog/how-to-secure-your-nginx-config",
            "https://example.com/blog/why-we-moved-off-kubernetes",
        ];
        assert_eq!(dedup(&slugs).len(), 2);
    }

    #[test]
    fn test_dedup_similar_normalizes_an_identifier_stem_but_keeps_the_extension() {
        let out = dedup(&[
            "https://example.com/article/1234.html",
            "https://example.com/article/5678.html",
        ]);
        assert_eq!(out.len(), 1, "{out:?}");

        // The extension is part of the endpoint's shape: `.html` and `.json`
        // are two different responses.
        let out = dedup(&[
            "https://example.com/article/1234.html",
            "https://example.com/article/1234.json",
        ]);
        assert_eq!(out.len(), 2, "{out:?}");
    }

    #[test]
    fn test_dedup_similar_groups_by_parameter_names_not_values() {
        let out = dedup(&[
            "https://example.com/search?q=cats&page=1",
            "https://example.com/search?q=dogs&page=7",
            "https://example.com/search?q=cats",
        ]);
        // `?q=&page=` is one shape and `?q=` is another: dropping a parameter
        // changes the request.
        assert_eq!(
            out,
            vec![
                "https://example.com/search?q=cats".to_string(),
                "https://example.com/search?q=cats&page=1".to_string(),
            ]
        );
    }

    #[test]
    fn test_dedup_similar_keeps_urls_without_a_query_distinct() {
        // No query at all is not the same endpoint as one with a query, and two
        // different static pages must never be folded together.
        let urls = [
            "https://example.com/about",
            "https://example.com/contact",
            "https://example.com/about?ref=nav",
        ];
        assert_eq!(dedup(&urls).len(), 3);
    }

    #[test]
    fn test_dedup_similar_separates_hosts_schemes_and_ports() {
        let urls = [
            "https://example.com/post/1",
            "http://example.com/post/1",
            "https://example.com:8443/post/1",
            "https://other.com/post/1",
        ];
        assert_eq!(dedup(&urls).len(), urls.len());
    }

    #[test]
    fn test_dedup_similar_keeps_trailing_slash_distinct_and_passes_junk_through() {
        let out = dedup(&[
            "https://example.com/a/1",
            "https://example.com/a/1/",
            "not a url",
            "also not a url",
        ]);
        assert_eq!(out.len(), 4, "{out:?}");
    }

    #[test]
    fn test_dedup_similar_reports_how_much_it_collapsed() {
        let mut transformer = UrlTransformer::new();
        transformer.with_dedup_similar(true);

        let (out, stats) = transformer.transform_with_stats(
            ["/1", "/2", "/3", "/about"]
                .iter()
                .map(|p| format!("https://example.com/post{p}"))
                .collect(),
        );
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(stats.similar_collapsed, 2);
    }

    #[test]
    fn test_dedup_similar_is_off_by_default() {
        let transformer = UrlTransformer::new();
        let urls: Vec<String> = ["https://example.com/post/1", "https://example.com/post/2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (out, stats) = transformer.transform_with_stats(urls.clone());
        assert_eq!(out, urls);
        assert_eq!(stats.similar_collapsed, 0);
    }

    #[test]
    fn test_dedup_similar_composes_with_normalize_and_merge() {
        // The three options are independent stages; each must still do its own
        // job when the others are on.
        let mut transformer = UrlTransformer::new();
        transformer
            .with_normalize_url(true)
            .with_merge_endpoint(true)
            .with_dedup_similar(true);

        let (out, stats) = transformer.transform_with_stats(
            [
                // Trailing slash + unsorted query: normalisation's job.
                "https://example.com/post/1/?b=2&a=1",
                // Same endpoint, different parameter: merging's job.
                "https://example.com/post/1?c=3",
                // Same shape, different id: the similarity pass's job.
                "https://example.com/post/2?a=1&b=2&c=3",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        );

        assert_eq!(
            out,
            vec!["https://example.com/post/1?a=1&b=2&c=3".to_string()]
        );
        assert_eq!(stats.similar_collapsed, 1);
    }

    #[test]
    fn test_dedup_similar_runs_before_the_show_only_views() {
        let mut transformer = UrlTransformer::new();
        transformer
            .with_dedup_similar(true)
            .with_show_only_path(true);

        let (out, _) = transformer.transform_with_stats(
            ["https://example.com/post/1", "https://example.com/post/2"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        assert_eq!(out, vec!["/post/1".to_string()]);
    }

    #[test]
    fn test_url_transformer_merge_endpoints() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/api?param1=value1".to_string(),
            "https://example.com/api?param2=value2".to_string(),
            "https://example.com/api?param3=value3".to_string(),
            "https://other.com/path".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert!(transformed.contains(
            &"https://example.com/api?param1=value1&param2=value2&param3=value3".to_string()
        ));
        assert!(transformed.contains(&"https://other.com/path".to_string()));
    }

    #[test]
    fn test_merge_endpoints_preserves_encoded_values() {
        // Regression: merging used to decode each pair with query_pairs() and
        // re-join the results raw, so an encoded '&' or '=' inside a value broke
        // out and became extra parameters — silently rewriting the URL the
        // archive actually recorded.
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/go?next=%2Fadmin%3Fdebug%3D1".to_string(),
            "https://example.com/go?ref=home".to_string(),
        ];

        let out = transformer.transform_with_stats(urls).0;
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(
            out[0],
            "https://example.com/go?next=%2Fadmin%3Fdebug%3D1&ref=home"
        );
    }

    #[test]
    fn test_merge_endpoints_preserves_plus_and_bare_params() {
        // '+' must not become a space, and a valueless `?foo` must not gain an
        // '=' — both were casualties of the decode/re-encode round trip.
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/s?q=a+b".to_string(),
            "https://example.com/s?debug".to_string(),
        ];

        let out = transformer.transform_with_stats(urls).0;
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0], "https://example.com/s?q=a+b&debug");
    }

    #[test]
    fn test_merge_endpoints_keeps_origins_apart() {
        // Regression: the group key was host+path, so a plain-HTTP endpoint and
        // its HTTPS counterpart (and a non-default port) merged into one URL —
        // inventing an endpoint that carried parameters never seen on that origin.
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "http://example.com/api?a=1".to_string(),
            "https://example.com/api?b=2".to_string(),
            "https://example.com:8443/api?c=3".to_string(),
        ];

        let out = transformer.transform_with_stats(urls).0;
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(
            out.contains(&"http://example.com/api?a=1".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com/api?b=2".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com:8443/api?c=3".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn test_merge_endpoints_still_merges_same_origin() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/api?a=1".to_string(),
            "https://example.com/api?b=2".to_string(),
        ];

        let out = transformer.transform_with_stats(urls).0;
        assert_eq!(out, vec!["https://example.com/api?a=1&b=2".to_string()]);
    }

    #[test]
    fn test_url_transformer_show_only_host() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_host(true);

        let urls = vec![
            "https://example.com/path1".to_string(),
            "https://example.com/path2".to_string(),
            "https://other.com/path".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 2); // Duplicates should be removed
        assert!(transformed.contains(&"example.com".to_string()));
        assert!(transformed.contains(&"other.com".to_string()));
    }

    #[test]
    fn test_url_transformer_show_only_path() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_path(true);

        let urls = vec![
            "https://example.com/path1".to_string(),
            "https://example.com/path2".to_string(),
            "https://other.com/path1".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 2); // Duplicates should be removed
        assert!(transformed.contains(&"/path1".to_string()));
        assert!(transformed.contains(&"/path2".to_string()));
    }

    #[test]
    fn test_url_transformer_show_only_param() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_param(true);

        let urls = vec![
            "https://example.com/api?param1=value1".to_string(),
            "https://example.com/api?param2=value2".to_string(),
            "https://other.com/api?param1=value1".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 2); // Duplicates should be removed
        assert!(transformed.contains(&"param1=value1".to_string()));
        assert!(transformed.contains(&"param2=value2".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_query_params() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api?b=2&a=1".to_string(),
            "https://example.com/api?a=1&b=2".to_string(),
            "https://example.com/path?z=3&y=2&x=1".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 2); // First two should be deduplicated
        assert!(transformed.contains(&"https://example.com/api?a=1&b=2".to_string()));
        assert!(transformed.contains(&"https://example.com/path?x=1&y=2&z=3".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_trailing_slashes() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api/".to_string(),
            "https://example.com/api".to_string(),
            "https://example.com/path/subpath/".to_string(),
            "https://example.com/path/subpath".to_string(),
            "https://example.com/".to_string(), // Root path should keep trailing slash
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 3); // Should deduplicate trailing slash variants
        assert!(transformed.contains(&"https://example.com/".to_string())); // Root keeps slash
        assert!(transformed.contains(&"https://example.com/api".to_string()));
        assert!(transformed.contains(&"https://example.com/path/subpath".to_string()));
        assert!(!transformed.contains(&"https://example.com/api/".to_string()));
        assert!(!transformed.contains(&"https://example.com/path/subpath/".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_complex() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api/?c=3&b=2&a=1".to_string(),
            "https://example.com/api?a=1&b=2&c=3".to_string(),
            "https://example.com/api/?a=1&c=3&b=2".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 1); // All should be normalized to the same URL
        assert!(transformed.contains(&"https://example.com/api?a=1&b=2&c=3".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_with_merge_endpoint() {
        let mut transformer = UrlTransformer::new();
        transformer
            .with_normalize_url(true)
            .with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/api/?param2=value2&param1=value1".to_string(),
            "https://example.com/api?param3=value3".to_string(),
            "https://example.com/api/?param1=value1&param2=value2".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        // After normalization, first and third URLs should be identical
        // Then merge_endpoint should combine all parameters
        assert_eq!(transformed.len(), 1);
        let result_url = &transformed[0];
        assert!(result_url.starts_with("https://example.com/api?"));
        assert!(result_url.contains("param1=value1"));
        assert!(result_url.contains("param2=value2"));
        assert!(result_url.contains("param3=value3"));
    }

    #[test]
    fn test_url_transformer_normalize_preserves_bare_param_and_plus() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/a?foo".to_string(), // bare param, no '='
            "https://example.com/b?q=a+b".to_string(), // literal '+'
            "https://example.com/c?b=2&a=1".to_string(), // still gets sorted
        ];

        let out = transformer.transform_with_stats(urls).0;
        // Bare param keeps no '='; '+' is not rewritten to '%20'; order sorted.
        assert!(
            out.contains(&"https://example.com/a?foo".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com/b?q=a+b".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com/c?a=1&b=2".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn test_url_transformer_normalize_invalid_urls() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "https://example.com/api?a=1&b=2".to_string(),
            "not-a-valid-url".to_string(),
            "https://example.com/api?b=2&a=1".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 2); // Valid URLs normalized, invalid kept as-is
        assert!(transformed.contains(&"https://example.com/api?a=1&b=2".to_string()));
        assert!(transformed.contains(&"not-a-valid-url".to_string()));
    }

    #[test]
    fn test_url_transformer_new() {
        let transformer = UrlTransformer::new();

        // Transform empty list should return empty list
        let urls: Vec<String> = vec![];
        let transformed = transformer.transform_with_stats(urls).0;
        assert!(transformed.is_empty());
    }

    #[test]
    fn test_url_transformer_no_options() {
        let transformer = UrlTransformer::new();

        let urls = vec![
            "https://example.com/path1".to_string(),
            "https://example.com/path2".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls.clone()).0;
        // Without any options, URLs should be returned as-is
        assert_eq!(transformed, urls);
    }

    #[test]
    fn test_url_transformer_show_only_path_root_path() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_path(true);

        let urls = vec![
            "https://example.com/".to_string(),
            "https://example.com/path".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        // Root path "/" should not be included
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"/path".to_string()));
    }

    #[test]
    fn test_show_only_param_skips_an_empty_query() {
        // Regression: `https://example.com/x?` parses with a query of
        // `Some("")`, which was emitted as a value — a blank line in the
        // output (and a blank entry in JSON/CSV).
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_param(true);

        let urls = vec![
            "https://example.com/x?".to_string(),
            "https://example.com/api?id=123".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed, vec!["id=123".to_string()]);
    }

    #[test]
    fn test_transform_one_skips_an_empty_query() {
        // The streaming path decides each URL on arrival and must agree.
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_param(true);

        assert_eq!(transformer.transform_one("https://example.com/x?"), None);
        assert_eq!(
            transformer.transform_one("https://example.com/api?id=123"),
            Some("id=123".to_string())
        );
    }

    #[test]
    fn test_url_transformer_show_only_param_no_params() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_param(true);

        let urls = vec![
            "https://example.com/path".to_string(),
            "https://example.com/api?id=123".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        // URL without params should not contribute to the result
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"id=123".to_string()));
    }

    #[test]
    fn test_url_transformer_merge_endpoints_single_url() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec!["https://example.com/api?param1=value1".to_string()];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"https://example.com/api?param1=value1".to_string()));
    }

    #[test]
    fn test_url_transformer_merge_endpoints_no_params() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "https://example.com/path".to_string(),
            "https://example.com/path".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"https://example.com/path".to_string()));
    }

    #[test]
    fn test_url_transformer_merge_endpoints_invalid_url() {
        let mut transformer = UrlTransformer::new();
        transformer.with_merge_endpoint(true);

        let urls = vec![
            "not-a-valid-url".to_string(),
            "another-invalid-url".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        // Invalid URLs should be kept as-is
        assert_eq!(transformed.len(), 2);
    }

    #[test]
    fn test_url_transformer_normalize_empty_query() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec!["https://example.com/path".to_string()];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 1);
        assert!(transformed.contains(&"https://example.com/path".to_string()));
    }

    #[test]
    fn test_url_transformer_chaining() {
        let mut transformer = UrlTransformer::new();
        transformer
            .with_merge_endpoint(true)
            .with_show_only_host(false)
            .with_show_only_path(false)
            .with_show_only_param(false)
            .with_normalize_url(true);

        let urls = vec![
            "https://example.com/api?b=2&a=1".to_string(),
            "https://example.com/api?a=1&b=2".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        assert_eq!(transformed.len(), 1);
    }

    #[test]
    fn test_url_transformer_show_only_host_invalid_url() {
        let mut transformer = UrlTransformer::new();
        transformer.with_show_only_host(true);

        let urls = vec![
            "https://example.com/path".to_string(),
            "not-a-valid-url".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;
        // Invalid URL should be kept as-is
        assert!(transformed.contains(&"example.com".to_string()));
        assert!(transformed.contains(&"not-a-valid-url".to_string()));
    }

    #[test]
    fn test_url_transformer_normalize_completely_invalid_inputs() {
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);

        let urls = vec![
            "plain-text".to_string(),
            "://start-with-colon".to_string(),
            "".to_string(),
        ];

        let transformed = transformer.transform_with_stats(urls).0;

        assert_eq!(transformed.len(), 3);
        assert!(transformed.contains(&"plain-text".to_string()));
        assert!(transformed.contains(&"://start-with-colon".to_string()));
        assert!(transformed.contains(&"".to_string()));
    }

    // --- output-views ---

    fn view(urls: &[&str], view: ParamView) -> Vec<String> {
        let mut transformer = UrlTransformer::new();
        transformer.with_param_view(view);
        transformer
            .transform_with_stats(urls.iter().map(|s| s.to_string()).collect())
            .0
    }

    #[test]
    fn test_params_lists_every_parameter_name_once_sorted() {
        let out = view(
            &[
                "https://example.com/search?q=a&page=2",
                "https://example.com/search?q=b&sort=asc",
                "https://example.com/other?q=c",
                "https://example.com/nothing",
            ],
            ParamView::Names,
        );
        assert_eq!(out, vec!["page", "q", "sort"]);
    }

    #[test]
    fn test_params_keeps_names_verbatim_and_ignores_bare_values() {
        // A bare `?foo` is a parameter with no value, and an encoded name is
        // reported exactly as the archive recorded it — decoding it would
        // produce a name that does not exist on the target.
        let out = view(
            &[
                "https://example.com/a?foo",
                "https://example.com/b?we%20ird=1",
                "https://example.com/c?=novalue",
                "https://example.com/d?",
            ],
            ParamView::Names,
        );
        assert_eq!(out, vec!["foo", "we%20ird"]);
    }

    #[test]
    fn test_params_skips_unparseable_input() {
        assert!(view(&["not-a-url?q=1"], ParamView::Names).is_empty());
    }

    #[test]
    fn test_params_by_endpoint_unions_the_names_per_endpoint() {
        let out = view(
            &[
                "https://example.com/search?q=x",
                "https://example.com/search?page=2&q=y",
                "https://example.com/search?sort=asc",
                "https://example.com/login",
            ],
            ParamView::ByEndpoint,
        );
        // One line per endpoint, parameters sorted and deduped; the
        // parameterless endpoint contributes nothing.
        assert_eq!(out, vec!["https://example.com/search page,q,sort"]);
    }

    #[test]
    fn test_params_by_endpoint_collapses_identifier_segments() {
        // The same reuse --dedup-similar makes of normalize_segment: /post/1
        // and /post/2 are one endpoint wearing different data.
        let out = view(
            &[
                "https://example.com/post/1?a=1",
                "https://example.com/post/2?b=2",
                "https://example.com/post/550e8400-e29b-41d4-a716-446655440000?c=3",
            ],
            ParamView::ByEndpoint,
        );
        assert_eq!(out, vec!["https://example.com/post/{id} a,b,c"]);
    }

    #[test]
    fn test_params_by_endpoint_keeps_hosts_apart() {
        // A bare path would merge two different endpoints into one line that is
        // true of neither, which is why the endpoint is spelled out in full.
        let out = view(
            &[
                "https://a.example.com/search?q=1",
                "https://b.example.com/search?debug=1",
            ],
            ParamView::ByEndpoint,
        );
        assert_eq!(
            out,
            vec![
                "https://a.example.com/search q".to_string(),
                "https://b.example.com/search debug".to_string(),
            ]
        );
    }

    #[test]
    fn test_params_by_endpoint_line_is_cuttable_on_the_first_space() {
        let out = view(&["https://example.com/s?q=1&r=2"], ParamView::ByEndpoint);
        let (endpoint, params) = out[0].split_once(' ').expect("one space separator");
        assert_eq!(endpoint, "https://example.com/s");
        assert_eq!(params, "q,r");
    }

    #[test]
    fn test_fuzz_placeholder_replaces_values_and_folds_the_signature() {
        let out = view(
            &[
                "https://example.com/search?q=cats&page=1",
                "https://example.com/search?q=dogs&page=2",
                "https://example.com/search?q=birds",
            ],
            ParamView::Fuzz("FUZZ".to_string()),
        );
        // Two signatures — {q,page} and {q} — so two templates, not five.
        assert_eq!(
            out,
            vec![
                "https://example.com/search?q=FUZZ".to_string(),
                "https://example.com/search?q=FUZZ&page=FUZZ".to_string(),
            ]
        );
    }

    #[test]
    fn test_fuzz_placeholder_drops_urls_without_parameters() {
        let out = view(
            &["https://example.com/about", "https://example.com/x?"],
            ParamView::Fuzz("FUZZ".to_string()),
        );
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn test_fuzz_placeholder_keeps_a_routable_path() {
        // The template is meant to go straight into ffuf, so the path stays the
        // representative's real one — a `{id}` placeholder would not route.
        // Which representative that is follows --dedup-similar's rule: the
        // lexicographically smallest URL of the group, so "22" wins over "3".
        let out = view(
            &[
                "https://example.com/post/22/edit?token=abc",
                "https://example.com/post/3/edit?token=def",
            ],
            ParamView::Fuzz("FUZZ".to_string()),
        );
        assert_eq!(out, vec!["https://example.com/post/22/edit?token=FUZZ"]);
    }

    #[test]
    fn test_fuzz_placeholder_is_deterministic_regardless_of_input_order() {
        let forward = view(
            &[
                "https://example.com/a?x=1",
                "https://example.com/b?x=2",
                "https://example.com/c?x=3",
            ],
            ParamView::Fuzz("FUZZ".to_string()),
        );
        let backward = view(
            &[
                "https://example.com/c?x=3",
                "https://example.com/b?x=2",
                "https://example.com/a?x=1",
            ],
            ParamView::Fuzz("FUZZ".to_string()),
        );
        assert_eq!(forward, backward);
    }

    #[test]
    fn test_fuzz_placeholder_dedupes_a_repeated_name_and_drops_the_fragment() {
        let out = view(
            &["https://example.com/s?a=1&a=2&b=3#frag"],
            ParamView::Fuzz("FUZZ".to_string()),
        );
        assert_eq!(out, vec!["https://example.com/s?a=FUZZ&b=FUZZ"]);
    }

    #[test]
    fn test_param_view_composes_with_the_earlier_stages() {
        // --normalize-url runs first, so the sorted query it produces is what
        // the inventory sees; the view then replaces the list outright.
        let mut transformer = UrlTransformer::new();
        transformer
            .with_normalize_url(true)
            .with_param_view(ParamView::Names);
        let out = transformer
            .transform_with_stats(vec![
                "https://example.com/s/?z=1&a=2".to_string(),
                "https://example.com/s?a=3".to_string(),
            ])
            .0;
        assert_eq!(out, vec!["a", "z"]);
    }

    #[test]
    fn test_no_param_view_leaves_the_list_alone() {
        let urls = ["https://example.com/a?q=1", "https://example.com/b"];
        assert_eq!(
            view(&urls, ParamView::None),
            urls.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_wordlist_terms_takes_path_segments_and_parameter_names() {
        assert_eq!(
            wordlist_terms("https://example.com/admin/users?id=1&sort=asc"),
            vec!["admin", "users", "id", "sort"]
        );
    }

    #[test]
    fn test_wordlist_terms_drops_data_looking_segments() {
        // Numbers, UUIDs, hashes, dates and opaque tokens are words that exist
        // on exactly one target, which is the opposite of a wordlist.
        assert_eq!(
            wordlist_terms(
                "https://example.com/post/4711/550e8400-e29b-41d4-a716-446655440000/2024-01-02/edit"
            ),
            vec!["post", "edit"]
        );
        // The stem is the identifier here, so the whole segment goes: `{id}.html`
        // is not a word either.
        assert_eq!(
            wordlist_terms("https://example.com/news/1234.html"),
            vec!["news"]
        );
        // A stem that is a name with a number stuck on it is still a route,
        // and `normalize_segment` leaves it alone — so it stays a word.
        assert_eq!(
            wordlist_terms("https://example.com/news/article-1234.html"),
            vec!["news", "article-1234.html"]
        );
        // ...but a segment that merely contains digits is a real route name.
        assert_eq!(
            wordlist_terms("https://example.com/api/v2/2fa"),
            vec!["api", "v2", "2fa"]
        );
    }

    #[test]
    fn test_wordlist_terms_preserves_case_and_dedupes_within_a_url() {
        // Path segments are case-sensitive on most origins, so lower-casing
        // would produce words that 404 everywhere they are tried.
        assert_eq!(
            wordlist_terms("https://example.com/Admin/admin/Admin?Q=1&Q=2"),
            vec!["Admin", "admin", "Q"]
        );
    }

    #[test]
    fn test_wordlist_terms_of_an_unparseable_or_bare_url() {
        assert!(wordlist_terms("not a url").is_empty());
        assert!(wordlist_terms("https://example.com/").is_empty());
    }
}
