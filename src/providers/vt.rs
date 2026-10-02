use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use super::ApiKeyRotator;
use super::{Provider, UrlRecord};
use crate::network::client::send_with_retry;
use crate::network::NetConfig;
use crate::progress::ProgressReporter;

/// Page size for the v3 `urls` relationship. VirusTotal caps this relationship
/// endpoint at 40 per page; larger values are silently clamped server-side.
const VT_PAGE_LIMIT: usize = 40;

/// Hard ceiling on pages followed for one domain, mirroring the other
/// paginating providers so a misbehaving `links.next` can't loop forever.
const VT_MAX_PAGES: usize = 10_000;

#[derive(Clone)]
pub struct VirusTotalProvider {
    api_key_rotator: ApiKeyRotator,
    include_subdomains: bool,
    net: NetConfig,
    base_url: String,
    page_limit: usize,
}

/// A page of the v3 `/domains/{domain}/urls` response. `data` holds the URL
/// objects; `meta.cursor` is the opaque token for the next page (absent on the
/// last page). We deliberately page via this token rather than the sibling
/// `links.next` absolute URL: `links.next` is server-controlled, and following
/// it would send the `x-apikey` header to whatever host it names — a credential
/// leak under a malicious/MITM'd response. Rebuilding the request from the
/// trusted base URL + cursor removes that trust entirely. `Default` lets a 404
/// ("no such domain") resolve to an empty page rather than an error.
#[derive(Debug, Deserialize, Default)]
struct VtUrlsResponse {
    #[serde(default)]
    data: Vec<VtUrlObject>,
    #[serde(default)]
    meta: VtMeta,
}

#[derive(Debug, Deserialize, Default)]
struct VtMeta {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VtUrlObject {
    attributes: VtUrlAttributes,
}

#[derive(Debug, Deserialize)]
struct VtUrlAttributes {
    url: String,
}

impl VirusTotalProvider {
    pub fn new_with_keys(api_keys: Vec<String>) -> Self {
        VirusTotalProvider {
            api_key_rotator: ApiKeyRotator::new(api_keys),
            include_subdomains: false,
            net: NetConfig::default(),
            base_url: "https://www.virustotal.com".to_string(),
            page_limit: VT_MAX_PAGES,
        }
    }

    /// Build a page URL for a domain's v3 `urls` relationship from the *trusted*
    /// base, optionally carrying an opaque pagination `cursor`. Always built
    /// from our own base host (never a server-supplied URL), so the API key is
    /// only ever sent to VirusTotal.
    fn page_url(&self, domain: &str, cursor: Option<&str>) -> String {
        let encoded = url::form_urlencoded::byte_serialize(domain.as_bytes()).collect::<String>();
        let mut url = format!(
            "{}/api/v3/domains/{encoded}/urls?limit={VT_PAGE_LIMIT}",
            self.base_url
        );
        if let Some(cursor) = cursor {
            // The cursor is opaque base64-ish; percent-encode so reserved bytes
            // survive being spliced into the query string.
            let encoded_cursor: String =
                url::form_urlencoded::byte_serialize(cursor.as_bytes()).collect();
            url.push_str("&cursor=");
            url.push_str(&encoded_cursor);
        }
        url
    }

    /// Fetch and parse a single page with retry/back-off and key rotation.
    ///
    /// A 404 (the domain has no VT object) resolves to an empty page rather
    /// than an error, matching the "no data" semantics of the other providers.
    async fn fetch_page(&self, client: &reqwest::Client, url: &str) -> Result<VtUrlsResponse> {
        send_with_retry(
            self.net.retries,
            self.net.rate_limit.as_ref(),
            |status| status != 404,
            || {
                // Rotate the key per attempt so a throttled/invalid key is
                // retried with a different one when several are configured.
                // v3 carries the key in the `x-apikey` header.
                let api_key = self.api_key_rotator.next_key().unwrap_or_default();
                let req = client.get(url);
                if api_key.is_empty() {
                    req
                } else {
                    req.header("x-apikey", api_key)
                }
            },
            |status, body| {
                if status == 404 {
                    return Ok(VtUrlsResponse::default());
                }
                serde_json::from_str(&body)
                    .map_err(|e| anyhow::anyhow!("Failed to parse VirusTotal response: {e}"))
            },
        )
        .await
    }
}

impl Provider for VirusTotalProvider {
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
            // Skip if no API keys are provided.
            if !self.api_key_rotator.has_keys() {
                return Ok(Vec::new());
            }

