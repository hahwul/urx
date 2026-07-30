use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

/// Common HTTP client configuration shared across providers and testers.
///
/// This struct centralizes the logic for building a `reqwest::Client` with
/// proxy, timeout, TLS, and User-Agent settings so that every provider and
/// tester does not have to duplicate the same builder code.
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Request timeout in seconds
    pub timeout: u64,
    /// Skip TLS certificate verification
    pub insecure: bool,
    /// Use a randomized User-Agent header
    pub random_agent: bool,
    /// Optional proxy URL (e.g. "http://proxy:8080")
    pub proxy: Option<String>,
    /// Optional proxy authentication in "username:password" format
    pub proxy_auth: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            timeout: 30,
            insecure: false,
            random_agent: false,
            proxy: None,
            proxy_auth: None,
        }
    }
}

impl HttpClientConfig {
    /// Build a `reqwest::Client` from this configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the proxy URL is invalid or the client fails to build.
    pub fn build_client(&self) -> Result<Client> {
        let mut builder = Client::builder().timeout(Duration::from_secs(self.timeout));

        if self.insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }

        // Always send a User-Agent. reqwest sends none by default, and several
        // upstreams — notably the Wayback CDX API — answer a UA-less request
        // with `400 Bad Request`, so an unset header was a silent, blanket
        // source of provider failures. `--random-agent` rotates realistic
        // browser strings; otherwise we send a polite, tool-identifying default.
        let ua = if self.random_agent {
            crate::network::random_user_agent()
        } else {
            crate::network::default_user_agent()
        };
        builder = builder.user_agent(ua);

        if let Some(proxy_url) = &self.proxy {
            let mut proxy = reqwest::Proxy::all(proxy_url)?;

            if let Some(auth) = &self.proxy_auth {
                let username = auth.split(':').next().unwrap_or("");
                let password = auth.split(':').nth(1).unwrap_or("");
                proxy = proxy.basic_auth(username, password);
            }

            builder = builder.proxy(proxy);
        }

        Ok(builder.build()?)
    }
}

/// Parse a `Retry-After` response header into a sleep duration so a throttled
/// request waits as long as the server asked before retrying. Only the
/// delta-seconds form (the common API case, e.g. `Retry-After: 30`) is honored;
/// the HTTP-date form returns `None` and the caller falls back to its normal
/// back-off. The value is capped so a hostile or absurd header can't stall a run.
pub fn retry_after_delay(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    const MAX_RETRY_AFTER_SECS: u64 = 60;
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: u64 = raw.trim().parse().ok()?;
    Some(Duration::from_secs(secs.min(MAX_RETRY_AFTER_SECS)))
}

/// Ceiling on a single response body buffered by [`get_with_retry`].
///
/// Every provider that walks a CDX index goes through `get_with_retry`, and each
/// of those requests is already bounded server-side (a 50k-row Wayback page, one
/// Common Crawl block, one OTX page of 500 records) — a legitimate body is single
/// -digit megabytes. This limit exists for the case where the remote does *not*
/// honour that bound: a hostile, misconfigured, or hijacked index answering with
/// an endless stream would otherwise be buffered in full, in memory, once per
/// concurrent domain.
pub const MAX_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

/// Read a response body, stopping after `max` bytes.
///
/// Reads incrementally via `chunk()` rather than buffering the whole body, so an
/// endpoint that streams gigabytes — by accident or on purpose — can't exhaust
/// memory before any parsing happens. Every caller that fetches a document from
/// a host urx does not control should go through this.
pub async fn read_body_capped(mut resp: reqwest::Response, max: usize) -> Result<String> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        let remaining = max.saturating_sub(buf.len());
        if remaining == 0 {
            break;
        }
        if chunk.len() > remaining {
            buf.extend_from_slice(&chunk[..remaining]);
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Read a JSON response body and deserialize it, bounded by
/// [`MAX_RESPONSE_BYTES`].
///
/// `reqwest::Response::json` buffers the entire body first, which the keyed API
/// providers (urlscan, VirusTotal, ZoomEye, GitHub) must not do for a host urx
/// does not control — the same reason [`get_with_retry`] reads capped.
pub async fn read_json_capped<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T> {
    let body = read_body_capped(resp, MAX_RESPONSE_BYTES).await?;
    Ok(serde_json::from_str(&body)?)
}

/// Whether an HTTP status is worth another attempt.
///
/// Server errors and explicit throttling are transient. The rest of the 4xx
/// range is a deterministic "no" and retrying it only wastes requests and
/// wall-clock: a 404 from a CDX index is how those APIs say "this domain has no
/// captures", so re-asking three times with back-off turns an instant miss into
/// a multi-second one — per domain, per provider.
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::REQUEST_TIMEOUT
}

