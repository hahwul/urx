//! BeVigil: URLs extracted from unpacked Android apps.
//!
//! BeVigil (CloudSEK) decompiles mobile apps and indexes the endpoints found in
//! their code and resources. That is a corpus no web archive has — an API host
//! only ever called from an app never appears in a crawl — which is why it is
//! worth a provider of its own. The OSINT API is keyed
//! (`X-Access-Token`), one request per domain, no pagination.
//!
//! # Response schema — an assumption, stated
//!
//! The schema below was **not** verified against a live response: no key was
//! available while this was written. It follows the public documentation at
//! <https://bevigil.com/osint-api> and the shape projectdiscovery's `urlfinder`
//! reads from the same endpoint
//! (<https://github.com/projectdiscovery/urlfinder>, `pkg/source/bevigil`):
//!
//! ```json
//! { "domain": "example.com", "urls": ["https://example.com/api/v1/…", "…"] }
//! ```
//!
//! Because that is an assumption, the parser is deliberately loose about the
//! shape — `urls` may hold strings or objects carrying a `url` field, and a
//! bare top-level array is accepted too — and when nothing recognisable is
//! there it fails *loudly*, naming the top-level keys it did see (keys only:
//! values could carry the token or user data) so a schema drift is diagnosable
//! from `-v` output rather than showing up as "BeVigil found nothing".

use anyhow::Result;
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;

use super::ApiKeyRotator;
use super::{Provider, UrlRecord};
use crate::network::client::send_with_retry;
use crate::network::NetConfig;
use crate::progress::ProgressReporter;

#[derive(Clone)]
pub struct BeVigilProvider {
    api_key_rotator: ApiKeyRotator,
    include_subdomains: bool,
    net: NetConfig,
    base_url: String,
}

/// Pull URLs out of one JSON value, in any of the shapes the module docs
/// describe. `None` means the shape was not recognised at all — distinct from
/// a recognised shape holding zero URLs, which is `Some(empty)`.
fn extract_urls(value: &serde_json::Value) -> Option<Vec<String>> {
    let list = match value {
        // Documented: {"domain": ..., "urls": [...]}
        serde_json::Value::Object(map) => map.get("urls")?.as_array()?,
        // Tolerated: a bare array.
        serde_json::Value::Array(items) => items,
        _ => return None,
    };

    let mut urls = Vec::with_capacity(list.len());
    for item in list {
        let url = match item {
            serde_json::Value::String(s) => Some(s.as_str()),
            // Tolerated: [{"url": "..."}, ...]
            serde_json::Value::Object(map) => map.get("url").and_then(|u| u.as_str()),
            _ => None,
        };
        if let Some(url) = url {
            let url = url.trim();
            if url.starts_with("http://") || url.starts_with("https://") {
                urls.push(url.to_string());
            }
        }
    }
    Some(urls)
}

/// Describe an unrecognised body for the error message without echoing it:
/// the top-level keys of an object, or the JSON type of anything else.
fn describe_shape(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
            format!("top-level keys: [{}]", keys.join(", "))
        }
        serde_json::Value::Array(_) => "a top-level array without URL strings".to_string(),
        serde_json::Value::String(_) => "a JSON string".to_string(),
        serde_json::Value::Number(_) => "a JSON number".to_string(),
        serde_json::Value::Bool(_) => "a JSON boolean".to_string(),
        serde_json::Value::Null => "JSON null".to_string(),
    }
}

/// Parse a BeVigil `/urls/` body into deduplicated, sorted URLs.
///
/// A body that is not JSON, or JSON of a shape with no URL list in it, is an
/// error that says what *was* there — see the module docs for why that is
/// preferable to an empty result.
pub(crate) fn parse_response(body: &str) -> Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        anyhow::anyhow!(
            "BeVigil answered with a non-JSON body ({e}); expected {{\"domain\", \"urls\": [...]}}"
        )
    })?;
    match extract_urls(&value) {
        Some(urls) => {
            let unique: BTreeSet<String> = urls.into_iter().collect();
            Ok(unique.into_iter().collect())
        }
        None => Err(anyhow::anyhow!(
            "BeVigil response did not match the expected schema {{\"domain\", \"urls\": [...]}} — got {}. \
             The schema was taken from the public API docs; if BeVigil changed it, this is where to look.",
            describe_shape(&value)
        )),
    }
}

impl BeVigilProvider {
    pub fn new_with_keys(api_keys: Vec<String>) -> Self {
        BeVigilProvider {
            api_key_rotator: ApiKeyRotator::new(api_keys),
            include_subdomains: false,
            net: NetConfig::default(),
            base_url: "https://osint.bevigil.com".to_string(),
        }
    }