            let client = self.net.http.build_client()?;

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            // Walk the v3 cursor: each page returns up to VT_PAGE_LIMIT URL
            // objects plus a `meta.cursor` token for the next page. We rebuild
            // each request from the trusted base + cursor (never the server's
            // `links.next` URL). The deprecated v2 `domain/report` returned a
            // single server-capped, non-paginated slice that silently truncated
            // large domains.
            let mut urls = Vec::new();
            let mut cursor: Option<String> = None;
            let mut seen_cursors = HashSet::new();
            let mut pages = 0usize;

            loop {
                pages += 1;
                if pages > self.page_limit {
                    if let Some(r) = &reporter {
                        r.mark_partial();
                    }
                    break;
                }
                // "First request" tracked explicitly: a clean (HTTP 200) but
                // empty leading page can still carry a cursor, so emptiness is
                // not a reliable proxy for "first page".
                let first_page = pages == 1;
                let url = self.page_url(domain, cursor.as_deref());

                let page = match self.fetch_page(&client, &url).await {
                    Ok(page) => page,
                    Err(e) => {
                        // A failure on the very first request is fatal; any
                        // later failure keeps what we have and flags the result
                        // partial rather than presenting a truncated crawl as a
                        // clean success.
                        if first_page {
                            return Err(e);
                        }
                        if let Some(r) = &reporter {
                            r.mark_partial();
                        }
                        break;
                    }
                };

                for obj in page.data {
                    urls.push(obj.attributes.url);
                }
                if let Some(r) = &reporter {
                    r.detail(format!("{} URLs…", urls.len()));
                    // The run asked us to stop (--max-time elapsed, or
                    // Ctrl-C). Hand back the pages already walked instead of
                    // losing them to the hard cancel after the grace window.
                    if r.stop_requested() {
                        r.mark_partial();
                        break;
                    }
                }

                match page.meta.cursor {
                    Some(c) if !c.is_empty() => {
                        if !seen_cursors.insert(c.clone()) {
                            if let Some(r) = &reporter {
                                r.mark_partial();
                            }
                            break;
                        }
                        cursor = Some(c);
                    }
                    _ => break,
                }
            }

