use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use super::ApiKeyRotator;
use super::{Provider, UrlRecord};
use crate::network::client::send_with_retry;
use crate::network::NetConfig;
use crate::progress::ProgressReporter;

#[derive(Clone)]
pub struct ZoomEyeProvider {
    api_key_rotator: ApiKeyRotator,
    include_subdomains: bool,
    net: NetConfig,
    base_url: String,
    page_limit: u32,
}

/// ZoomEye v2 returns HTTP 200 for business-logic errors too; only this `code`
/// means the query succeeded. Anything else (bad/expired key, quota, malformed
/// query) must be surfaced rather than read as "zero results".
const ZOOMEYE_SUCCESS_CODE: i32 = 60000;

/// Hard ceiling on pages walked for one domain, so a stale or inflated `total`
/// from the server can't drive an unbounded request loop.
const ZOOMEYE_MAX_PAGES: u32 = 1_000;

#[derive(Debug, Serialize, Deserialize)]
struct ZoomEyeResponse {
    #[serde(default)]
    code: i32,
    /// Human-readable explanation that accompanies a non-success `code`.
    #[serde(default)]
    message: String,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    data: Vec<ZoomEyeEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ZoomEyeEntry {
    #[serde(default)]
    url: String,
    #[serde(default)]
    ip: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Serialize)]
struct ZoomEyeRequest {
    qbase64: String,
    page: u32,
    pagesize: u32,
    sub_type: String,
}

/// Error text for a response whose `code` is not the success code. Includes
/// ZoomEye's own `message` when it sent one, so the user sees e.g. "auth failed"
/// instead of a bare numeric code.
fn api_error_message(response: &ZoomEyeResponse) -> String {
    let message = response.message.trim();
    if message.is_empty() {
        format!("ZoomEye API error: code {}", response.code)
    } else {
        format!("ZoomEye API error: code {} ({message})", response.code)
    }
}

impl ZoomEyeProvider {
    pub fn new_with_keys(api_keys: Vec<String>) -> Self {
        ZoomEyeProvider {
            api_key_rotator: ApiKeyRotator::new(api_keys),
            include_subdomains: false,
            net: NetConfig::default(),
            base_url: "https://api.zoomeye.ai".to_string(),
            page_limit: ZOOMEYE_MAX_PAGES,
        }
    }

    fn build_dork(&self, domain: &str) -> String {
        if self.include_subdomains {
            format!("site:*.{domain}")
        } else {
            format!("site:{domain}")
        }
    }
}

impl Provider for ZoomEyeProvider {
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

            if let Some(r) = &reporter {
                r.detail("fetching…");
            }

            let dork = self.build_dork(domain);
            let qbase64 = STANDARD.encode(dork.as_bytes());

            let api_url = format!("{}/v2/search", self.base_url);
            let client = self.net.http.build_client()?;

            let mut all_urls: Vec<String> = Vec::new();
            let mut page: u32 = 1;
            let pagesize: u32 = 100;
            // Rows actually received, counted before we drop the ones without a
            // `url`. Progress against `total` must be measured from what the
            // server sent, not from the page size we asked for — see the stop
            // condition at the bottom of the loop.
            let mut rows_received: u64 = 0;

