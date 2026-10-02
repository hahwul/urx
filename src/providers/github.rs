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

/// Maximum search-result pages we fetch per domain. GitHub Code Search caps at
/// 1000 results total (10 × 100), so 10 pages covers everything the API will
/// return; a smaller cap silently truncated large domains. The page loop still
/// stops as soon as a page comes back empty, so domains with fewer results
/// never pay for the extra pages — and `--rate-limit` paces the requests.
const MAX_PAGES: u32 = 10;
const PER_PAGE: u32 = 100;

#[derive(Clone)]
pub struct GitHubProvider {
    api_key_rotator: ApiKeyRotator,
    include_subdomains: bool,
    net: NetConfig,
    base_url: String,
}

#[derive(Debug, Deserialize, Default)]
struct SearchResponse {
    #[serde(default)]
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    #[serde(default)]
    text_matches: Vec<TextMatch>,
}

#[derive(Debug, Deserialize)]
struct TextMatch {
    #[serde(default)]
    fragment: String,
}

impl GitHubProvider {
    pub fn new_with_keys(api_keys: Vec<String>) -> Self {
        GitHubProvider {
            api_key_rotator: ApiKeyRotator::new(api_keys),
            include_subdomains: false,
            net: NetConfig::default(),
            base_url: "https://api.github.com".to_string(),
        }
    }
}

/// Extract URLs from a free-form text fragment that contain `domain`
/// (or, when `include_subdomains` is true, any subdomain of it).
/// The matched URL must end its host exactly at `domain` so we don't
/// surface unrelated hosts like `notexample.com` for a search of `example.com`.
pub(crate) fn extract_matching_urls(
    fragment: &str,
    domain: &str,
    include_subdomains: bool,
    sink: &mut HashSet<String>,
) {
    let domain = domain.to_ascii_lowercase();
    for token in fragment.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',') {
        let token = token.trim_end_matches(|c: char| {
            matches!(c, '.' | ')' | ']' | '}' | '>' | ';' | ':' | '!' | '?' | '`')
        });
        let lower = token.to_ascii_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            continue;
        }
        let Ok(parsed) = url::Url::parse(token) else {
            continue;
        };
        let Some(host) = parsed.host_str().map(|s| s.to_ascii_lowercase()) else {
            continue;
        };
        let host_matches = if include_subdomains {
            host == domain || host.ends_with(&format!(".{domain}"))
        } else {
            host == domain
        };
        if host_matches {
            sink.insert(token.to_string());
        }
    }
}

impl Provider for GitHubProvider {
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
            let base = &self.base_url;

            // Quoted phrase search keeps the result set tight to literal
            // mentions of the domain rather than partial-token matches.
            let q = format!("\"{domain}\"");
            let encoded_q = url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>();

            let mut urls: HashSet<String> = HashSet::new();
            let mut last_error: Option<anyhow::Error> = None;
            // Set when a page exhausts its retries, so results collected so far
            // are reported as a truncated/partial crawl rather than a clean run.
            let mut truncated = false;

            for page in 1..=MAX_PAGES {
                let url =
                    format!("{base}/search/code?q={encoded_q}&per_page={PER_PAGE}&page={page}");

                let fetched = send_with_retry(
                    self.net.retries,
                    self.net.rate_limit.as_ref(),
                    // 422 from search/code typically means we ran past the
                    // result set — the natural end, not something to retry.
                    |status| status != 422,
                    || {
                        // Rotate the token per attempt so a rate-limited/
                        // secondary-limited token is retried with a different
                        // one when several are configured.
                        let api_key = self.api_key_rotator.next_key().unwrap_or_default();
                        client
                            .get(&url)
                            .header("Authorization", format!("Bearer {api_key}"))
                            .header("Accept", "application/vnd.github.v3.text-match+json")
                            .header("X-GitHub-Api-Version", "2022-11-28")
                    },
                    |status, body| {
                        if status == 422 {
                            return Ok(SearchResponse::default());
                        }
                        serde_json::from_str::<SearchResponse>(&body)
                            .map_err(|e| anyhow::anyhow!("Failed to parse GitHub response: {e}"))
                    },
                )
                .await;
                let parsed = match fetched {
                    Ok(parsed) => parsed,
                    Err(e) => {
                        last_error = Some(e);
                        truncated = true;
                        break;
                    }
                };
                if parsed.items.is_empty() {
                    // No more results — stop paginating.
                    break;
                }
                for item in parsed.items {
                    for m in item.text_matches {
                        extract_matching_urls(
                            &m.fragment,
                            domain,
                            self.include_subdomains,
                            &mut urls,
                        );
                    }
                }
                // The run asked us to stop (--max-time elapsed, or Ctrl-C).
                // Keep the pages already walked instead of losing them to the
                // hard cancel after the grace window.
                if reporter.as_ref().is_some_and(|r| r.stop_requested()) {
                    truncated = true;
                    break;
                }
            }

            if urls.is_empty() {
                if let Some(e) = last_error {
                    return Err(e);
                }
            } else if truncated {
                // We collected some URLs but a later page exhausted its retries,
                // so this is a partial result — flag it instead of presenting a
                // truncated crawl as a clean success.
                if let Some(r) = &reporter {
                    r.mark_partial();
                }
            }

