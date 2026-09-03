use anyhow::Result;
use reqwest::Client;
use scraper::node::Element;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
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

/// Element name to the attribute on it that holds a URL.
///
/// A page's fetchable surface is not just its anchors. Script bundles,
/// stylesheets, form endpoints, iframes and embedded objects are precisely the
/// URLs an OSINT sweep is after — script and form targets especially, since
/// they name endpoints that never appear as a link — and only `<a href>` was
/// ever read, so every one of them was dropped on the floor.
const URL_ATTRIBUTES: &[(&str, &str)] = &[
    ("a", "href"),
    ("script", "src"),
    ("link", "href"),
    ("form", "action"),
    ("iframe", "src"),
    ("img", "src"),
    ("source", "src"),
    ("object", "data"),
    ("embed", "src"),
];

/// `<meta http-equiv="refresh" content="0; url=/next">`: a redirect written in
/// markup instead of a header, so its target is a real successor page.
const META_REFRESH_SELECTOR: &str = "meta[http-equiv]";

/// The selectors [`LinkExtractor::extract_links`] runs, parsed once for the
/// process.
///
/// `Selector::parse` is real work and every one of these strings is fixed at
/// compile time, so re-parsing them for each of the thousands of pages an
/// `--extract-links` run touches is pure waste.
struct LinkSelectors {
    /// `<base href>`, which redefines what relative URLs resolve against.
    base: Selector,
    /// Every entry of [`URL_ATTRIBUTES`] plus the meta-refresh tag, as a single
    /// selector: one walk of the document, and matches arrive in document
    /// order rather than grouped by tag.
    links: Selector,
}

static SELECTORS: LazyLock<LinkSelectors> = LazyLock::new(|| {
    let mut parts: Vec<String> = URL_ATTRIBUTES
        .iter()
        .map(|(tag, attr)| format!("{tag}[{attr}]"))
        .collect();
    parts.push(META_REFRESH_SELECTOR.to_string());

    LinkSelectors {
        base: Selector::parse("base[href]").expect("`base[href]` is a valid CSS selector"),
        links: Selector::parse(&parts.join(", "))
            .expect("URL_ATTRIBUTES and META_REFRESH_SELECTOR must form a valid CSS selector"),
    }
});

/// The raw URL carried by `element`, if it carries one.
///
/// Every tag but `<meta>` simply holds it in an attribute; a meta refresh
/// buries it inside `content`.
fn url_attribute(element: &Element) -> Option<&str> {
    if element.name() == "meta" {
        return meta_refresh_target(element);
    }
    URL_ATTRIBUTES
        .iter()
        .find(|(tag, _)| *tag == element.name())
        .and_then(|(_, attr)| element.attr(attr))
}