            loop {
                let request_body = ZoomEyeRequest {
                    qbase64: qbase64.clone(),
                    page,
                    pagesize,
                    sub_type: "web".to_string(),
                };

                let fetched = send_with_retry(
                    self.net.retries,
                    self.net.rate_limit.as_ref(),
                    |_| true,
                    || {
                        // Rotate the key per attempt so a rate-limited/quota-hit
                        // key is retried with a different one when several are
                        // configured.
                        let api_key = self.api_key_rotator.next_key().unwrap_or_default();
                        client
                            .post(&api_url)
                            .header("API-KEY", api_key)
                            .json(&request_body)
                    },
                    |_, body| {
                        serde_json::from_str::<ZoomEyeResponse>(&body)
                            .map_err(|e| anyhow::anyhow!("Failed to parse ZoomEye response: {e}"))
                    },
                )
                .await
                .and_then(|response| {
                    // A 200 with a non-success code is an API error (rejected
                    // key, quota, bad query) — don't mistake it for an empty
                    // result set.
                    if response.code != ZOOMEYE_SUCCESS_CODE {
                        anyhow::bail!("{}", api_error_message(&response));
                    }
                    Ok(response)
                });

                let response = match fetched {
                    Ok(response) => response,
                    // Best effort: a page that failed after all its retries
                    // must not discard the pages already collected. Only a
                    // failure with nothing collected is fatal, matching every
                    // other paginating provider.
                    Err(e) if all_urls.is_empty() => return Err(e),
                    Err(_) => {
                        // We're returning a truncated result. Flag it so the
                        // runner marks the line partial and warns instead of
                        // presenting an incomplete crawl as a clean success.
                        if let Some(r) = &reporter {
                            r.mark_partial();
                        }
                        break;
                    }
                };
                let total = response.total;
                let page_rows = response.data.len() as u64;

                // A page that returned no rows means the data is exhausted even
                // if `total` claims otherwise — stop rather than loop on a stale
                // count. Judged on the rows the server sent, not on the URLs we
                // kept: a page whose rows all lack a `url` is still a page of
                // data, and treating it as the end truncated the walk.
                let page_was_empty = page_rows == 0;
                rows_received += page_rows;
                all_urls.extend(
                    response
                        .data
                        .into_iter()
                        .map(|entry| entry.url)
                        .filter(|url| !url.is_empty()),
                );

                if let Some(r) = &reporter {
                    r.detail(format!("{} URLs…", all_urls.len()));
                    // The run asked us to stop (--max-time elapsed, or Ctrl-C).
                    // Hand back the pages already walked instead of losing them
                    // to the hard cancel after the runner's grace window.
                    if r.stop_requested() {
                        r.mark_partial();
                        break;
                    }
                }

                // Check if there are more pages. `rows_received` is what the
                // server actually returned; the old code assumed every page held
                // exactly `pagesize` rows, so a server that clamps the page size
                // below what we asked for (plan limits, or its own cap) made urx
                // believe it had seen `page * 100` results and stop after a
                // fraction of them.
                if page_was_empty || rows_received >= total {
                    break;
                }

                if page >= self.page_limit {
                    if let Some(r) = &reporter {
                        r.mark_partial();
                    }
                    break;
                }

                page += 1;
            }

            Ok(all_urls.into_iter().map(UrlRecord::bare).collect())
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
        let provider = ZoomEyeProvider::new_with_keys(api_keys);