            let mut out: Vec<String> = urls.into_iter().collect();
            out.sort();
            Ok(out.into_iter().map(UrlRecord::bare).collect())
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
    fn test_extract_urls_exact_host() {
        let mut sink = HashSet::new();
        extract_matching_urls(
            "see https://example.com/login and https://other.test/x",
            "example.com",
            false,
            &mut sink,
        );
        assert_eq!(sink.len(), 1);
        assert!(sink.contains("https://example.com/login"));
    }

    #[test]
    fn test_extract_urls_rejects_non_matching_suffix() {
        // A naive substring check would catch this; the host-match logic
        // must require exact equality (or a `.<domain>` suffix).
        let mut sink = HashSet::new();
        extract_matching_urls(
            "https://notexample.com/path",
            "example.com",
            false,
            &mut sink,
        );
        assert!(sink.is_empty());
    }

    #[test]
    fn test_extract_urls_subdomains() {
        let mut sink = HashSet::new();
        extract_matching_urls(
            "https://api.example.com/v1 https://example.com/root https://x.notexample.com",
            "example.com",
            true,
            &mut sink,
        );
        assert!(sink.contains("https://api.example.com/v1"));
        assert!(sink.contains("https://example.com/root"));
        assert!(!sink.iter().any(|u| u.contains("notexample.com")));
    }

    #[test]
    fn test_extract_urls_strips_trailing_punctuation() {
        let mut sink = HashSet::new();
        extract_matching_urls(
            "visit https://example.com/path. Then check https://example.com/other),",
            "example.com",
            false,
            &mut sink,
        );
        assert!(sink.contains("https://example.com/path"));
        assert!(sink.contains("https://example.com/other"));
    }

    #[test]
    fn test_new_provider_filters_empty_keys() {
        let p = GitHubProvider::new_with_keys(vec!["".to_string(), "k1".to_string()]);
        assert!(p.api_key_rotator.has_keys());
    }

    #[tokio::test]
    async fn test_fetch_urls_returns_empty_without_keys() {
        let p = GitHubProvider::new_with_keys(vec![]);
        let urls = urls_of(p.fetch_urls("example.com").await.unwrap());
        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn test_partial_result_is_flagged_when_a_page_fails() {
        let mut server = mockito::Server::new_async().await;
        let body = serde_json::json!({
            "items": [ { "text_matches": [ { "fragment": "https://example.com/login" } ] } ]
        })
        .to_string();
        // Page 1 yields a URL...
        let _p1 = server
            .mock("GET", "/search/code")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        // ...but page 2 fails and exhausts retries, so the result is partial.
        let _p2 = server
            .mock("GET", "/search/code")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "2".into()))
            .with_status(500)
            .expect(1)
            .create_async()
            .await;

        let mut provider = GitHubProvider::new_with_keys(vec!["t".into()]);
        provider.base_url = server.url();
        provider.net.retries = 0;

        let reporter =
            ProgressReporter::new(indicatif::ProgressBar::hidden(), "test · ".to_string());
        let urls = urls_of(
            provider
                .fetch_urls_with_progress("example.com", Some(reporter.clone()))
                .await
                .unwrap(),
        );

        // The page-1 URL is kept...
        assert_eq!(urls, vec!["https://example.com/login".to_string()]);
        // ...and the lost page is surfaced as partial, not a clean success.
        assert!(reporter.is_partial());
    }

    #[tokio::test]
    async fn test_fetch_urls_with_mock() {
        let mut server = mockito::Server::new_async().await;
        let body = serde_json::json!({
            "items": [
                {
                    "text_matches": [
                        { "fragment": "see https://example.com/login and https://api.example.com/v2" },
                        { "fragment": "noise" }
                    ]
                }
            ]
        })
        .to_string();
        // Page 1 returns one item; page 2 returns no items → loop stops.
        let _p1 = server
            .mock("GET", "/search/code")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let _p2 = server
            .mock("GET", "/search/code")
            .match_query(mockito::Matcher::UrlEncoded("page".into(), "2".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"items": []}"#)
            .expect(1)
            .create_async()
            .await;

        let mut provider = GitHubProvider::new_with_keys(vec!["test-token".into()]);
        provider.base_url = server.url();
        provider.net.retries = 0;

        let urls = urls_of(provider.fetch_urls("example.com").await.unwrap());
        assert_eq!(urls, vec!["https://example.com/login".to_string()]);

        // With subdomains enabled the api.example.com URL also surfaces.
        provider.with_subdomains(true);
        // Reset rotator key state isn't necessary; rotation tolerates re-fetch.
        // Reuse page mocks — they each expect exactly 1 hit per provider run
        // so we need fresh mocks for the second call: just confirm extraction
        // logic from a fragment instead.
        let mut sink = HashSet::new();
        extract_matching_urls(
            "see https://example.com/login and https://api.example.com/v2",
            "example.com",
            true,
            &mut sink,
        );
        assert!(sink.contains("https://api.example.com/v2"));
    }
}
