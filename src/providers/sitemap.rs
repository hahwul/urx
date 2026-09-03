use anyhow::Result;
use async_recursion::async_recursion;
use async_trait::async_trait;
use reqwest::Client;
use roxmltree::Document;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::network::client::{read_body_capped, HttpClientConfig};
use crate::network::RateLimiter;
use crate::providers::{Provider, UrlRecord};

/// Max nesting depth for sitemap-index → sitemap recursion. A hostile or
/// misconfigured index can chain or cycle indefinitely; this bounds it.
const MAX_SITEMAP_DEPTH: usize = 10;

/// Overall cap on URLs collected from one domain's sitemaps, so a giant (or
/// adversarial) sitemap tree can't grow memory without bound.
const MAX_SITEMAP_URLS: usize = 1_000_000;

/// Cap on the raw bytes read from a single sitemap document. Without it, a
/// hostile or misconfigured endpoint could stream gigabytes into memory before
/// any URL parsing happens (the per-URL cap only bounds the *parsed* output).
const MAX_SITEMAP_BYTES: usize = 50 * 1024 * 1024;

/// Whether a `<loc>` in a sitemap index may be followed from `parent`.
///
/// sitemaps.org requires every entry in a sitemap to reside on the same host as
/// the sitemap itself, and urx never enforced it: a `<sitemapindex>` could point
/// `<loc>` at any host at all and urx would fetch it and report what it found.
/// That let the document under audit redirect the provider away from the target
/// — including at hosts only the machine running urx can reach — and surface
/// their contents as URLs "discovered" for the domain.
///
/// Only the host and any explicit port must match; a sitemap served over HTTPS
/// listing its children over HTTP (or the reverse) is common enough that scheme
/// is not compared. `Url::port` reports `None` for a scheme's default port, so
/// comparing it keeps `https://h` and `https://h:443` equal while still telling
/// `h` apart from `h:8443`.
fn same_host_as_parent(parent: &str, child: &str) -> bool {
    match (url::Url::parse(parent), url::Url::parse(child)) {
        (Ok(p), Ok(c)) => match (p.host_str(), c.host_str()) {
            (Some(ph), Some(ch)) => ph.eq_ignore_ascii_case(ch) && p.port() == c.port(),
            _ => false,
        },
        _ => false,
    }
}

/// State threaded through one domain's sitemap walk.
#[derive(Default)]
struct Walk {
    /// Sitemap URLs already fetched, so a cycle (`A → A`, `A → B → A`) or a
    /// sitemap reachable from two entry points is fetched at most once.
    visited: HashSet<String>,
    /// Set once any request produced a definitive answer — a body we read, or
    /// an HTTP status saying there is nothing at this location. This is what
    /// separates "this host has no sitemap" (a successful scan with zero URLs)
    /// from "we never reached this host at all" (an error the run must report).
    answered: bool,
    /// The last transport-level failure, surfaced when nothing ever answered.
    failure: Option<anyhow::Error>,
}

#[derive(Clone)]
pub struct SitemapProvider {
    timeout: Duration,
    retries: u32,
    random_agent: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    insecure: bool,
    rate_limit: Option<RateLimiter>,
}