        assert!(provider.api_key_rotator.has_keys());

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
        );
    }

    #[test]
    fn test_new_provider_filters_empty_keys() {
        let api_keys = vec![
            "key1".to_string(),
            "".to_string(),
            "key2".to_string(),
            "".to_string(),
        ];
        let provider = ZoomEyeProvider::new_with_keys(api_keys);

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
        let provider = ZoomEyeProvider::new_with_keys(vec!["".to_string()]);
        assert!(!provider.api_key_rotator.has_keys());
    }

    #[test]
    fn test_build_dork() {
        let provider = ZoomEyeProvider::new_with_keys(vec!["key".to_string()]);
        assert_eq!(provider.build_dork("example.com"), "site:example.com");
    }

    #[test]
    fn test_build_dork_with_subdomains() {
        let mut provider = ZoomEyeProvider::new_with_keys(vec!["key".to_string()]);
        provider.with_subdomains(true);
        assert_eq!(provider.build_dork("example.com"), "site:*.example.com");
    }

    #[test]
    fn test_with_subdomains() {
        let provider = &mut ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_clone_box() {
        let provider = ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        let _cloned = provider.clone_box();
    }

    #[test]
    fn test_zoomeye_response_deserialize() {
        let json = r#"{
            "code": 60000,
            "total": 2,
            "data": [
                {
                    "url": "https://example.com/page1",
                    "ip": "1.2.3.4",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Example Page 1"
                },
                {
                    "url": "https://example.com/page2",
                    "ip": "1.2.3.5",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Example Page 2"
                }
            ]
        }"#;

        let response: ZoomEyeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.code, 60000);
        assert_eq!(response.total, 2);
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].url, "https://example.com/page1");
        assert_eq!(response.data[1].url, "https://example.com/page2");
        assert_eq!(response.data[0].domain, "example.com");
        assert_eq!(response.data[0].port, 443);
    }

    #[test]
    fn test_zoomeye_response_empty_deserialize() {
        let json = r#"{"code": 60000, "total": 0, "data": []}"#;

        let response: ZoomEyeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.code, 60000);
        assert_eq!(response.total, 0);
        assert_eq!(response.data.len(), 0);
    }

    #[test]
    fn test_qbase64_encoding() {
        let dork = "site:example.com";
        let encoded = STANDARD.encode(dork.as_bytes());
        assert_eq!(encoded, "c2l0ZTpleGFtcGxlLmNvbQ==");
    }

    #[tokio::test]
    async fn test_fetch_urls_with_empty_api_key() {
        let provider = ZoomEyeProvider::new_with_keys(vec!["".to_string()]);
        let result = provider.fetch_urls("example.com").await;

        assert!(result.is_ok(), "Expected success with empty API key");
        let urls = urls_of(result.unwrap());
        assert_eq!(urls.len(), 0, "Expected empty URLs list with empty API key");
    }

    #[tokio::test]
    async fn test_fetch_urls_surfaces_business_error_code() {
        // HTTP 200 but a non-success `code` (e.g. expired key / quota) must be
        // an error, not a silent empty result.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/v2/search")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code": 60500, "message": "auth failed", "data": []}"#)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["expired-key".to_string()]);
        provider.base_url = server.url();
        provider.net.retries = 0;

        let err = provider
            .fetch_urls("example.com")
            .await
            .expect_err("non-success code should be an error");
        assert!(err.to_string().contains("60500"), "got: {err}");
        assert!(err.to_string().contains("auth failed"), "got: {err}");
    }

    #[test]
    fn test_api_error_message_without_message() {
        let response = ZoomEyeResponse {
            code: 60500,
            message: "  ".to_string(),
            total: 0,
            data: vec![],
        };
        assert_eq!(
            api_error_message(&response),
            "ZoomEye API error: code 60500"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_with_mock() {
        let mut mock_server = mockito::Server::new_async().await;

        let mock_response = r#"{
            "code": 60000,
            "total": 2,
            "data": [
                {
                    "url": "https://example.com/page1",
                    "ip": "1.2.3.4",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 1"
                },
                {
                    "url": "https://example.com/page2",
                    "ip": "1.2.3.5",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 2"
                }
            ]
        }"#;

        let _m = mock_server
            .mock("POST", "/v2/search")
            .match_header("API-KEY", "test_api_key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_response)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = mock_server.url();

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "Expected success with mock API");

        let urls = urls_of(result.unwrap());
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/page1");
        assert_eq!(urls[1], "https://example.com/page2");
    }

    #[tokio::test]
    async fn test_fetch_urls_keeps_earlier_pages_when_a_later_page_fails() {
        // Regression: a page failing after its retries used to `return Err`,
        // discarding every page already collected — so one flaky page late in a
        // large result set reported the whole provider as failed.
        let mut mock_server = mockito::Server::new_async().await;

        // A full first page (100 rows) with total=200, so the walk continues.
        let rows: Vec<String> = (0..100)
            .map(|i| format!(r#"{{"url":"https://example.com/{i}","ip":"1.2.3.4","domain":"example.com","port":443,"title":"t"}}"#))
            .collect();
        let _page1 = mock_server
            .mock("POST", "/v2/search")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"page":1}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{ "code": 60000, "total": 200, "data": [{}] }}"#,
                rows.join(",")
            ))
            .create_async()
            .await;
        // ...and page 2 is permanently broken.
        let _page2 = mock_server
            .mock("POST", "/v2/search")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"page":2}"#.to_string(),
            ))
            .with_status(503)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = mock_server.url();
        provider.net.retries = 0; // fail fast, don't sleep through back-off

        let reporter = ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ");
        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .expect("page one must survive a later page's failure"),
        );

        assert_eq!(urls.len(), 100);
        assert!(urls.contains(&"https://example.com/0".to_string()));
        // The lost page is surfaced as a partial result, not a clean success.
        assert!(reporter.is_partial());
    }

    #[tokio::test]
    async fn test_fetch_urls_errors_when_the_first_page_fails() {
        // Nothing collected means nothing to salvage — the failure propagates.
        let mut mock_server = mockito::Server::new_async().await;
        let _m = mock_server
            .mock("POST", "/v2/search")
            .with_status(503)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = mock_server.url();
        provider.net.retries = 0;

        assert!(provider.fetch_urls("example.com").await.is_err());
    }

    #[tokio::test]
    async fn test_pagination_counts_rows_returned_not_the_requested_page_size() {
        // Regression: progress against `total` was computed as `page * 100`,
        // the page size urx *asks* for. A server that returns fewer rows per
        // page than requested — ZoomEye clamps by plan — made urx believe it
        // had already seen 100 results after a 10-row page, so it stopped after
        // 3 pages of a 25-result set and dropped the rest.
        let mut mock_server = mockito::Server::new_async().await;

        // 25 results served 10 rows at a time: pages 1 and 2 full, page 3 short.
        for (page, range) in [(1u32, 0..10u32), (2, 10..20), (3, 20..25)] {
            let rows: Vec<String> = range
                .map(|i| {
                    format!(
                        r#"{{"url":"https://example.com/{i}","ip":"1.2.3.4","domain":"example.com","port":443,"title":"t"}}"#
                    )
                })
                .collect();
            mock_server
                .mock("POST", "/v2/search")
                .match_body(mockito::Matcher::PartialJsonString(format!(
                    r#"{{"page":{page}}}"#
                )))
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(format!(
                    r#"{{ "code": 60000, "total": 25, "data": [{}] }}"#,
                    rows.join(",")
                ))
                .create_async()
                .await;
        }

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = mock_server.url();

        let urls = urls_of(provider.fetch_urls("example.com").await.unwrap());
        assert_eq!(urls.len(), 25, "walk stopped early: {}", urls.len());
        assert!(urls.contains(&"https://example.com/24".to_string()));
    }

    #[tokio::test]
    async fn test_page_limit_marks_results_partial_when_zoomeye_reports_more() {
        let mut server = mockito::Server::new_async().await;
        let page1 = server
            .mock("POST", "/v2/search")
            .match_body(mockito::Matcher::PartialJsonString(r#"{"page":1}"#.into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"code":60000,"total":2,"data":[{"url":"https://example.com/a"}]}"#)
            .expect(1)
            .create_async()
            .await;
        let page2 = server
            .mock("POST", "/v2/search")
            .match_body(mockito::Matcher::PartialJsonString(r#"{"page":2}"#.into()))
            .with_status(200)
            .with_body(r#"{"code":60000,"total":2,"data":[]}"#)
            .expect(0)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["key".to_string()]);
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
    async fn test_pagination_continues_past_a_page_whose_rows_all_lack_a_url() {
        // A page of rows without a usable `url` is still a page of data. It used
        // to read as "no more results" because emptiness was judged after the
        // rows were filtered, halting the walk before the rest of the set.
        let mut mock_server = mockito::Server::new_async().await;

        let blank: Vec<String> = (0..2)
            .map(|_| {
                r#"{"url":"","ip":"1.2.3.4","domain":"example.com","port":80,"title":""}"#
                    .to_string()
            })
            .collect();
        let _p1 = mock_server
            .mock("POST", "/v2/search")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"page":1}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{ "code": 60000, "total": 3, "data": [{}] }}"#,
                blank.join(",")
            ))
            .create_async()
            .await;
        let _p2 = mock_server
            .mock("POST", "/v2/search")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"page":2}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{ "code": 60000, "total": 3, "data": [
                    {"url":"https://example.com/kept","ip":"1.2.3.4","domain":"example.com","port":443,"title":"t"}
                ] }"#,
            )
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = mock_server.url();

        let urls = urls_of(provider.fetch_urls("example.com").await.unwrap());
        assert_eq!(urls, vec!["https://example.com/kept".to_string()]);
    }

    #[tokio::test]
    async fn test_fetch_urls_skips_empty_urls() {
        let mut mock_server = mockito::Server::new_async().await;

        let mock_response = r#"{
            "code": 60000,
            "total": 3,
            "data": [
                {
                    "url": "https://example.com/page1",
                    "ip": "1.2.3.4",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 1"
                },
                {
                    "url": "",
                    "ip": "1.2.3.5",
                    "domain": "example.com",
                    "port": 80,
                    "title": ""
                },
                {
                    "url": "https://example.com/page3",
                    "ip": "1.2.3.6",
                    "domain": "example.com",
                    "port": 443,
                    "title": "Page 3"
                }
            ]
        }"#;

        let _m = mock_server
            .mock("POST", "/v2/search")
            .match_header("API-KEY", "test_api_key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_response)
            .create_async()
            .await;

        let mut provider = ZoomEyeProvider::new_with_keys(vec!["test_api_key".to_string()]);
        provider.base_url = mock_server.url();

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok());

        let urls = urls_of(result.unwrap());
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/page1");
        assert_eq!(urls[1], "https://example.com/page3");
    }
}
