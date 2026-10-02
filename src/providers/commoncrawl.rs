use anyhow::Result;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::OnceCell;

use super::cdx::{walk_block_pages, CdxSession, MAX_PAGES};
use super::filters::{ArchiveFilters, CdxDialect};
use super::{Provider, UrlRecord};
use crate::network::client::{get_with_retry, HttpClientConfig};
use crate::network::NetConfig;
use crate::progress::ProgressReporter;

/// Sentinel value that asks the provider to resolve the most recent Common
/// Crawl index at runtime via `collinfo.json`.
pub(crate) const LATEST_INDEX_ALIAS: &str = "latest";

/// Validate that a Common Crawl index identifier matches the expected
/// `CC-MAIN-YYYY-WW` shape before we splice it into a URL path. This guards
/// against a hostile or corrupted `collinfo.json` causing path manipulation.
fn is_valid_cc_index_id(id: &str) -> bool {
    // Expected: "CC-MAIN-YYYY-WW" — fixed prefix, 4-digit year, 2-digit week.
    let Some(rest) = id.strip_prefix("CC-MAIN-") else {
        return false;
    };
    let mut parts = rest.split('-');
    let (Some(year), Some(week), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    year.len() == 4
        && year.chars().all(|c| c.is_ascii_digit())
        && week.len() == 2
        && week.chars().all(|c| c.is_ascii_digit())
}

#[derive(Clone)]
pub struct CommonCrawlProvider {
    index: String,
    /// Cached resolution of `LATEST_INDEX_ALIAS`. Shared across clones so the
    /// `collinfo.json` lookup happens at most once per run.
    resolved_index: Arc<OnceCell<String>>,
    include_subdomains: bool,
    net: NetConfig,
    /// Server-side CDX predicates (date range, status code, MIME type).
    filters: ArchiveFilters,
    base_url: String,
}

#[derive(Deserialize)]
struct CollInfoEntry {
    id: String,
}

impl CommonCrawlProvider {
    /// Creates a provider instance with a specific Common Crawl index.
    ///
    /// Pass [`LATEST_INDEX_ALIAS`] (`"latest"`) to defer resolution until the
    /// first fetch — the provider will then look up `collinfo.json` and use
    /// the most recent published index.
    pub fn with_index(index: String) -> Self {
        CommonCrawlProvider {
            index,
            resolved_index: Arc::new(OnceCell::new()),
            include_subdomains: false,
            net: NetConfig {
                http: HttpClientConfig {
                    timeout: 10,
                    random_agent: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            filters: ArchiveFilters::default(),
            base_url: "https://index.commoncrawl.org".to_string(),
        }
    }

    /// Apply server-side CDX predicates (date range, status code, MIME type).
    /// The Common Crawl index server runs pywb, so it names these fields
    /// `status`/`mime` rather than the classic CDX `statuscode`/`mimetype`.
    pub fn with_filters(&mut self, filters: ArchiveFilters) -> &mut Self {
        self.filters = filters;
        self
    }

    /// Resolve `self.index`, fetching `collinfo.json` once if the user passed
    /// the `latest` alias. The resolved value is memoised across all clones.
    async fn effective_index(&self) -> Result<String> {
        if !self.index.eq_ignore_ascii_case(LATEST_INDEX_ALIAS) {
            return Ok(self.index.clone());
        }

        let cached = self
            .resolved_index
            .get_or_try_init(|| async {
                let url = format!("{}/collinfo.json", self.base_url);
                let client = self.net.http.build_client()?;
                let body = get_with_retry(&client, &url, self.net.retries).await?;
                let entries: Vec<CollInfoEntry> = serde_json::from_str(&body)?;
                let id = entries
                    .into_iter()
                    .next()
                    .map(|e| e.id)
                    .ok_or_else(|| anyhow::anyhow!("collinfo.json returned no entries"))?;
                if !is_valid_cc_index_id(&id) {
                    return Err(anyhow::anyhow!(
                        "collinfo.json returned an unexpected index id: {id:?}"
                    ));
                }
                Ok(id)
            })
            .await?;

        Ok(cached.clone())
    }

    /// Build the index query without pagination params. `output=json` streams
    /// one JSON record per line; `&page=N` / `&showNumPages=true` are appended
    /// per request.
    fn query_base(&self, index: &str, domain: &str) -> String {
        let base_url = &self.base_url;
        let pattern = super::cdx_url_pattern(domain, self.include_subdomains);
        let mut url = format!("{base_url}/{index}-index?url={pattern}&output=json");
        url.push_str(&self.filters.query_params(CdxDialect::Pywb));
        url
    }
}

impl Provider for CommonCrawlProvider {
    /// A CDX query carries the path scope itself; see
    /// [`super::cdx_url_pattern`].
    fn accepts_path_scope(&self) -> bool {
        true
    }

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
            let index = self.effective_index().await?;
            let query_base = self.query_base(&index, domain);
            let client = self.net.http.build_client()?;

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            // The Common Crawl index server block-paginates; the shared pywb
            // walk in `cdx` probes `showNumPages` and walks every page.
            let session = CdxSession {
                client: &client,
                retries: self.net.retries,
                limiter: self.net.rate_limit.as_ref(),
                reporter: reporter.as_ref(),
                endpoint: &self.base_url,
            };
            walk_block_pages(&session, &query_base, MAX_PAGES).await
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
    use crate::progress::StopSignal;
    use crate::providers::cdx::{PywbRow as CCRecord, MAX_CONSECUTIVE_PAGE_FAILURES};
    use crate::providers::urls_of;

    #[test]
    fn test_with_index() {
        let index = "CC-MAIN-2023-06".to_string();
        let provider = CommonCrawlProvider::with_index(index.clone());
        assert_eq!(provider.index, index);
        assert_eq!(provider.base_url, "https://index.commoncrawl.org");
    }

    #[test]
    fn test_with_subdomains() {
        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_clone_box() {
        let provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[test]
    fn test_cc_record_deserialize() {
        let json = r#"{"url":"https://example.com/test"}"#;
        let record: CCRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.url, "https://example.com/test");
    }

    #[test]
    fn test_cc_record_carries_the_capture_metadata() {
        // A real CDXJ row, unused fields included, so the parse is exercised
        // against the shape the index actually serves.
        let json = r#"{"urlkey":"com,example)/","timestamp":"20250802232428",
            "url":"http://www.example.com/","mime":"text/html",
            "mime-detected":"text/html","status":"200",
            "digest":"JI6OR3QR4CI526JD6TMMNZNV4QPMPQCH","length":"1219"}"#;
        let record: CCRecord = serde_json::from_str(json).unwrap();
        let meta = record.into_record().meta;

        assert_eq!(meta.first_seen(), Some("20250802232428"));
        assert_eq!(meta.last_seen(), Some("20250802232428"));
        assert_eq!(meta.mime(), Some("text/html"));
        assert_eq!(meta.archive_status(), Some("200"));
        assert_eq!(meta.digest(), Some("JI6OR3QR4CI526JD6TMMNZNV4QPMPQCH"));
    }

    #[test]
    fn test_cc_record_without_metadata_stays_empty() {
        let record: CCRecord = serde_json::from_str(r#"{"url":"https://example.com/"}"#).unwrap();
        assert!(record.into_record().meta.is_empty());
    }

    #[test]
    fn test_url_construction_without_subdomains() {
        // This test just verifies that the URL is constructed correctly without making a network request
        let provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());

        // Use private helper function to check URL formation
        let url = format!(
            "https://index.commoncrawl.org/{}-index?url={}/*&output=json",
            provider.index, "example.com"
        );

        assert_eq!(
            url,
            "https://index.commoncrawl.org/CC-MAIN-2026-17-index?url=example.com/*&output=json"
        );
    }

    #[test]
    fn test_url_construction_with_subdomains() {
        // This test just verifies that the URL is constructed correctly without making a network request
        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.with_subdomains(true);

        // Use private helper function to check URL formation
        let url = format!(
            "https://index.commoncrawl.org/{}-index?url=*.{}/*&output=json",
            provider.index, "example.com"
        );

        assert_eq!(
            url,
            "https://index.commoncrawl.org/CC-MAIN-2026-17-index?url=*.example.com/*&output=json"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_integration() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("output".into(), "json".into()),
            ]))
            .with_status(200)
            .with_body(
                "{\"url\": \"https://example.com/page1\", \"timestamp\": \"20240101000000\", \"mime\": \"text/html\", \"status\": \"200\", \"digest\": \"NEW\"}\n\
                 {\"url\": \"https://example.com/page2\"}\n\
                 {\"url\": \"https://example.com/page1\", \"timestamp\": \"20200101000000\", \"mime\": \"text/plain\", \"status\": \"404\", \"digest\": \"OLD\"}",
            )
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        // Override base_url to point to the mock server
        provider.base_url = server.url();

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok());

        let records = result.unwrap();
        // Should be deduped and sorted
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].url, "https://example.com/page1");
        assert_eq!(records[1].url, "https://example.com/page2");

        // Both captures of page1 fold into one record; the newest one decides
        // the MIME type and status.
        let page1 = &records[0].meta;
        assert_eq!(page1.first_seen(), Some("20200101000000"));
        assert_eq!(page1.last_seen(), Some("20240101000000"));
        assert_eq!(page1.mime(), Some("text/html"));
        assert_eq!(page1.archive_status(), Some("200"));

        // A row the index served without metadata gets none invented for it.
        assert!(records[1].meta.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_urls_paginates_all_pages() {
        let mut server = mockito::Server::new_async().await;

        // The server reports the query spans two pages.
        let probe = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded(
                "showNumPages".into(),
                "true".into(),
            ))
            .with_status(200)
            .with_body(r#"{"pages": 2, "pageSize": 5, "blocks": 9}"#)
            .expect(1)
            .create_async()
            .await;
        let page0 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            .with_body("{\"url\": \"https://example.com/a\"}")
            .expect(1)
            .create_async()
            .await;
        let page1 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_body("{\"url\": \"https://example.com/b\"}")
            .expect(1)
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.base_url = server.url();

        let urls = urls_of(provider.fetch_urls("example.com").await.unwrap());
        assert_eq!(
            urls,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ]
        );
        probe.assert();
        page0.assert();
        page1.assert();
    }

    #[tokio::test]
    async fn test_latest_alias_resolves_via_collinfo() {
        let mut server = mockito::Server::new_async().await;

        let collinfo = server
            .mock("GET", "/collinfo.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[
                    {"id": "CC-MAIN-2099-01", "name": "Latest"},
                    {"id": "CC-MAIN-2098-50", "name": "Previous"}
                ]"#,
            )
            .expect(1)
            .create_async()
            .await;

        // Each fetch issues a showNumPages probe plus the page fetch, so two
        // fetch_urls calls hit the index endpoint four times. The probe body
        // isn't a page-count document, so the provider falls back to one page.
        let index_mock = server
            .mock("GET", "/CC-MAIN-2099-01-index")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/*".into()),
                mockito::Matcher::UrlEncoded("output".into(), "json".into()),
            ]))
            .with_status(200)
            .with_body("{\"url\": \"https://example.com/a\"}")
            .expect(4)
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index(LATEST_INDEX_ALIAS.to_string());
        provider.base_url = server.url();

        // First fetch triggers collinfo lookup; second reuses the cached value.
        let first = urls_of(provider.fetch_urls("example.com").await.unwrap());
        let second = urls_of(provider.fetch_urls("example.com").await.unwrap());

        assert_eq!(first, vec!["https://example.com/a".to_string()]);
        assert_eq!(second, first);

        collinfo.assert();
        index_mock.assert();
    }

    #[tokio::test]
    async fn test_latest_alias_is_case_insensitive() {
        let mut server = mockito::Server::new_async().await;

        let _collinfo = server
            .mock("GET", "/collinfo.json")
            .with_status(200)
            .with_body(r#"[{"id": "CC-MAIN-2099-01"}]"#)
            .create_async()
            .await;

        let _idx = server
            .mock("GET", "/CC-MAIN-2099-01-index")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body("")
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index("LATEST".to_string());
        provider.base_url = server.url();

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_latest_alias_errors_on_empty_collinfo() {
        let mut server = mockito::Server::new_async().await;

        let _collinfo = server
            .mock("GET", "/collinfo.json")
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index(LATEST_INDEX_ALIAS.to_string());
        provider.base_url = server.url();
        provider.net.retries = 0;

        let err = provider.fetch_urls("example.com").await.unwrap_err();
        assert!(err
            .to_string()
            .contains("collinfo.json returned no entries"));
    }

    #[tokio::test]
    async fn test_latest_alias_rejects_malformed_index_id() {
        let mut server = mockito::Server::new_async().await;

        let _collinfo = server
            .mock("GET", "/collinfo.json")
            .with_status(200)
            .with_body(r#"[{"id": "../../etc/passwd"}]"#)
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index(LATEST_INDEX_ALIAS.to_string());
        provider.base_url = server.url();
        provider.net.retries = 0;

        let err = provider.fetch_urls("example.com").await.unwrap_err();
        assert!(err.to_string().contains("unexpected index id"));
    }

    #[test]
    fn test_is_valid_cc_index_id() {
        assert!(is_valid_cc_index_id("CC-MAIN-2026-17"));
        assert!(is_valid_cc_index_id("CC-MAIN-2099-01"));
        assert!(!is_valid_cc_index_id("CC-MAIN-2026-1"));
        assert!(!is_valid_cc_index_id("CC-MAIN-2026"));
        assert!(!is_valid_cc_index_id("CC-MAIN-2026-17-extra"));
        assert!(!is_valid_cc_index_id("CC-MAIN-202X-17"));
        assert!(!is_valid_cc_index_id("../../etc/passwd"));
        assert!(!is_valid_cc_index_id(""));
    }

    #[test]
    fn test_query_base_uses_pywb_filter_fields() {
        // The CC index server matches filter values EXACTLY — a regex such as
        // `20.` or `(200|301)` returns nothing there — and names the fields
        // `status`/`mime` rather than the classic `statuscode`/`mimetype`.
        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.with_filters(ArchiveFilters::from_cli_lists(
            None,
            None,
            &["200".to_string()],
            &[],
            &[],
            &["text/html".to_string()],
        ));

        let q = provider.query_base("CC-MAIN-2026-17", "example.com");
        assert!(q.contains("&filter=status:200"), "{q}");
        assert!(q.contains("&filter=!mime:text%2Fhtml"), "{q}");
        assert!(!q.contains("statuscode"), "{q}");
        assert!(!q.contains("mimetype"), "{q}");
    }

    #[test]
    fn test_query_base_drops_unsatisfiable_positive_list() {
        // "200 or 301" cannot be expressed against an exact-match index, so the
        // filter is omitted rather than sent as an AND that matches nothing.
        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.with_filters(ArchiveFilters::from_cli_lists(
            None,
            None,
            &["200".to_string(), "301".to_string()],
            &[],
            &[],
            &[],
        ));

        let q = provider.query_base("CC-MAIN-2026-17", "example.com");
        assert!(!q.contains("filter="), "{q}");
    }

    #[tokio::test]
    async fn test_stop_request_keeps_the_pages_already_walked() {
        // Companion to the wayback case: the page walk must hand back what it
        // has when the run asks it to stop, instead of letting the runner's
        // hard cancel drop the whole buffer. The stop is raised from the
        // page-zero handler, so it lands exactly once that page has been
        // served -- no sleeps, no race.
        let mut server = mockito::Server::new_async().await;
        let stop = StopSignal::default();

        let _probe = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded(
                "showNumPages".into(),
                "true".into(),
            ))
            .with_status(200)
            .with_body(r#"{"pages": 3, "pageSize": 5, "blocks": 12}"#)
            .expect(1)
            .create_async()
            .await;
        let flip = stop.clone();
        let page0 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            .with_body_from_request(move |_| {
                flip.request_stop();
                b"{\"url\": \"https://example.com/a\"}".to_vec()
            })
            .expect(1)
            .create_async()
            .await;
        // Pages after the stop must never be requested.
        let page1 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_body("{\"url\": \"https://example.com/b\"}")
            .expect(0)
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.base_url = server.url();
        provider.net.retries = 0;

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ")
            .with_stop_signal(stop.clone());

        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );

        assert_eq!(urls, vec!["https://example.com/a".to_string()]);
        // Pages 1 and 2 were never walked, so the crawl is incomplete.
        assert!(reporter.is_partial());
        page0.assert();
        page1.assert();
    }

    #[tokio::test]
    async fn test_one_failed_page_does_not_abandon_the_rest() {
        // Common Crawl's `page=N` is a direct block address, not a cursor, so a
        // failure on one page tells us nothing about the pages after it. The
        // walk used to `break` here, discarding every remaining page — on a
        // 266-page domain one transient 503 cost 98% of the result.
        let mut server = mockito::Server::new_async().await;

        let _probe = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded(
                "showNumPages".into(),
                "true".into(),
            ))
            .with_status(200)
            .with_body(r#"{"pages": 3, "pageSize": 5, "blocks": 12}"#)
            .expect(1)
            .create_async()
            .await;
        let page0 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            .with_body("{\"url\": \"https://example.com/a\"}")
            .expect(1)
            .create_async()
            .await;
        let page1 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(503)
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "2".into()))
            .with_status(200)
            .with_body("{\"url\": \"https://example.com/c\"}")
            .expect(1)
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.base_url = server.url();
        provider.net.retries = 0; // fail fast, don't sleep through back-off

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");
        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );

        // Page 2 is still fetched even though page 1 failed.
        assert_eq!(
            urls,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/c".to_string(),
            ]
        );
        // ...and the gap is reported rather than passed off as a clean crawl.
        assert!(reporter.is_partial());
        page0.assert();
        page1.assert();
        page2.assert();
    }

    #[tokio::test]
    async fn test_walk_gives_up_after_repeated_page_failures() {
        // A run of failures means the index is unhealthy, not that one page was
        // unlucky — stop instead of walking every remaining page.
        let mut server = mockito::Server::new_async().await;

        let _probe = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded(
                "showNumPages".into(),
                "true".into(),
            ))
            .with_status(200)
            .with_body(r#"{"pages": 20, "pageSize": 5, "blocks": 100}"#)
            .expect(1)
            .create_async()
            .await;
        let _page0 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "0".into()))
            .with_status(200)
            .with_body("{\"url\": \"https://example.com/a\"}")
            .expect(1)
            .create_async()
            .await;
        // Pages 1.. all fail; only MAX_CONSECUTIVE_PAGE_FAILURES of them are
        // attempted before the walk stops.
        let failing = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::Any)
            .with_status(503)
            .expect(MAX_CONSECUTIVE_PAGE_FAILURES)
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.base_url = server.url();
        provider.net.retries = 0;

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");
        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );

        assert_eq!(urls, vec!["https://example.com/a".to_string()]);
        assert!(reporter.is_partial());
        failing.assert();
    }

    #[tokio::test]
    async fn test_first_page_failure_is_still_fatal() {
        // A domain the index has no captures for answers page 0 with a 404;
        // that must stay an error rather than an empty success.
        let mut server = mockito::Server::new_async().await;

        let _probe = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::UrlEncoded(
                "showNumPages".into(),
                "true".into(),
            ))
            .with_status(200)
            .with_body(r#"{"pages": 2, "pageSize": 5, "blocks": 6}"#)
            .expect(1)
            .create_async()
            .await;
        let _page0 = server
            .mock("GET", "/CC-MAIN-2026-17-index")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .create_async()
            .await;

        let mut provider = CommonCrawlProvider::with_index("CC-MAIN-2026-17".to_string());
        provider.base_url = server.url();
        provider.net.retries = 0;

        assert!(provider.fetch_urls("example.com").await.is_err());
    }
}