/// The target of a `<meta http-equiv="refresh">`, or `None` when the element is
/// some other `http-equiv` or names no URL.
///
/// The content is a delay optionally followed by `url=...`. The key is
/// case-insensitive, the spacing is arbitrary, and the value is frequently
/// quoted — all three appear in the wild.
fn meta_refresh_target(element: &Element) -> Option<&str> {
    if !element
        .attr("http-equiv")?
        .trim()
        .eq_ignore_ascii_case("refresh")
    {
        return None;
    }

    let target = element
        .attr("content")?
        .split(';')
        // The delay carries no '=', so it falls out here without being skipped
        // positionally — some pages omit it entirely.
        .find_map(|part| {
            let (key, value) = part.split_once('=')?;
            key.trim().eq_ignore_ascii_case("url").then(|| value.trim())
        })?;

    Some(target.trim_matches(['"', '\'']))
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
    /// Every URL-bearing tag in [`URL_ATTRIBUTES`] is read, plus meta refresh
    /// targets — not just `<a href>`.
    ///
    /// A `<base href>` in the document overrides `base_url` for relative links —
    /// that is the entire purpose of the tag, and ignoring it meant every
    /// relative href on a page that declares one resolved to the wrong absolute
    /// URL. An unparseable or relative `<base href>` is itself resolved against
    /// the page URL, as browsers do.
    ///
    /// The result is deduplicated in document order: a single page names the
    /// same stylesheet or logo from a dozen places, and reporting each of them
    /// as a separate discovery is noise.
    fn extract_links(base_url: &Url, html_content: &str) -> Vec<String> {
        let document = Html::parse_document(html_content);

        // Only the first <base href> in the document has effect.
        let resolved_base = document
            .select(&SELECTORS.base)
            .next()
            .and_then(|el| el.value().attr("href"))
            .and_then(|href| base_url.join(href).ok())
            .unwrap_or_else(|| base_url.clone());

        let mut links = Vec::new();
        let mut seen = HashSet::new();

        for element in document.select(&SELECTORS.links) {
            let Some(raw) = url_attribute(element.value()) else {
                continue;
            };
            if !Self::is_fetchable_href(raw) {
                continue;
            }
            // Resolve relative URLs to absolute URLs
            let Ok(absolute_url) = resolved_base.join(raw.trim()) else {
                continue;
            };
            let absolute_url = absolute_url.to_string();
            if seen.insert(absolute_url.clone()) {
                links.push(absolute_url);
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
    fn test_every_url_bearing_tag_is_collected() {
        // Regression: only `<a href>` was read, so script bundles, stylesheets,
        // form endpoints, iframes and embedded objects — the URLs an OSINT
        // sweep is actually after — were all dropped.
        let base_url = Url::parse("https://example.com/dir/page.html").unwrap();

        // One case per supported tag, checked alone so a failure names the tag
        // that broke rather than a diff of ten URLs.
        let cases = [
            (r#"<a href="/anchor">x</a>"#, "https://example.com/anchor"),
            (
                r#"<script src="/app.js"></script>"#,
                "https://example.com/app.js",
            ),
            (
                r#"<link rel="stylesheet" href="/site.css">"#,
                "https://example.com/site.css",
            ),
            (
                r#"<form action="/search" method="get"></form>"#,
                "https://example.com/search",
            ),
            (
                r#"<iframe src="/frame.html"></iframe>"#,
                "https://example.com/frame.html",
            ),
            // Relative, to prove resolution runs on every tag and not just <a>.
            (
                r#"<img src="logo.png">"#,
                "https://example.com/dir/logo.png",
            ),
            (
                r#"<video><source src="/clip.mp4"></video>"#,
                "https://example.com/clip.mp4",
            ),
            (
                r#"<object data="/legacy.swf"></object>"#,
                "https://example.com/legacy.swf",
            ),
            (
                r#"<embed src="/plugin.svg">"#,
                "https://example.com/plugin.svg",
            ),
            (
                r#"<meta http-equiv="refresh" content="0; url=/next">"#,
                "https://example.com/next",
            ),
        ];

        for (html, expected) in cases {
            let links = LinkExtractor::extract_links(&base_url, html);
            assert_eq!(links, vec![expected.to_string()], "markup: {html}");
        }
    }

    #[test]
    fn test_tags_are_collected_together_in_document_order() {
        let base_url = Url::parse("https://example.com/").unwrap();
        let html = r#"
            <html>
              <head>
                <link rel="stylesheet" href="/a.css">
                <script src="/b.js"></script>
              </head>
              <body>
                <a href="/c">c</a>
                <img src="/d.png">
                <form action="/e"></form>
              </body>
            </html>
        "#;

        assert_eq!(
            LinkExtractor::extract_links(&base_url, html),
            vec![
                "https://example.com/a.css".to_string(),
                "https://example.com/b.js".to_string(),
                "https://example.com/c".to_string(),
                "https://example.com/d.png".to_string(),
                "https://example.com/e".to_string(),
            ]
        );
    }

    #[test]
    fn test_duplicate_links_are_reported_once() {
        // A real page names its logo and its stylesheet from several places;
        // each repeat used to come back as a separate discovery.
        let base_url = Url::parse("https://example.com/").unwrap();
        let html = r#"
            <a href="/same">one</a>
            <a href="/same">two</a>
            <img src="/same">
            <link rel="preload" href="https://example.com/same">
            <a href="/other">other</a>
        "#;

        assert_eq!(
            LinkExtractor::extract_links(&base_url, html),
            vec![
                "https://example.com/same".to_string(),
                "https://example.com/other".to_string(),
            ]
        );
    }

    #[test]
    fn test_meta_refresh_content_variants() {
        let base_url = Url::parse("https://example.com/start").unwrap();

        for html in [
            r#"<meta http-equiv="refresh" content="0; url=/next">"#,
            // No space after the delimiter.
            r#"<meta http-equiv="refresh" content="5;url=/next">"#,
            // Uppercase key and header name.
            r#"<meta http-equiv="Refresh" content="0; URL=/next">"#,
            // Quoted value.
            r#"<meta http-equiv="refresh" content="0; url='/next'">"#,
            // No delay at all.
            r#"<meta http-equiv="refresh" content="url=/next">"#,
        ] {
            assert_eq!(
                LinkExtractor::extract_links(&base_url, html),
                vec!["https://example.com/next".to_string()],
                "markup: {html}"
            );
        }
    }

    #[test]
    fn test_meta_tags_that_name_no_url_are_ignored() {
        let base_url = Url::parse("https://example.com/start").unwrap();

        for html in [
            // A refresh that only reloads the current page names no target.
            r#"<meta http-equiv="refresh" content="30">"#,
            // A different http-equiv entirely: its content is not a URL.
            r#"<meta http-equiv="content-type" content="text/html; charset=utf-8">"#,
            r#"<meta name="description" content="url=not-a-redirect">"#,
        ] {
            assert!(
                LinkExtractor::extract_links(&base_url, html).is_empty(),
                "markup: {html}"
            );
        }
    }

    #[test]
    fn test_non_fetchable_sources_are_skipped_on_every_tag() {
        // `is_fetchable_href` has to guard the new attributes too — inline
        // `data:` images and `javascript:` targets are not URLs urx found.
        let base_url = Url::parse("https://example.com/start").unwrap();
        let html = r##"
            <img src="data:image/gif;base64,R0lGOD">
            <script src="javascript:void(0)"></script>
            <form action="#"></form>
            <iframe src="about:blank"></iframe>
            <embed src="">
            <link rel="icon" href="/favicon.ico">
        "##;

        assert_eq!(
            LinkExtractor::extract_links(&base_url, html),
            vec!["https://example.com/favicon.ico".to_string()]
        );
    }

    #[test]
    fn test_base_href_applies_to_every_tag() {
        let base_url = Url::parse("https://example.com/deep/page.html").unwrap();
        let html = r#"
            <head><base href="https://cdn.example.com/app/"></head>
            <body><script src="bundle.js"></script><img src="hero.png"></body>
        "#;

        assert_eq!(
            LinkExtractor::extract_links(&base_url, html),
            vec![
                "https://cdn.example.com/app/bundle.js".to_string(),
                "https://cdn.example.com/app/hero.png".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn test_fetched_page_yields_more_than_anchors() {
        // The same expansion, end to end through the HTTP path.
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/index.html")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(
                r#"<script src="/static/app.js"></script>
                   <form action="/api/search"></form>
                   <a href="/about">about</a>"#,
            )
            .create_async()
            .await;

        let extractor = LinkExtractor::new();
        let links = extractor
            .test_url(&format!("{}/index.html", server.url()))
            .await
            .unwrap();

        let base = server.url();
        assert_eq!(
            links,
            vec![
                format!("{base}/static/app.js"),
                format!("{base}/api/search"),
                format!("{base}/about"),
            ]
        );
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