            urls.sort();
            urls.dedup();
            Ok(urls.into_iter().map(UrlRecord::bare).collect())
        })
    }

    fn with_subdomains(&mut self, include: bool) {
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

    #[test]
    fn test_new_provider_with_multiple_keys() {
        let api_keys = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let provider = VirusTotalProvider::new_with_keys(api_keys.clone());

        assert!(provider.api_key_rotator.has_keys());

        // Test rotation
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key1".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key2".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key3".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key1".to_string())
        ); // Should wrap
    }

    #[test]
    fn test_new_provider_filters_empty_keys() {
        let api_keys = vec![
            "key1".to_string(),
            "".to_string(),
            "key2".to_string(),
            "".to_string(),
        ];
        let provider = VirusTotalProvider::new_with_keys(api_keys);

        assert!(provider.api_key_rotator.has_keys());
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key1".to_string())
        );
        assert_eq!(
            provider.api_key_rotator.next_key(),
            Some("key2".to_string())
        );
    }

    #[test]
    fn test_new_provider_with_empty_key() {
        let provider = VirusTotalProvider::new_with_keys(vec!["".to_string()]);
        assert!(!provider.api_key_rotator.has_keys());
    }

    #[tokio::test]
    async fn test_transport_error_does_not_leak_api_key() {
        // The v3 key travels in the `x-apikey` header, but a transport-layer
        // failure (which reqwest renders with the full URL) must still not
        // surface it — and we strip the URL from the error for good measure.
        let mut provider = VirusTotalProvider::new_with_keys(vec!["SUPERSECRETKEY".to_string()]);
        // Port 1 reliably refuses the connection; keep the run fast.
        provider.base_url = "http://127.0.0.1:1".to_string();
        provider.net.retries = 0;
        provider.net.http.timeout = 5;

        let err = provider
            .fetch_urls("example.com")
            .await
            .expect_err("connection to port 1 should fail");
        let msg = err.to_string();
        assert!(
            !msg.contains("SUPERSECRETKEY"),
            "API key leaked in error message: {msg}"
        );
    }

    #[test]
    fn test_with_subdomains() {
        let provider = &mut VirusTotalProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_clone_box() {
        let provider = VirusTotalProvider::new_with_keys(vec!["test_api_key".to_string()]);
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[test]
    fn test_vt_response_deserialize() {
        // v3 shape: data[].attributes.url plus a meta.cursor token. The
        // server-controlled `links` block is present but intentionally ignored.
        let json = r#"{
            "data": [
                {"type": "url", "id": "a", "attributes": {"url": "https://example.com/page1"}},
                {"type": "url", "id": "b", "attributes": {"url": "https://example.com/page2"}}
            ],
            "links": {"self": "https://x/self", "next": "https://x/next"},
            "meta": {"cursor": "CURSOR"}
        }"#;

        let response: VtUrlsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].attributes.url, "https://example.com/page1");
        assert_eq!(response.data[1].attributes.url, "https://example.com/page2");
        assert_eq!(response.meta.cursor.as_deref(), Some("CURSOR"));
    }

    #[test]
    fn test_vt_response_empty_deserialize() {
        // No data and no cursor (last/empty page) must parse cleanly.
        let json = r#"{"data": [], "meta": {}}"#;
        let response: VtUrlsResponse = serde_json::from_str(json).unwrap();
        assert!(response.data.is_empty());
        assert!(response.meta.cursor.is_none());

        // A bare object also parses (all fields default).
        let response: VtUrlsResponse = serde_json::from_str("{}").unwrap();
        assert!(response.data.is_empty());
        assert!(response.meta.cursor.is_none());
    }

    #[tokio::test]
    async fn test_fetch_urls_with_empty_api_key() {
        let provider = VirusTotalProvider::new_with_keys(vec!["".to_string()]);
        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_ok(), "Expected success with empty API key");
        let urls = urls_of(result.unwrap());
        assert_eq!(urls.len(), 0, "Expected empty URLs list with empty API key");
    }

    #[tokio::test]
    async fn test_fetch_urls_with_invalid_api_key() {
        let provider = VirusTotalProvider::new_with_keys(vec!["invalid_key".to_string()]);
        // This test should fail with an HTTP error since the API key is invalid
        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_err(), "Expected error with invalid API key");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("HTTP error")
                || err.contains("Failed after")
                || err.contains("VirusTotal")
                || err.contains("parse"),
            "Unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_with_mock() {
        let mut server = mockito::Server::new_async().await;

        // v3: domain in the path, key in the x-apikey header, urls under
        // data[].attributes.url. A single page (no meta.cursor) ends the walk.
        let m = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_header("x-apikey", "test_api_key")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "40".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "data": [
                        {"attributes": {"url": "https://example.com/page1"}},
                        {"attributes": {"url": "https://example.com/page2"}}
                    ],
                    "meta": {}
                }"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = server.url();

        let urls = urls_of(provider.fetch_urls("example.com").await.unwrap());
        assert_eq!(
            urls,
            vec![
                "https://example.com/page1".to_string(),
                "https://example.com/page2".to_string(),
            ]
        );
        m.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_paginates_via_cursor_ignoring_server_next_url() {
        let mut server = mockito::Server::new_async().await;

        // Page one hands back a meta.cursor token AND a hostile `links.next`
        // pointing at an unrelated host. The provider must page by rebuilding
        // the request from its trusted base + cursor and must NEVER follow
        // links.next (which would leak the x-apikey header off-host). Page two
        // is served from THIS server keyed by cursor=PAGE2, so the fact that it
        // is reached proves links.next was ignored.
        let page1 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Exact("limit=40".into()))
            .with_status(200)
            .with_body(
                r#"{
                    "data": [{"attributes": {"url": "https://example.com/a"}}],
                    "meta": {"cursor": "PAGE2"},
                    "links": {"next": "http://127.0.0.1:1/evil?leak=key"}
                }"#,
            )
            .expect(1)
            .create_async()
            .await;
        // Page two is keyed by the cursor and carries no cursor, so the walk ends.
        let page2 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::UrlEncoded(
                "cursor".into(),
                "PAGE2".into(),
            ))
            .with_status(200)
            .with_body(
                r#"{"data": [{"attributes": {"url": "https://example.com/b"}}], "meta": {}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = server.url();

        let urls = urls_of(provider.fetch_urls("example.com").await.unwrap());
        assert_eq!(
            urls,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ]
        );
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_page_limit_marks_results_partial_when_vt_has_more() {
        let mut server = mockito::Server::new_async().await;
        let page1 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Exact("limit=40".into()))
            .with_status(200)
            .with_body(
                r#"{"data":[{"attributes":{"url":"https://example.com/a"}}],"meta":{"cursor":"NEXT"}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::UrlEncoded("cursor".into(), "NEXT".into()))
            .with_status(200)
            .with_body(r#"{"data":[],"meta":{}}"#)
            .expect(0)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new_with_keys(vec!["key".to_string()]);
        provider.base_url = server.url();
        provider.page_limit = 1;
        provider.net.retries = 0;
        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");

        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );

        assert_eq!(urls, vec!["https://example.com/a"]);
        assert!(
            reporter.is_partial(),
            "the page ceiling truncates this crawl"
        );
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_repeated_vt_cursor_stops_early_and_marks_partial() {
        let mut server = mockito::Server::new_async().await;
        let page1 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Exact("limit=40".into()))
            .with_status(200)
            .with_body(
                r#"{"data":[{"attributes":{"url":"https://example.com/a"}}],"meta":{"cursor":"LOOP"}}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::UrlEncoded("cursor".into(), "LOOP".into()))
            .with_status(200)
            .with_body(
                r#"{"data":[{"attributes":{"url":"https://example.com/b"}}],"meta":{"cursor":"LOOP"}}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new_with_keys(vec!["key".to_string()]);
        provider.base_url = server.url();
        provider.page_limit = 3;
        provider.net.retries = 0;
        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");

        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );

        assert_eq!(urls.len(), 2);
        assert!(reporter.is_partial());
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_fetch_urls_keeps_partial_on_midpage_failure() {
        let mut server = mockito::Server::new_async().await;

        let _page1 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Exact("limit=40".into()))
            .with_status(200)
            .with_body(
                r#"{"data": [{"attributes": {"url": "https://example.com/a"}}], "meta": {"cursor": "PAGE2"}}"#,
            )
            .create_async()
            .await;
        // The follow-up page fails: keep page one and flag the result partial.
        let _page2 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::UrlEncoded(
                "cursor".into(),
                "PAGE2".into(),
            ))
            .with_status(503)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = server.url();
        provider.net.retries = 0; // fail fast, no back-off sleeps

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "t · ");
        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );
        assert_eq!(urls, vec!["https://example.com/a".to_string()]);
        assert!(reporter.is_partial());
    }

    #[tokio::test]
    async fn test_fetch_urls_empty_first_page_then_failure_is_partial_not_error() {
        // A clean (HTTP 200) but EMPTY first page can still carry a cursor. A
        // later-page failure must yield a partial (Ok) result, not a hard error
        // — the first request succeeded. Guards against using urls.is_empty() as
        // a "first page" proxy.
        let mut server = mockito::Server::new_async().await;

        let _page1 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Exact("limit=40".into()))
            .with_status(200)
            .with_body(r#"{"data": [], "meta": {"cursor": "PAGE2"}}"#)
            .create_async()
            .await;
        let _page2 = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::UrlEncoded(
                "cursor".into(),
                "PAGE2".into(),
            ))
            .with_status(503)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = server.url();
        provider.net.retries = 0;

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "t · ");
        let result = provider
            .fetch_urls_with_progress("example.com", Some(reporter.clone()))
            .await;
        assert!(
            result.is_ok(),
            "empty-but-200 first page + later failure must be partial, not error"
        );
        assert!(result.unwrap().is_empty());
        assert!(reporter.is_partial());
    }

    #[tokio::test]
    async fn test_fetch_urls_404_returns_empty() {
        // A domain with no VT object answers 404; treat it as "no data", not an
        // error, matching the other providers.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/api/v3/domains/example.com/urls")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .with_body(r#"{"error": {"code": "NotFoundError"}}"#)
            .create_async()
            .await;

        let mut provider = VirusTotalProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = server.url();
        provider.net.retries = 0;

        let urls = urls_of(provider.fetch_urls("example.com").await.unwrap());
        assert!(urls.is_empty());
    }
}