    /// `GET /api/{domain}/urls/`. The domain is a path segment, so it is
    /// percent-encoded rather than spliced in raw.
    fn request_url(&self, domain: &str) -> String {
        let encoded: String = url::form_urlencoded::byte_serialize(domain.as_bytes()).collect();
        format!("{}/api/{encoded}/urls/", self.base_url)
    }
}

impl Provider for BeVigilProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlRecord>>> + Send + 'a>> {
        self.fetch_urls_with_progress(domain, None)
    }

    fn fetch_urls_with_progress<'a>(
        &'a self,
        domain: &'a str,
        reporter: Option<ProgressReporter>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlRecord>>> + Send + 'a>> {
        Box::pin(async move {
            if !self.api_key_rotator.has_keys() {
                return Ok(Vec::new());
            }

            let client = self.net.http.build_client()?;
            let url = self.request_url(domain);

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            let body = send_with_retry(
                self.net.retries,
                self.net.rate_limit.as_ref(),
                |status| status == 429 || status.is_server_error(),
                || {
                    // Rotate per attempt so a throttled key is retried with a
                    // different one when several are configured.
                    let api_key = self.api_key_rotator.next_key().unwrap_or_default();
                    client
                        .get(&url)
                        .header("X-Access-Token", api_key)
                        .header("Accept", "application/json")
                },
                |status, body| match status.as_u16() {
                    200..=299 => Ok(Some(body)),
                    // The API's own word for "no such domain in the index".
                    // Assumed, like the schema: a 404 here is treated as an
                    // empty result rather than a failure.
                    404 => Ok(None),
                    // A bad or expired token is deterministic; retrying with
                    // the same keys only burns requests.
                    401 | 403 => Err(anyhow::anyhow!(
                        "BeVigil rejected the API key (HTTP {status}); check --bevigil-api-key / URX_BEVIGIL_API_KEY"
                    )),
                    _ => Err(anyhow::anyhow!("HTTP error: {status}")),
                },
            )
            .await?;
            // Parsed outside the retry: a body of the wrong shape is
            // deterministic, and asking again only burns requests.
            let urls = match body {
                Some(body) => parse_response(&body)?,
                None => Vec::new(),
            };
            Ok(urls.into_iter().map(UrlRecord::bare).collect())
        })
    }

    fn with_subdomains(&mut self, include: bool) {
        // BeVigil's index is keyed by the domain and already returns every URL
        // found under it; host validation downstream applies the scope.
        self.include_subdomains = include;
    }

    fn with_network(&mut self, net: NetConfig) {
        self.net = net;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::urls_of;
    use mockito::Matcher;

    fn provider(server: &mockito::ServerGuard, keys: &[&str]) -> BeVigilProvider {
        let mut p = BeVigilProvider::new_with_keys(keys.iter().map(|k| k.to_string()).collect());
        p.base_url = server.url();
        p.net.retries = 0;
        p
    }

    #[test]
    fn documented_schema_parses_and_dedupes() {
        let body = r#"{"domain":"example.com","urls":[
            "https://example.com/api/v1/login",
            "https://api.example.com/v2/users",
            "https://example.com/api/v1/login",
            " http://example.com/legacy ",
            "ftp://example.com/skip",
            42
        ]}"#;
        assert_eq!(
            parse_response(body).unwrap(),
            vec![
                "http://example.com/legacy",
                "https://api.example.com/v2/users",
                "https://example.com/api/v1/login",
            ]
        );
    }

    #[test]
    fn tolerated_shapes_parse_too() {
        // Objects with a `url` field, and a bare array.
        let objects = r#"{"urls":[{"url":"https://example.com/a","source":"apk"},{"nourl":1}]}"#;
        assert_eq!(
            parse_response(objects).unwrap(),
            vec!["https://example.com/a"]
        );
        let bare = r#"["https://example.com/b"]"#;
        assert_eq!(parse_response(bare).unwrap(), vec!["https://example.com/b"]);
        // A recognised shape with nothing in it is an empty success.
        assert!(parse_response(r#"{"domain":"example.com","urls":[]}"#)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn an_unexpected_schema_names_the_keys_it_saw_but_not_the_values() {
        let body =
            r#"{"domain":"example.com","endpoints":["https://example.com/x"],"token":"SECRET"}"#;
        let err = parse_response(body).unwrap_err().to_string();
        assert!(err.contains("did not match the expected schema"), "{err}");
        assert!(
            err.contains("top-level keys: [domain, endpoints, token]"),
            "{err}"
        );
        assert!(!err.contains("SECRET"), "values must not be echoed: {err}");
        assert!(
            !err.contains("example.com/x"),
            "values must not be echoed: {err}"
        );

        let err = parse_response("<html>login</html>")
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-JSON body"), "{err}");

        let err = parse_response("\"just a string\"").unwrap_err().to_string();
        assert!(err.contains("a JSON string"), "{err}");
    }

    #[test]
    fn request_url_is_built_from_the_domain() {
        let p = BeVigilProvider::new_with_keys(vec!["k".to_string()]);
        assert_eq!(
            p.request_url("example.com"),
            "https://osint.bevigil.com/api/example.com/urls/"
        );
        // Nothing a hostile target string carries can reach the path raw.
        assert_eq!(
            p.request_url("a/b?c"),
            "https://osint.bevigil.com/api/a%2Fb%3Fc/urls/"
        );
    }

    #[tokio::test]
    async fn fetch_sends_the_token_and_returns_bare_records() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/example.com/urls/")
            .match_header("x-access-token", "key-1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"domain":"example.com","urls":["https://example.com/b","https://example.com/a"]}"#)
            .expect(1)
            .create_async()
            .await;

        let p = provider(&server, &["key-1"]);
        let records = p.fetch_urls("example.com").await.unwrap();
        assert_eq!(
            records.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            vec!["https://example.com/a", "https://example.com/b"]
        );
        assert!(
            records.iter().all(|r| r.meta.is_empty()),
            "BeVigil has no capture index; nothing may be invented"
        );
        mock.assert();
    }

    #[tokio::test]
    async fn an_unexpected_schema_is_an_error_not_an_empty_result() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/example.com/urls/")
            .with_status(200)
            .with_body(r#"{"domain":"example.com","data":{"urls":[]}}"#)
            .create_async()
            .await;

        let p = provider(&server, &["key-1"]);
        let err = p.fetch_urls("example.com").await.unwrap_err().to_string();
        assert!(err.contains("top-level keys: [data, domain]"), "{err}");
    }

    #[tokio::test]
    async fn no_key_means_no_request() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", Matcher::Any)
            .expect(0)
            .create_async()
            .await;

        let p = provider(&server, &[]);
        assert!(!p.api_key_rotator.has_keys());
        let urls = urls_of(p.fetch_urls("example.com").await.unwrap());
        assert!(urls.is_empty());
        mock.assert();
    }

    #[tokio::test]
    async fn a_rejected_key_is_reported_without_retrying() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/example.com/urls/")
            .with_status(401)
            .with_body(r#"{"detail":"Access Token Invalid"}"#)
            .expect(1)
            .create_async()
            .await;

        let mut p = provider(&server, &["bad"]);
        p.net.retries = 3;
        let err = p.fetch_urls("example.com").await.unwrap_err().to_string();
        assert!(err.contains("rejected the API key"), "{err}");
        assert!(err.contains("URX_BEVIGIL_API_KEY"), "{err}");
        mock.assert();
    }

    #[tokio::test]
    async fn not_found_is_an_empty_result() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/example.com/urls/")
            .with_status(404)
            .create_async()
            .await;

        let p = provider(&server, &["key-1"]);
        assert!(p.fetch_urls("example.com").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rate_limits_rotate_keys_and_retry() {
        let mut server = mockito::Server::new_async().await;
        let throttled = server
            .mock("GET", "/api/example.com/urls/")
            .match_header("x-access-token", "key-1")
            .with_status(429)
            .with_header("retry-after", "0")
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("GET", "/api/example.com/urls/")
            .match_header("x-access-token", "key-2")
            .with_status(200)
            .with_body(r#"{"urls":["https://example.com/a"]}"#)
            .expect(1)
            .create_async()
            .await;

        let mut p = provider(&server, &["key-1", "key-2"]);
        p.net.retries = 1;
        let urls = urls_of(p.fetch_urls("example.com").await.unwrap());
        assert_eq!(urls, vec!["https://example.com/a"]);
        throttled.assert();
        ok.assert();
    }

    #[tokio::test]
    async fn a_server_error_that_never_clears_is_the_error_returned() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/example.com/urls/")
            .with_status(503)
            .expect(2)
            .create_async()
            .await;

        let mut p = provider(&server, &["key-1"]);
        p.net.retries = 1;
        let err = p.fetch_urls("example.com").await.unwrap_err().to_string();
        assert!(err.contains("503"), "{err}");
        mock.assert();
    }
}
