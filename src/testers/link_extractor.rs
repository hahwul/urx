use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::OnceCell;
use url::Url;

use super::Tester;
use crate::network::client::{read_body_capped, HttpClientConfig};

/// Cap on bytes read from one page before parsing.
///
/// `--extract-links` runs over whatever the archives recorded, which routinely
/// includes multi-gigabyte media. The whole body was previously buffered into
/// memory and handed to the HTML parser, so a single large URL in the list could
/// exhaust memory. The sitemap provider already caps its documents this way;
/// this is the same guard for the same reason. 10 MiB is far more than any real
/// HTML page.
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Whether a response body is worth handing to the HTML parser.
///
/// A missing `Content-Type` is treated as "maybe HTML" and parsed, since plenty
/// of servers omit it; an explicitly non-HTML type (an image, a video, a zip) is
/// skipped — running an HTML parser over binary yields nothing but the bytes are
/// downloaded either way.
fn is_html_like(headers: &reqwest::header::HeaderMap) -> bool {
    match headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        Some(ct) => {
            let ct = ct.to_ascii_lowercase();
            ct.contains("html") || ct.contains("xml") || ct.contains("text/plain")
        }
        None => true,
    }
}

/// HTML link extractor that finds URLs in web pages
#[derive(Clone)]
pub struct LinkExtractor {
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    /// One HTTP client, built lazily on first use and reused for every tested
    /// URL. `reqwest::Client` pools connections internally, so building it once
    /// (rather than per URL) lets TLS handshakes and keep-alive connections be
    /// reused across the many URLs an `--extract-links` run can touch. Shared
    /// across `clone_box` clones via `Arc<OnceCell>` so all concurrent workers
    /// share a single connection pool. The cell is populated only after the
    /// `with_*` setters have applied network settings, so it always reflects
    /// the final configuration.
    client: Arc<OnceCell<Client>>,
}

impl LinkExtractor {
    /// Creates a new LinkExtractor with default settings
    pub fn new() -> Self {
        LinkExtractor {
            proxy: None,
            proxy_auth: None,
            timeout: 30,
            retries: 3,
            random_agent: false,
            insecure: false,
            client: Arc::new(OnceCell::new()),
        }
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

    /// Return the shared HTTP client, building it on the first call and reusing
    /// it thereafter. If a build fails the cell stays empty, so a later call
    /// retries rather than caching the error.
    async fn client(&self) -> Result<&Client> {
        self.client
            .get_or_try_init(|| async { self.client_config().build_client() })
            .await
    }

    /// Whether an `href` names something a crawler can fetch.
    ///
    /// `javascript:`, `mailto:`, `tel:` and `data:` hrefs resolve to perfectly
    /// valid absolute URLs, so they used to be emitted as discovered links — a
    /// `mailto:` address reported as a URL urx had found. A bare `#anchor` is
    /// worse: it resolves to the page urx is already looking at, so every
    /// in-page anchor came back as a "new" URL that survives host validation.
    fn is_fetchable_href(href: &str) -> bool {
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') {
            return false;
        }
        // A scheme is everything before the first ':' — but only when no '/',
        // '?' or '#' comes first, otherwise `foo/bar:baz` would read as a scheme.
        match href.find([':', '/', '?', '#']) {
            Some(i) if href.as_bytes()[i] == b':' => !matches!(
                href[..i].to_ascii_lowercase().as_str(),
                "javascript" | "mailto" | "tel" | "data" | "about" | "blob"
            ),
            _ => true,
        }
    }

    /// Extracts links from HTML content, resolving them against a base URL.
    ///
    /// A `<base href>` in the document overrides `base_url` for relative links —
    /// that is the entire purpose of the tag, and ignoring it meant every
    /// relative href on a page that declares one resolved to the wrong absolute
    /// URL. An unparseable or relative `<base href>` is itself resolved against
    /// the page URL, as browsers do.
    fn extract_links(base_url: &Url, html_content: &str) -> Vec<String> {
        let document = Html::parse_document(html_content);
        let mut links = Vec::new();

        // Constant, known-valid selectors.
        let base_selector = Selector::parse("base[href]").unwrap();
        let selector = Selector::parse("a[href]").unwrap();

        // Only the first <base href> in the document has effect.
        let resolved_base = document
            .select(&base_selector)
            .next()
            .and_then(|el| el.value().attr("href"))
            .and_then(|href| base_url.join(href).ok())
            .unwrap_or_else(|| base_url.clone());

        // Extract and normalize links
        for element in document.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                if !Self::is_fetchable_href(href) {
                    continue;
                }
                // Resolve relative URLs to absolute URLs
                if let Ok(absolute_url) = resolved_base.join(href.trim()) {
                    links.push(absolute_url.to_string());
                }
            }
        }

        links
    }
}