impl SitemapProvider {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            retries: 3,
            random_agent: false,
            proxy: None,
            proxy_auth: None,
            insecure: false,
            rate_limit: None,
        }
    }

    fn client_config(&self) -> HttpClientConfig {
        HttpClientConfig {
            timeout: self.timeout.as_secs(),
            insecure: self.insecure,
            random_agent: self.random_agent,
            proxy: self.proxy.clone(),
            proxy_auth: self.proxy_auth.clone(),
        }
    }

    /// Build the HTTP client via the shared config so it always sends a
    /// User-Agent (a UA-less request is rejected with 400 by some servers).
    fn build_client(&self) -> Result<Client> {
        self.client_config().build_client()
    }

    /// Recursively fetch and parse a sitemap (or sitemap index).
    ///
    /// `walk.visited` records already-fetched sitemap URLs to break cycles
    /// (`A → A`, `A → B → A`); `depth` bounds straight-line nesting; and the
    /// caller stops feeding work once [`MAX_SITEMAP_URLS`] is reached. Together
    /// these stop a malicious sitemap from hanging the run or exhausting memory.
    #[async_recursion]
    async fn parse_sitemap(
        client: &Client,
        sitemap_url: &str,
        depth: usize,
        walk: &mut Walk,
        limiter: Option<&RateLimiter>,
    ) -> Result<Vec<String>> {
        if depth > MAX_SITEMAP_DEPTH {
            return Ok(Vec::new());
        }
        // A sitemap URL we've already fetched in this walk is a cycle — skip it.
        if !walk.visited.insert(sitemap_url.to_string()) {
            return Ok(Vec::new());
        }

        // Pace nested-sitemap fetches: a sitemap index can chain to many child
        // sitemaps, so honor --rate-limit before each request.
        if let Some(rl) = limiter {
            rl.acquire().await;
        }
        // Sitemap discovery is best-effort and speculative: most of the
        // candidate locations we probe don't exist, so a single location that
        // isn't there means "nothing here", not an error that should sink the
        // whole provider — the `visited` insert above already stops us re-asking.
        //
        // A transport failure is different in kind from a non-200 and is
        // recorded as such: if *no* location ever answered, the caller turns
        // that into a real error rather than an empty success.
        let resp = match client.get(sitemap_url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                walk.failure = Some(e.into());
                return Ok(Vec::new());
            }
        };
        if !resp.status().is_success() {
            walk.answered = true;
            return Ok(Vec::new());
        }

        // Only a genuine text sitemap should fall back to line-based parsing.
        // Decide that from the URL suffix / Content-Type *before* consuming the
        // body, so an XML endpoint that returns an HTML error page isn't mined
        // for stray `http` lines.
        let is_text_sitemap = sitemap_url.to_ascii_lowercase().ends_with(".txt")
            || resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|ct| ct.to_ascii_lowercase().contains("text/plain"))
                .unwrap_or(false);

        let content = match read_body_capped(resp, MAX_SITEMAP_BYTES).await {
            Ok(content) => content,
            // A body that dies mid-stream yields no usable URLs; same
            // best-effort reasoning as above.
            Err(e) => {
                walk.failure = Some(e);
                return Ok(Vec::new());
            }
        };
        walk.answered = true;
        let mut urls = Vec::new();

        match Document::parse(&content) {
            Ok(doc) => {
                // Check if this is a sitemap index file
                let is_sitemap_index = doc.root_element().has_tag_name("sitemapindex");

                if is_sitemap_index {
                    // This is a sitemap index file, so we need to process each sitemap
                    for sitemap_node in doc.descendants().filter(|n| n.has_tag_name("sitemap")) {
                        if urls.len() >= MAX_SITEMAP_URLS {
                            break;
                        }
                        if let Some(loc_node) =
                            sitemap_node.descendants().find(|n| n.has_tag_name("loc"))
                        {
                            if let Some(nested_sitemap_url) = loc_node.text() {
                                // A child sitemap must live on the same host as
                                // the index that names it; anything else is the
                                // document steering us off the target.
                                if !same_host_as_parent(sitemap_url, nested_sitemap_url.trim()) {
                                    continue;
                                }
                                // Recursively fetch and parse nested sitemaps.
                                // Box::pin the future to avoid infinitely sized futures.
                                let nested_urls = Box::pin(Self::parse_sitemap(
                                    client,
                                    nested_sitemap_url.trim(),
                                    depth + 1,
                                    walk,
                                    limiter,
                                ))
                                .await?;
                                urls.extend(nested_urls);
                            }
                        }
                    }
                } else {
                    // This is a regular sitemap file
                    for url_node in doc.descendants().filter(|n| n.has_tag_name("url")) {
                        if urls.len() >= MAX_SITEMAP_URLS {
                            break;
                        }
                        if let Some(loc_node) =
                            url_node.descendants().find(|n| n.has_tag_name("loc"))
                        {
                            if let Some(url) = loc_node.text() {
                                // Trimmed, like the sitemap-index branch above:
                                // XML lets a pretty-printed
                                // `<loc>\n  https://…\n</loc>` carry the
                                // surrounding indentation into the text node,
                                // and the newlines rode along into the emitted
                                // URL — which then failed host validation, so
                                // every URL of such a sitemap was silently lost.
                                let url = url.trim();
                                if !url.is_empty() {
                                    urls.push(url.to_string());
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // XML parse failed. Only treat it as a plain-text URL list when
                // the source actually is text (a .txt sitemap or text/plain);
                // otherwise this was an HTML/error page and yields no URLs.
                if is_text_sitemap {
                    for line in content.lines() {
                        if urls.len() >= MAX_SITEMAP_URLS {
                            break;
                        }
                        let line = line.trim();
                        if line.starts_with("http") {
                            urls.push(line.to_string());
                        }
                    }
                }
            }
        }

        Ok(urls)
    }
}

#[async_trait]
impl Provider for SitemapProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlRecord>>> + Send + 'a>> {
        Box::pin(async move {
            let client = self.build_client()?;
            let limiter = self.rate_limit.as_ref();
            let mut urls = Vec::new();
            // Shared across all candidate locations so a sitemap reachable from
            // more than one entry point is fetched at most once, and so
            // reachability is judged over the whole set of probes.
            let mut walk = Walk::default();

            // Try common sitemap locations
            let sitemap_urls = vec![
                format!("https://{}/sitemap.xml", domain),
                format!("https://{}/sitemap_index.xml", domain),
                format!("https://{}/sitemap.txt", domain),
                format!("http://{}/sitemap.xml", domain),
                format!("http://{}/sitemap_index.xml", domain),
                format!("http://{}/sitemap.txt", domain),
            ];

            // `parse_sitemap` does the request itself and reports "nothing here"
            // for a missing or unreachable location, so probing first would just
            // fetch every sitemap that *does* exist twice — doubling the requests
            // aimed at the target and the bytes pulled for a large sitemap.
            // It also paces each request through the limiter, so this loop's up
            // to six candidate locations stay rate-limited.
            for sitemap_url in sitemap_urls {
                let found =
                    Self::parse_sitemap(&client, &sitemap_url, 0, &mut walk, limiter).await?;
                urls.extend(found);
            }

            // Not one of the candidate locations produced an HTTP response —
            // DNS failure, refused connection, TLS failure, timeout. That is a
            // failed scan, and returning `Ok(vec![])` for it made `--stats`
            // print "Sitemap  0 urls  0 errors", indistinguishable from a host
            // that simply publishes no sitemap.
            if !walk.answered {
                return Err(match walk.failure {
                    Some(e) => {
                        anyhow::anyhow!("could not reach {domain} to look for a sitemap: {e}")
                    }
                    None => anyhow::anyhow!("could not reach {domain} to look for a sitemap"),
                });
            }

            // The https and http candidates are distinct sitemap URLs, so
            // `visited` doesn't stop a site that serves both from contributing
            // every URL twice. Every other provider sorts and dedupes its
            // return; this one didn't.
            urls.sort();
            urls.dedup();

            Ok(urls.into_iter().map(UrlRecord::bare).collect())
        })
    }

    fn with_subdomains(&mut self, _include: bool) {}
    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }
    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }
    fn with_timeout(&mut self, seconds: u64) {
        self.timeout = Duration::from_secs(seconds);
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
    fn with_rate_limit(&mut self, rate_limit: Option<f32>) {
        self.rate_limit = RateLimiter::from_rate(rate_limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::urls_of;
    use mockito::Server;

    #[test]
    fn test_same_host_as_parent() {
        let parent = "https://example.com/sitemap.xml";
        assert!(same_host_as_parent(parent, "https://example.com/a.xml"));
        // Scheme may differ; the host may not.
        assert!(same_host_as_parent(parent, "http://example.com/a.xml"));
        assert!(same_host_as_parent(parent, "https://EXAMPLE.com/a.xml"));

        assert!(!same_host_as_parent(parent, "https://evil.example/a.xml"));
        assert!(!same_host_as_parent(
            parent,
            "https://cdn.example.com/a.xml"
        ));
        assert!(!same_host_as_parent(parent, "http://127.0.0.1:8080/a.xml"));
        assert!(!same_host_as_parent(parent, "not a url"));
        // A non-default port is a different endpoint.
        assert!(!same_host_as_parent(
            parent,
            "https://example.com:8443/a.xml"
        ));
        // ...but the scheme's own default port is the same endpoint.
        assert!(same_host_as_parent(parent, "https://example.com:443/a.xml"));
    }

    #[tokio::test]
    async fn test_sitemap_index_cannot_redirect_the_fetch_to_another_host() {
        // Regression: a <sitemapindex> could name any host in <loc> and urx
        // would fetch it, reporting whatever it found as URLs belonging to the
        // target — including hosts only the machine running urx can reach.
        let mut elsewhere = Server::new_async().await;
        let offsite = elsewhere
            .mock("GET", "/private.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(
                r#"<?xml version="1.0"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://leaked.example/secret</loc></url>
</urlset>"#,
            )
            .expect(0)
            .create_async()
            .await;

        let mut server = Server::new_async().await;
        let host = server.host_with_port();
        let index_body = format!(
            r#"<?xml version="1.0"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>{}/private.xml</loc></sitemap>
  <sitemap><loc>http://{host}/own.xml</loc></sitemap>
</sitemapindex>"#,
            elsewhere.url()
        );
        let _index = server
            .mock("GET", "/sitemap.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(index_body)
            .create_async()
            .await;
        let _own = server
            .mock("GET", "/own.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(
                r#"<?xml version="1.0"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/kept</loc></url>
</urlset>"#,
            )
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        let urls = urls_of(provider.fetch_urls(&host).await.unwrap());

        assert_eq!(urls, vec!["https://example.com/kept".to_string()]);
        offsite.assert(); // never requested
    }

    #[test]
    fn test_new_provider() {
        let provider = SitemapProvider::new();
        assert_eq!(provider.timeout, Duration::from_secs(30));
        assert_eq!(provider.retries, 3);
        assert!(!provider.random_agent);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert!(!provider.insecure);
        assert!(provider.rate_limit.is_none());
    }

    #[test]
    fn test_with_rate_limit() {
        let mut provider = SitemapProvider::new();
        provider.with_rate_limit(Some(2.5));
        assert!(provider.rate_limit.is_some());
        // A non-positive rate means "no limiting".
        provider.with_rate_limit(Some(0.0));
        assert!(provider.rate_limit.is_none());
    }

    #[tokio::test]
    async fn test_rate_limit_paces_probe_requests() {
        // fetch_urls fires up to six back-to-back candidate-location probes;
        // --rate-limit must pace them. Before this provider honored the limiter
        // `with_rate_limit` was a no-op and the probes raced out instantly.
        let server = Server::new_async().await;
        let host = server.host_with_port();

        let mut provider = SitemapProvider::new();
        provider.with_rate_limit(Some(20.0)); // 50ms minimum interval

        let start = std::time::Instant::now();
        // No mocks: every probe 404s/fails fast, but each acquire() still paces.
        let _ = provider.fetch_urls(&host).await;

        // Six probes => five enforced ~50ms gaps (~250ms). A no-op limiter would
        // finish in a few ms; allow generous scheduler slack below that signal.
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "rate limit did not pace the sitemap probe requests: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn test_sitemap_is_fetched_once_not_probed_then_refetched() {
        // Regression: fetch_urls used to GET each candidate location to check it
        // existed and then hand the same URL to parse_sitemap, which fetched it
        // again — two requests (and two full downloads) for every sitemap that
        // actually exists.
        let mut server = Server::new_async().await;
        let host = server.host_with_port();
        let sitemap = server
            .mock("GET", "/sitemap.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://example.com/only</loc></url>
</urlset>"#,
            )
            .expect(1)
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        // The https:// candidates can't reach a plain-HTTP mock; the http:// one
        // does, which is what exercises the single-fetch path.
        let urls = urls_of(provider.fetch_urls(&host).await.unwrap());

        assert_eq!(urls, vec!["https://example.com/only".to_string()]);
        sitemap.assert();
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = SitemapProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = SitemapProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = SitemapProvider::new();
        provider.with_timeout(60);
        assert_eq!(provider.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_with_retries() {
        let mut provider = SitemapProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = SitemapProvider::new();
        provider.with_random_agent(true);
        assert!(provider.random_agent);
        provider.with_random_agent(false);
        assert!(!provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = SitemapProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_clone_box() {
        let provider = SitemapProvider::new();
        let _cloned = provider.clone_box();
        // Testing the existence of cloned object
    }

    #[test]
    fn test_build_client() {
        let provider = SitemapProvider::new();
        let client_result = provider.build_client();
        assert!(client_result.is_ok());
    }

    #[tokio::test]
    async fn test_sitemap_xml_parsing() {
        // Sample sitemap XML content for testing
        let sitemap_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/</loc>
    <lastmod>2023-01-01</lastmod>
    <changefreq>daily</changefreq>
    <priority>1.0</priority>
  </url>
  <url>
    <loc>https://example.com/about</loc>
    <lastmod>2023-01-02</lastmod>
    <changefreq>weekly</changefreq>
    <priority>0.8</priority>
  </url>
</urlset>"#;

        // Parse the sample sitemap
        let doc = Document::parse(sitemap_xml).unwrap();
        let mut urls = Vec::new();

        for url_node in doc.descendants().filter(|n| n.has_tag_name("url")) {
            if let Some(loc_node) = url_node.descendants().find(|n| n.has_tag_name("loc")) {
                if let Some(url) = loc_node.text() {
                    urls.push(url.to_string());
                }
            }
        }

        // Verify extracted URLs
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/".to_string()));
        assert!(urls.contains(&"https://example.com/about".to_string()));
    }

    #[tokio::test]
    async fn test_sitemap_index_parsing() {
        // Sample sitemap index XML content for testing
        let sitemap_index_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap>
    <loc>https://example.com/sitemap1.xml</loc>
    <lastmod>2023-01-01</lastmod>
  </sitemap>
  <sitemap>
    <loc>https://example.com/sitemap2.xml</loc>
    <lastmod>2023-01-02</lastmod>
  </sitemap>
</sitemapindex>"#;

        // Parse the sample sitemap index
        let doc = Document::parse(sitemap_index_xml).unwrap();
        let mut sitemap_urls = Vec::new();

        for sitemap_node in doc.descendants().filter(|n| n.has_tag_name("sitemap")) {
            if let Some(loc_node) = sitemap_node.descendants().find(|n| n.has_tag_name("loc")) {
                if let Some(url) = loc_node.text() {
                    sitemap_urls.push(url.to_string());
                }
            }
        }

        // Verify extracted sitemap URLs
        assert_eq!(sitemap_urls.len(), 2);
        assert!(sitemap_urls.contains(&"https://example.com/sitemap1.xml".to_string()));
        assert!(sitemap_urls.contains(&"https://example.com/sitemap2.xml".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_urls_sitemap_xml() {
        let mut server = Server::new_async().await;
        let sitemap_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/page1</loc>
  </url>
  <url>
    <loc>https://example.com/page2</loc>
  </url>
</urlset>"#;

        let _m = server
            .mock("GET", "/sitemap.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(sitemap_xml)
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        // remove "http://" prefix from host_with_port if present (mockito shouldn't have it, but just in case)
        let host = server.host_with_port();
        // fetch_urls expects domain without protocol
        let result = provider.fetch_urls(&host).await;

        assert!(result.is_ok());
        let urls = urls_of(result.unwrap());
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/page1".to_string()));
        assert!(urls.contains(&"https://example.com/page2".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_urls_sitemap_index() {
        let mut server = Server::new_async().await;
        let host = server.host_with_port();

        // Sitemap index pointing to a nested sitemap
        // We use the server address to ensure it calls back to our mock
        let sitemap_index = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap>
    <loc>http://{}/nested.xml</loc>
  </sitemap>
</sitemapindex>"#,
            host
        );

        let nested_sitemap = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/nested-page</loc>
  </url>
</urlset>"#;

        let _m1 = server
            .mock("GET", "/sitemap_index.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(sitemap_index)
            .create_async()
            .await;

        let _m2 = server
            .mock("GET", "/nested.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(nested_sitemap)
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        // fetch_urls will try sitemap.xml first (which will 404/fail or 501), then sitemap_index.xml
        // Mockito returns 501 for unmocked requests by default, but it's fine as long as fetch_urls continues
        let result = provider.fetch_urls(&host).await;

        assert!(result.is_ok());
        let urls = urls_of(result.unwrap());
        assert_eq!(urls.len(), 1);
        assert!(urls.contains(&"https://example.com/nested-page".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_urls_sitemap_index_self_cycle_terminates() {
        // A sitemap index that references itself must not loop forever.
        let mut server = Server::new_async().await;
        let host = server.host_with_port();

        let self_ref_index = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>http://{host}/sitemap_index.xml</loc></sitemap>
</sitemapindex>"#
        );

        // Allow many hits; the cycle guard should make far fewer (ideally 1-2).
        let _m = server
            .mock("GET", "/sitemap_index.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(self_ref_index)
            .expect_at_most(5)
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        // Completing at all (not hanging) is the assertion.
        let result = provider.fetch_urls(&host).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_fetch_urls_sitemap_txt() {
        let mut server = Server::new_async().await;
        let sitemap_txt = "https://example.com/page1\nhttps://example.com/page2";

        let _m = server
            .mock("GET", "/sitemap.txt")
            .with_status(200)
            .with_body(sitemap_txt)
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        let host = server.host_with_port();
        let result = provider.fetch_urls(&host).await;

        assert!(result.is_ok());
        let urls = urls_of(result.unwrap());
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/page1".to_string()));
        assert!(urls.contains(&"https://example.com/page2".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_urls_xml_html_error_page_not_mined() {
        let mut server = Server::new_async().await;
        let host = server.host_with_port();

        // A non-XML HTML error page served where a .xml sitemap was expected.
        // The unescaped '&' makes XML parsing fail; the http line must NOT be
        // harvested, because the source is not a text sitemap.
        let html = "<html>\n<p>error & oops</p>\nhttp://attacker.example/inject\n</html>";
        let _m = server
            .mock("GET", "/sitemap.xml")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(html)
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        let urls = urls_of(provider.fetch_urls(&host).await.unwrap());
        assert!(
            !urls.iter().any(|u| u.contains("attacker.example")),
            "HTML error page was mined for URLs: {urls:?}"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_not_found() {
        let server = Server::new_async().await;
        // No mocks created, so all requests will return 501 (Not Implemented) or fail connection

        let provider = SitemapProvider::new();
        let host = server.host_with_port();
        let result = provider.fetch_urls(&host).await;

        assert!(result.is_ok());
        let urls = urls_of(result.unwrap());
        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn test_unreachable_host_is_an_error_not_an_empty_success() {
        // Regression: every transport failure was swallowed into `Ok(vec![])`,
        // so a run against a host urx could not connect to at all reported
        // "Sitemap  0 urls  0 errors" in --stats — a failed scan wearing a ✓.
        // Bind a port, drop the listener: nothing answers on it.
        let addr = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            drop(listener);
            addr
        };

        let mut provider = SitemapProvider::new();
        provider.with_timeout(5);
        let err = provider
            .fetch_urls(&addr.to_string())
            .await
            .expect_err("an unreachable host must be reported as an error");
        assert!(err.to_string().contains("could not reach"), "got: {err}");
    }

    #[tokio::test]
    async fn test_host_with_no_sitemap_is_a_success_with_zero_urls() {
        // The other side of the line above: a host that answers 404 at every
        // candidate location has told us it publishes no sitemap. That is a
        // completed scan with zero URLs and must stay an `Ok`.
        let mut server = Server::new_async().await;
        let host = server.host_with_port();
        let _m = server
            .mock("GET", mockito::Matcher::Any)
            .with_status(404)
            .expect_at_least(1)
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        let urls = urls_of(
            provider
                .fetch_urls(&host)
                .await
                .expect("a host without a sitemap is an empty result, not an error"),
        );
        assert!(urls.is_empty(), "{urls:?}");
    }

    #[tokio::test]
    async fn test_pretty_printed_loc_values_are_trimmed() {
        // Regression: XML keeps the indentation inside a pretty-printed
        // `<loc>\n  https://…\n</loc>`, and the surrounding whitespace rode
        // along into the emitted URL. Such a "URL" fails host validation, so
        // every URL of a pretty-printed sitemap was silently dropped.
        let mut server = Server::new_async().await;
        let host = server.host_with_port();
        let _m = server
            .mock("GET", "/sitemap.xml")
            .with_status(200)
            .with_header("content-type", "application/xml")
            .with_body(
                "<?xml version=\"1.0\"?>\n\
<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n\
  <url>\n    <loc>\n      https://example.com/pretty\n    </loc>\n  </url>\n\
  <url>\n    <loc>   </loc>\n  </url>\n\
</urlset>",
            )
            .create_async()
            .await;

        let provider = SitemapProvider::new();
        let urls = urls_of(provider.fetch_urls(&host).await.unwrap());

        assert_eq!(urls, vec!["https://example.com/pretty".to_string()]);
        // Belt and braces: nothing we emit may carry stray whitespace.
        for u in &urls {
            assert_eq!(u.trim(), u, "{u:?}");
            assert!(url::Url::parse(u).is_ok(), "{u:?}");
        }
    }
}