/// Execute an HTTP GET request with retry and linear back-off.
///
/// `max_retries` is the number of **additional** attempts after the first
/// failure (i.e. total attempts = 1 + max_retries). A response the server has
/// told us not to repeat (see [`is_retryable_status`]) ends the loop early, and
/// a `Retry-After` header overrides our own back-off so a throttled request
/// waits as long as the server asked instead of hammering it again.
///
/// On success the response body is returned as a `String`.
///
/// # Errors
///
/// Returns the last encountered error if all attempts are exhausted.
pub async fn get_with_retry(client: &Client, url: &str, max_retries: u32) -> Result<String> {
    let mut last_error: Option<anyhow::Error> = None;
    // Server-dictated wait for the *next* attempt, when it sent one.
    let mut next_delay: Option<Duration> = None;
    let mut attempts: u32 = 0;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            // Linear back-off: 500ms, 1000ms, 1500ms, … unless the server named
            // its own delay.
            let delay = next_delay
                .take()
                .unwrap_or_else(|| Duration::from_millis(500 * attempt as u64));
            tokio::time::sleep(delay).await;
        }
        attempts += 1;

        match client.get(url).send().await {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    last_error = Some(anyhow::anyhow!("HTTP error: {status}"));
                    if !is_retryable_status(status) {
                        break;
                    }
                    next_delay = retry_after_delay(response.headers());
                    continue;
                }

                // Capped rather than `response.text()`: this helper is the read
                // path for every archive index urx queries, none of which urx
                // controls, so an unbounded body must not be buffered in full.
                match read_body_capped(response, MAX_RESPONSE_BYTES).await {
                    Ok(text) => return Ok(text),
                    Err(e) => last_error = Some(e),
                }
            }
            Err(e) => last_error = Some(e.into()),
        }
    }

    let noun = if attempts == 1 { "attempt" } else { "attempts" };
    match last_error {
        Some(e) => Err(anyhow::anyhow!("Failed after {attempts} {noun}: {e}")),
        None => Err(anyhow::anyhow!("Failed after {attempts} {noun}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_after_delay_parses_seconds() {
        use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("30"));
        assert_eq!(retry_after_delay(&headers), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_retry_after_delay_caps_large_values() {
        use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("100000"));
        assert_eq!(retry_after_delay(&headers), Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_retry_after_delay_ignores_http_date_and_missing() {
        use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
        let empty = HeaderMap::new();
        assert_eq!(retry_after_delay(&empty), None);

        let mut headers = HeaderMap::new();
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
        );
        assert_eq!(retry_after_delay(&headers), None);
    }

    #[test]
    fn test_default_config() {
        let config = HttpClientConfig::default();
        assert_eq!(config.timeout, 30);
        assert!(!config.insecure);
        assert!(!config.random_agent);
        assert!(config.proxy.is_none());
        assert!(config.proxy_auth.is_none());
    }

    #[test]
    fn test_build_client_default() {
        let config = HttpClientConfig::default();
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_insecure() {
        let config = HttpClientConfig {
            insecure: true,
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_random_agent() {
        let config = HttpClientConfig {
            random_agent: true,
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_with_proxy() {
        let config = HttpClientConfig {
            proxy: Some("http://127.0.0.1:8080".to_string()),
            proxy_auth: Some("user:pass".to_string()),
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_with_proxy_no_auth() {
        let config = HttpClientConfig {
            proxy: Some("http://127.0.0.1:8080".to_string()),
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_with_custom_timeout() {
        let config = HttpClientConfig {
            timeout: 120,
            ..Default::default()
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_client_all_options() {
        let config = HttpClientConfig {
            timeout: 60,
            insecure: true,
            random_agent: true,
            proxy: Some("http://127.0.0.1:8080".to_string()),
            proxy_auth: Some("admin:secret".to_string()),
        };
        let client = config.build_client();
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_get_with_retry_success_first_try() {
        let mut mock_server = mockito::Server::new_async().await;
        let _m = mock_server
            .mock("GET", "/test")
            .with_status(200)
            .with_body("success")
            .create_async()
            .await;

        let client = Client::new();
        let url = format!("{}/test", mock_server.url());
        let result = get_with_retry(&client, &url, 3).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_get_with_retry_success_after_retry() {
        let mut mock_server = mockito::Server::new_async().await;

        // First attempt fails with 500
        let _m1 = mock_server
            .mock("GET", "/test")
            .with_status(500)
            .expect(1)
            .create_async()
            .await;

        // Second attempt succeeds
        let _m2 = mock_server
            .mock("GET", "/test")
            .with_status(200)
            .with_body("success")
            .expect(1)
            .create_async()
            .await;

        let client = Client::new();
        let url = format!("{}/test", mock_server.url());

        // We expect it to succeed eventually
        let result = get_with_retry(&client, &url, 3).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_get_with_retry_failure_max_retries() {
        let mut mock_server = mockito::Server::new_async().await;

        // Always fail. expects 2 calls (initial + 1 retry)
        let _m = mock_server
            .mock("GET", "/test")
            .with_status(500)
            .expect(2)
            .create_async()
            .await;

        let client = Client::new();
        let url = format!("{}/test", mock_server.url());

        // Max retries = 1. Total attempts = 2.
        let result = get_with_retry(&client, &url, 1).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed after 2 attempts"));
    }

    #[tokio::test]
    async fn test_get_with_retry_does_not_retry_client_errors() {
        let mut mock_server = mockito::Server::new_async().await;

        // A 404 is a definitive answer (for CDX indexes it means "no captures"),
        // so it must cost exactly one request no matter how many retries were
        // configured.
        let m = mock_server
            .mock("GET", "/missing")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;

        let client = Client::new();
        let url = format!("{}/missing", mock_server.url());
        let err = get_with_retry(&client, &url, 5).await.unwrap_err();

        assert!(err.to_string().contains("Failed after 1 attempt"), "{err}");
        m.assert();
    }

    #[tokio::test]
    async fn test_get_with_retry_retries_throttling() {
        let mut mock_server = mockito::Server::new_async().await;

        // 429 is transient, and the Retry-After the server names is what we
        // wait — capped, so a hostile value can't stall the run.
        let throttled = mock_server
            .mock("GET", "/slow")
            .with_status(429)
            .with_header("retry-after", "0")
            .expect(1)
            .create_async()
            .await;
        let ok = mock_server
            .mock("GET", "/slow")
            .with_status(200)
            .with_body("done")
            .expect(1)
            .create_async()
            .await;

        let client = Client::new();
        let url = format!("{}/slow", mock_server.url());
        assert_eq!(get_with_retry(&client, &url, 2).await.unwrap(), "done");
        throttled.assert();
        ok.assert();
    }

    #[tokio::test]
    async fn test_read_body_capped_stops_at_limit() {
        let mut mock_server = mockito::Server::new_async().await;
        let _m = mock_server
            .mock("GET", "/big")
            .with_status(200)
            .with_body("x".repeat(10_000))
            .create_async()
            .await;

        let resp = Client::new()
            .get(format!("{}/big", mock_server.url()))
            .send()
            .await
            .unwrap();
        let body = read_body_capped(resp, 128).await.unwrap();
        assert_eq!(body.len(), 128);
    }

    #[tokio::test]
    async fn test_get_with_retry_returns_whole_body_under_cap() {
        // get_with_retry reads through the capped path; a normal-sized body must
        // still come back byte-for-byte (including non-ASCII, which the chunked
        // read must not split incorrectly).
        let mut mock_server = mockito::Server::new_async().await;
        let payload = format!("{}\nsegunda linha — ção\n", "https://example.com/a");
        let _m = mock_server
            .mock("GET", "/body")
            .with_status(200)
            .with_body(&payload)
            .create_async()
            .await;

        let client = Client::new();
        let url = format!("{}/body", mock_server.url());
        assert_eq!(get_with_retry(&client, &url, 0).await.unwrap(), payload);
    }

    #[tokio::test]
    async fn test_get_with_retry_connection_error() {
        // Use a reserved port (0) which typically causes a connection error immediately
        let client = Client::new();
        let url = "http://127.0.0.1:0";

        let result = get_with_retry(&client, url, 1).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed after 2 attempts"));
    }
}