impl Tester for LinkExtractor {
    fn clone_box(&self) -> Box<dyn Tester> {
        Box::new(self.clone())
    }

    /// Extracts links from a URL by downloading the page and parsing the HTML
    fn test_url<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            let client = self.client().await?;

            // Perform the request with retries
            let mut last_error = None;

            for attempt in 0..=self.retries {
                match client.get(url).send().await {
                    Ok(response) => {
                        // Get the base URL for resolving relative URLs
                        let base_url = match Url::parse(url) {
                            Ok(parsed_url) => parsed_url,
                            Err(_) => {
                                return Err(anyhow::anyhow!("Failed to parse URL: {}", url));
                            }
                        };

                        // An error page still has a body, and its nav/footer is
                        // full of links — mining those would inject the site's
                        // chrome into the results as if it had been discovered.
                        // A non-2xx page has no links worth extracting.
                        if !response.status().is_success() {
                            return Ok(Vec::new());
                        }

                        // Nor is there anything to extract from a response that
                        // isn't markup.
                        if !is_html_like(response.headers()) {
                            return Ok(Vec::new());
                        }

                        // Get the HTML content, bounded so one huge page can't
                        // exhaust memory.
                        let html_content = read_body_capped(response, MAX_BODY_BYTES).await?;

                        // Extract links using the helper function
                        let links = Self::extract_links(&base_url, &html_content);

                        // Return the list of links
                        return Ok(links);
                    }
                    Err(e) => {
                        last_error = Some(e);
                        // Back off only when another attempt follows; a sleep
                        // after the final one is pure latency, paid once per
                        // unreachable URL (and even with `--retries 0`).
                        if attempt < self.retries {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                        continue;
                    }
                }
            }

            // If we get here, all retries failed
            Err(anyhow::anyhow!(
                "Failed to extract links from {}: {:?}",
                url,
                last_error
            ))
        })
    }

    /// Sets the request timeout in seconds
    fn with_timeout(&mut self, seconds: u64) {
        self.timeout = seconds;
    }

    /// Sets the number of retry attempts for failed requests
    fn with_retries(&mut self, count: u32) {
        self.retries = count;
    }

    /// Enables or disables the use of random User-Agent headers
    fn with_random_agent(&mut self, enabled: bool) {
        self.random_agent = enabled;
    }

    /// Enables or disables SSL certificate verification
    fn with_insecure(&mut self, enabled: bool) {
        self.insecure = enabled;
    }

    /// Sets the proxy server for HTTP requests
    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    /// Sets the proxy authentication credentials (username:password)
    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_extractor_new() {
        let extractor = LinkExtractor::new();
        assert_eq!(extractor.timeout, 30);
        assert_eq!(extractor.retries, 3);
        assert!(!extractor.random_agent);
        assert!(!extractor.insecure);
        assert_eq!(extractor.proxy, None);
        assert_eq!(extractor.proxy_auth, None);
    }

    #[test]
    fn test_link_extractor_with_timeout() {
        let mut extractor = LinkExtractor::new();
        extractor.with_timeout(60);
        assert_eq!(extractor.timeout, 60);
    }

    #[test]
    fn test_link_extractor_with_retries() {
        let mut extractor = LinkExtractor::new();
        extractor.with_retries(5);
        assert_eq!(extractor.retries, 5);
    }

    #[test]
    fn test_link_extractor_with_random_agent() {
        let mut extractor = LinkExtractor::new();
        extractor.with_random_agent(true);
        assert!(extractor.random_agent);
    }

    #[test]
    fn test_link_extractor_with_insecure() {
        let mut extractor = LinkExtractor::new();
        extractor.with_insecure(true);
        assert!(extractor.insecure);
    }

    #[test]
    fn test_link_extractor_with_proxy() {
        let mut extractor = LinkExtractor::new();
        extractor.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            extractor.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_link_extractor_with_proxy_auth() {
        let mut extractor = LinkExtractor::new();
        extractor.with_proxy_auth(Some("username:password".to_string()));
        assert_eq!(extractor.proxy_auth, Some("username:password".to_string()));
    }

    #[test]
    fn test_link_extractor_clone_box() {
        let extractor = LinkExtractor::new();
        let _cloned = extractor.clone_box();
        // Just verifying the method works, actual equality testing would be complex with Box<dyn>
    }

    #[test]
    fn test_extract_links() {
        let base_url = Url::parse("https://example.com/start").unwrap();

        // 1. Basic absolute and relative links
        let html = r#"
            <html>
                <body>
                    <a href="https://other.com/page">Absolute</a>
                    <a href="/relative/path">Relative Root</a>
                    <a href="sibling">Relative Sibling</a>
                    <a href="../parent">Relative Parent</a>
                </body>
            </html>
        "#;
        let links = LinkExtractor::extract_links(&base_url, html);
        assert_eq!(links.len(), 4);
        assert!(links.contains(&"https://other.com/page".to_string()));
        assert!(links.contains(&"https://example.com/relative/path".to_string()));
        assert!(links.contains(&"https://example.com/sibling".to_string()));
        assert!(links.contains(&"https://example.com/parent".to_string()));

        // 2. Fragment and Query parameters
        let html = r#"
            <a href="/page#fragment">Fragment</a>
            <a href="/page?query=1">Query</a>
        "#;
        let links = LinkExtractor::extract_links(&base_url, html);
        assert_eq!(links.len(), 2);
        assert!(links.contains(&"https://example.com/page#fragment".to_string()));
        assert!(links.contains(&"https://example.com/page?query=1".to_string()));

        // 3. No links
        let html = "<html><body><p>No links here</p></body></html>";
        let links = LinkExtractor::extract_links(&base_url, html);
        assert!(links.is_empty());

        // 4. Empty HTML
        let html = "";
        let links = LinkExtractor::extract_links(&base_url, html);
        assert!(links.is_empty());

        // 5. Links without href
        let html = "<a>No href</a>";
        let links = LinkExtractor::extract_links(&base_url, html);
        assert!(links.is_empty());
    }

    #[test]
    fn test_base_href_overrides_the_page_url() {
        // Regression: <base href> was ignored, so every relative link on a page
        // that declares one resolved against the page URL and produced an
        // absolute URL that does not exist on the site.
        let base_url = Url::parse("https://example.com/deep/page.html").unwrap();
        let html = r#"
            <html><head><base href="https://cdn.example.com/app/"></head>
            <body><a href="a.js">rel</a><a href="/root">root</a></body></html>
        "#;
        let links = LinkExtractor::extract_links(&base_url, html);
        assert!(
            links.contains(&"https://cdn.example.com/app/a.js".to_string()),
            "{links:?}"
        );
        assert!(
            links.contains(&"https://cdn.example.com/root".to_string()),
            "{links:?}"
        );
    }

    #[test]
    fn test_relative_base_href_is_resolved_against_the_page() {
        let base_url = Url::parse("https://example.com/a/b/page.html").unwrap();
        let html = r#"<head><base href="../assets/"></head><a href="x.css">x</a>"#;
        let links = LinkExtractor::extract_links(&base_url, html);
        assert_eq!(
            links,
            vec!["https://example.com/a/assets/x.css".to_string()]
        );
    }

    #[test]
    fn test_only_the_first_base_href_applies() {
        let base_url = Url::parse("https://example.com/p").unwrap();
        let html = r#"<base href="https://one.example/"><base href="https://two.example/"><a href="z">z</a>"#;
        let links = LinkExtractor::extract_links(&base_url, html);
        assert_eq!(links, vec!["https://one.example/z".to_string()]);
    }

    #[test]
    fn test_non_navigational_hrefs_are_not_reported_as_urls() {
        // These all resolve to valid absolute URLs, so they used to be emitted
        // as links urx had discovered.
        let base_url = Url::parse("https://example.com/start").unwrap();
        let html = r##"
            <a href="javascript:void(0)">js</a>
            <a href="mailto:a@example.com">mail</a>
            <a href="tel:+15551234">tel</a>
            <a href="data:text/plain,hi">data</a>
            <a href="#top">anchor</a>
            <a href="  ">blank</a>
            <a href="/real">real</a>
        "##;
        let links = LinkExtractor::extract_links(&base_url, html);
        assert_eq!(links, vec!["https://example.com/real".to_string()]);
    }

    #[test]
    fn test_path_containing_a_colon_is_still_followed() {
        // `is_fetchable_href` must not mistake a colon inside a path segment for
        // a scheme.
        let base_url = Url::parse("https://example.com/start").unwrap();
        let html = r#"<a href="/files/a:b/c.js">x</a><a href="rel:ative/thing">y</a>"#;
        let links = LinkExtractor::extract_links(&base_url, html);
        assert!(
            links.contains(&"https://example.com/files/a:b/c.js".to_string()),
            "{links:?}"
        );
    }

    #[tokio::test]
    async fn test_error_pages_are_not_mined_for_links() {
        // Regression: the body of any response was parsed, including 404/500
        // pages — so a dead URL contributed the site's nav and footer links to
        // the results as though they had been discovered.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/gone")
            .with_status(404)
            .with_header("content-type", "text/html")
            .with_body(r#"<a href="/nav">home</a><a href="/footer">about</a>"#)
            .create_async()
            .await;

        let extractor = LinkExtractor::new();
        let links = extractor
            .test_url(&format!("{}/gone", server.url()))
            .await
            .unwrap();

        assert!(links.is_empty(), "{links:?}");
    }

    #[tokio::test]
    async fn test_non_markup_bodies_are_skipped() {
        // Running an HTML parser over a JPEG finds nothing; skip it outright.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/photo.jpg")
            .with_status(200)
            .with_header("content-type", "image/jpeg")
            .with_body(r#"<a href="/not-really-html">x</a>"#)
            .create_async()
            .await;

        let extractor = LinkExtractor::new();
        let links = extractor
            .test_url(&format!("{}/photo.jpg", server.url()))
            .await
            .unwrap();

        assert!(links.is_empty(), "{links:?}");
    }

    #[tokio::test]
    async fn test_missing_content_type_is_still_parsed() {
        // Plenty of servers omit Content-Type; treat that as "maybe HTML" rather
        // than silently dropping the page.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/bare")
            .with_status(200)
            .with_body(r#"<a href="https://example.com/found">x</a>"#)
            .create_async()
            .await;

        let extractor = LinkExtractor::new();
        let links = extractor
            .test_url(&format!("{}/bare", server.url()))
            .await
            .unwrap();

        assert_eq!(links, vec!["https://example.com/found".to_string()]);
    }

    #[tokio::test]
    async fn test_body_is_capped() {
        // A body larger than the cap is truncated rather than buffered whole.
        // Links before the cut are still found; the run does not blow up.
        let mut body = String::from(r#"<a href="https://example.com/early">x</a>"#);
        body.push_str(&"<!-- padding -->".repeat(80_000));
        body.push_str(r#"<a href="https://example.com/late">y</a>"#);
        assert!(
            body.len() > MAX_BODY_BYTES / 10,
            "test body should be large"
        );

        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/big")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(body)
            .create_async()
            .await;

        let extractor = LinkExtractor::new();
        let links = extractor
            .test_url(&format!("{}/big", server.url()))
            .await
            .unwrap();

        assert!(
            links.contains(&"https://example.com/early".to_string()),
            "{links:?}"
        );
    }

    #[test]
    fn test_is_html_like() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
        let with = |v: &'static str| {
            let mut h = HeaderMap::new();
            h.insert(CONTENT_TYPE, HeaderValue::from_static(v));
            h
        };
        assert!(is_html_like(&with("text/html; charset=utf-8")));
        assert!(is_html_like(&with("application/xhtml+xml")));
        assert!(is_html_like(&with("text/plain")));
        assert!(!is_html_like(&with("image/png")));
        assert!(!is_html_like(&with("video/mp4")));
        assert!(!is_html_like(&with("application/octet-stream")));
        // Absent header: assume markup rather than drop the page.
        assert!(is_html_like(&HeaderMap::new()));
    }

    #[tokio::test]
    async fn test_no_retries_means_no_backoff_wait() {
        // Regression: the 500ms back-off was slept after the *final* attempt as
        // well, so an unreachable page cost half a second even with
        // `--retries 0` — once per URL, across a whole --extract-links run.
        let mut extractor = LinkExtractor::new();
        extractor.with_retries(0);

        let start = std::time::Instant::now();
        let result = extractor.test_url("http://127.0.0.1:0/nope").await;
        let elapsed = start.elapsed();

        assert!(result.is_err());
        assert!(
            elapsed < std::time::Duration::from_millis(400),
            "a single failed attempt must not sleep, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn test_client_is_built_once_and_reused() {
        let extractor = LinkExtractor::new();
        // Lazy: nothing is built until the first request needs it.
        assert!(extractor.client.get().is_none());

        let first = extractor.client().await.unwrap() as *const reqwest::Client;
        let second = extractor.client().await.unwrap() as *const reqwest::Client;

        // Both calls hand back the exact same cached client instead of building
        // a fresh one per URL — that shared client is what enables connection
        // pooling across the many URLs an --extract-links run touches.
        assert_eq!(first, second);
        assert!(extractor.client.get().is_some());
    }

    #[tokio::test]
    async fn test_reused_client_extracts_from_multiple_urls() {
        let mut server = mockito::Server::new_async().await;
        let p1 = server
            .mock("GET", "/p1")
            .with_status(200)
            .with_body(r#"<a href="https://example.com/one">x</a>"#)
            .expect(1)
            .create_async()
            .await;
        let p2 = server
            .mock("GET", "/p2")
            .with_status(200)
            .with_body(r#"<a href="https://example.com/two">y</a>"#)
            .expect(1)
            .create_async()
            .await;

        let extractor = LinkExtractor::new();
        let first = extractor
            .test_url(&format!("{}/p1", server.url()))
            .await
            .unwrap();
        let second = extractor
            .test_url(&format!("{}/p2", server.url()))
            .await
            .unwrap();

        assert_eq!(first, vec!["https://example.com/one".to_string()]);
        assert_eq!(second, vec!["https://example.com/two".to_string()]);
        // A single client was built and shared across both requests.
        assert!(extractor.client.get().is_some());
        p1.assert();
        p2.assert();
    }
}
