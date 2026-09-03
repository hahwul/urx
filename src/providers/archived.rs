//! Replaying what the Wayback Machine stored, not just what it indexed.
//!
//! The CDX index says *that* a URL was captured and when; the replay endpoint
//! hands back the bytes of that capture. The two features built on it —
//! `--archive-body` and `--archived-discovery` — both need the same address
//! and the same care about what comes back, so that lives here rather than in
//! either caller.

use anyhow::Result;
use reqwest::Client;

use crate::network::client::{get_with_retry, read_body_capped};
use crate::network::RateLimiter;

/// Origin of the Wayback Machine, for both its CDX index and its replay
/// endpoint.
pub const WAYBACK_ORIGIN: &str = "https://web.archive.org";

/// The replay URL for one capture, in its raw form.
///
/// `https://web.archive.org/web/<timestamp>/<url>` serves the capture wrapped
/// in the Wayback toolbar, with every link in the body rewritten to point back
/// into the archive. The `id_` flag after the timestamp turns all of that off
/// and returns the original response bytes — verified live: the body is the
/// capture as crawled, with the original `Content-Type`, and still in its
/// original `Content-Encoding` (reqwest's gzip/brotli support decodes it).
///
/// The timestamp need not be exact: Wayback replays the capture nearest to it,
/// so a timestamp another archive reported still lands on *a* capture of the
/// URL. A URL the Wayback Machine never captured answers 404.
pub fn replay_url(origin: &str, timestamp: &str, url: &str) -> String {
    format!("{origin}/web/{timestamp}id_/{url}")
}

/// How `--archived-discovery` should walk a document's history.
#[derive(Debug, Clone)]
pub struct ArchivedDiscovery {
    /// Ceiling on archived documents fetched per domain and provider. A
    /// robots.txt that has been edited weekly for fifteen years has hundreds
    /// of distinct versions; without a bound that is hundreds of requests per
    /// target, aimed at a public archive.
    pub limit: usize,
    /// `--from` / `--to`, in 14-digit CDX form, narrowing which versions are
    /// considered at all.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Archive origin, overridable so tests can point at a mock server.
    origin: String,
}

impl ArchivedDiscovery {
    pub fn new(limit: usize) -> Self {
        ArchivedDiscovery {
            limit,
            from: None,
            to: None,
            origin: WAYBACK_ORIGIN.to_string(),
        }
    }

    /// Restrict the versions considered to a capture date window.
    pub fn with_window(mut self, from: Option<String>, to: Option<String>) -> Self {
        self.from = from;
        self.to = to;
        self
    }

    #[cfg(test)]
    pub fn with_origin(mut self, origin: String) -> Self {
        self.origin = origin;
        self
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }
}

/// One capture of an archived document, as the CDX index lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedCapture {
    /// The URL exactly as it was crawled — scheme and host included, which a
    /// parser pasting paths after the host needs to know.
    pub original: String,
    /// 14-digit CDX timestamp of the capture.
    pub timestamp: String,
    /// HTTP status the archive recorded, when it recorded one.
    pub status: Option<String>,
    /// Content digest, when the archive recorded one.
    pub digest: Option<String>,
}

impl ArchivedCapture {
    /// Whether the archive recorded a success for this capture. A robots.txt
    /// captured as a 401 or 404 — real cases; github.com's was a 401 in 2007 —
    /// has no directives to read, so it is not worth a replay request.
    pub fn is_success(&self) -> bool {
        self.status
            .as_deref()
            .is_some_and(|s| s.len() == 3 && s.starts_with('2'))
    }
}

/// The CDX fields the version listing asks for. `original` first, as in the
/// Wayback provider: an archived URL can contain a raw space, so rows are split
/// from the right and whatever precedes the fixed trailing columns is the URL.
const INDEX_FIELDS: &str = "original,timestamp,statuscode,digest";

/// Rows asked of the index per document. Measured live: github.com/robots.txt
/// — about as heavily crawled as a file gets — lists ~14k rows under the
/// filters below, for 107 distinct versions. This cap only stops a misbehaving
/// index from streaming without end; the same figure the Wayback provider uses
/// per page.
const INDEX_ROW_LIMIT: usize = 50_000;

/// The CDX query listing the distinct versions of one document.
///
/// `collapse=digest` is what makes this cheap: the index folds consecutive
/// captures that served the same bytes into one row, so a file that was
/// crawled daily but edited yearly comes back as a handful of rows — one per
/// *change*, which is exactly the set worth fetching.
///
/// The status filter is not optional. The urlkey ignores scheme and a leading
/// `www.` — which is wanted, so `example.com/robots.txt` matches every
/// spelling the crawler used — but it also interleaves the `www.` host's
/// `301` rows with the apex's `200` rows, and `collapse=digest` only folds
/// *adjacent* rows. Measured live: github.com/robots.txt lists 325,036 rows
/// without the filter (~30 MB) and 13,909 with it, for the same 107 distinct
/// bodies. Captures recorded as anything but a success have no directives to
/// read anyway; the few that still arrive (a `-` status) are set aside
/// client-side by [`distinct_versions`].
pub fn capture_index_url(
    origin: &str,
    document: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> String {
    let mut url = format!(
        "{origin}/cdx/search/cdx?url={document}&fl={INDEX_FIELDS}&collapse=digest&filter=statuscode:2..&limit={INDEX_ROW_LIMIT}"
    );
    if let Some(from) = from {
        url.push_str("&from=");
        url.push_str(from);
    }
    if let Some(to) = to {
        url.push_str("&to=");
        url.push_str(to);
    }
    url
}

/// Treat CDX's `-` null marker, and an empty column, as absent.
fn field(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "-").then(|| value.to_string())
}

/// Parse one `INDEX_FIELDS` row. A row that does not split into exactly four
/// columns is not the layout asked for and is rejected rather than read with
/// its fields shifted.
fn parse_index_row(line: &str) -> Option<ArchivedCapture> {
    let line = line.trim();
    if !line.starts_with("http://") && !line.starts_with("https://") {
        return None;
    }
    let mut fields = line.rsplitn(4, ' ');
    let (Some(digest), Some(status), Some(timestamp), Some(original)) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return None;
    };
    let timestamp = timestamp.trim();
    if timestamp.len() != 14 || !timestamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(ArchivedCapture {
        original: original.to_string(),
        timestamp: timestamp.to_string(),
        status: field(status),
        digest: field(digest),
    })
}

/// The versions of one document worth replaying, and the ones not worth it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VersionListing {
    /// Distinct successful captures, newest first.
    pub fetchable: Vec<ArchivedCapture>,
    /// Captures the archive recorded as a non-success — nothing to parse.
    pub skipped: Vec<ArchivedCapture>,
}

/// Reduce a CDX listing to the versions worth fetching.
///
/// `collapse=digest` only folds *adjacent* rows, so a file that was edited and
/// later reverted lists the same digest twice; every digest is fetched at most
/// once here. Newest first: the live provider already covers the present, and
/// when `limit` cuts the list short the recently-removed paths are the ones
/// most likely to still be worth a look.
pub fn distinct_versions(text: &str) -> VersionListing {
    let mut listing = VersionListing::default();
    let mut seen_digests = std::collections::HashSet::new();

    for capture in text.lines().rev().filter_map(parse_index_row) {
        if let Some(digest) = &capture.digest {
            if !seen_digests.insert(digest.clone()) {
                continue;
            }
        }
        if capture.is_success() {
            listing.fetchable.push(capture);
        } else {
            listing.skipped.push(capture);
        }
    }
    listing
}

/// Ask the index for every distinct version of `document` (host and path,
/// no scheme — e.g. `example.com/robots.txt`).
pub async fn list_versions(
    client: &Client,
    settings: &ArchivedDiscovery,
    document: &str,
    retries: u32,
    limiter: Option<&RateLimiter>,
) -> Result<VersionListing> {
    if let Some(rl) = limiter {
        rl.acquire().await;
    }
    let url = capture_index_url(
        settings.origin(),
        document,
        settings.from.as_deref(),
        settings.to.as_deref(),
    );
    let text = get_with_retry(client, &url, retries).await?;
    Ok(distinct_versions(&text))
}

/// What replaying one capture produced.
#[derive(Debug)]
pub enum Replay {
    /// The archive served the capture; here is its body, read capped, and the
    /// `Content-Type` it was served with.
    Body {
        text: String,
        content_type: Option<String>,
    },
    /// The archive answered with a non-success for this capture (a 404 when it
    /// has no such capture after all, or the capture's own error status).
    Unavailable(reqwest::StatusCode),
}

/// Replay `capture` in raw form, reading at most `max_bytes` of body.
pub async fn replay_capture(
    client: &Client,
    origin: &str,
    capture: &ArchivedCapture,
    max_bytes: usize,
    limiter: Option<&RateLimiter>,
) -> Result<Replay> {
    replay_at(
        client,
        origin,
        &capture.timestamp,
        &capture.original,
        max_bytes,
        limiter,
    )
    .await
}

/// Replay `url` as it was at `timestamp` (nearest capture), in raw form.
pub async fn replay_at(
    client: &Client,
    origin: &str,
    timestamp: &str,
    url: &str,
    max_bytes: usize,
    limiter: Option<&RateLimiter>,
) -> Result<Replay> {
    if let Some(rl) = limiter {
        rl.acquire().await;
    }
    let response = client
        .get(replay_url(origin, timestamp, url))
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(Replay::Unavailable(response.status()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let text = read_body_capped(response, max_bytes).await?;
    Ok(Replay::Body { text, content_type })
}

/// One line summarising the captures a walk chose not to replay, for
/// `--verbose`: `3 skipped (401 ×2, 404 ×1)`.
pub fn describe_skipped(skipped: &[ArchivedCapture]) -> String {
    let mut by_status: std::collections::BTreeMap<&str, usize> = Default::default();
    for capture in skipped {
        *by_status
            .entry(capture.status.as_deref().unwrap_or("no status"))
            .or_default() += 1;
    }
    let detail: Vec<String> = by_status
        .iter()
        .map(|(status, n)| format!("{status} ×{n}"))
        .collect();
    format!("{} skipped ({})", skipped.len(), detail.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_url_carries_the_raw_flag_and_the_url_verbatim() {
        assert_eq!(
            replay_url(
                WAYBACK_ORIGIN,
                "20200101000000",
                "https://example.com/a?b=c&d=e"
            ),
            "https://web.archive.org/web/20200101000000id_/https://example.com/a?b=c&d=e"
        );
        // A mock origin stands in for the archive in tests.
        assert_eq!(
            replay_url("http://127.0.0.1:1234", "19990101000000", "http://x/"),
            "http://127.0.0.1:1234/web/19990101000000id_/http://x/"
        );
    }

    #[test]
    fn capture_index_url_collapses_on_digest_and_carries_the_window() {
        let url = capture_index_url(WAYBACK_ORIGIN, "example.com/robots.txt", None, None);
        assert!(
            url.starts_with("https://web.archive.org/cdx/search/cdx?url=example.com/robots.txt&")
        );
        assert!(url.contains("&collapse=digest"), "{url}");
        assert!(url.contains("&filter=statuscode:2.."), "{url}");
        assert!(url.contains(&format!("&fl={INDEX_FIELDS}")), "{url}");
        assert!(!url.contains("&from="), "{url}");

        let windowed = capture_index_url(
            WAYBACK_ORIGIN,
            "example.com/robots.txt",
            Some("20150101000000"),
            Some("20151231235959"),
        );
        assert!(windowed.contains("&from=20150101000000"), "{windowed}");
        assert!(windowed.contains("&to=20151231235959"), "{windowed}");
    }

    /// The layout the index answers with, as observed live for
    /// github.com/robots.txt: chronological, one row per change.
    const GITHUB_LIKE: &str = "\
http://github.com/robots.txt 20070917000000 401 AAA401\n\
http://github.com/robots.txt 20080301000000 200 BBB200\n\
https://github.com/robots.txt 20120301000000 200 CCC200\n\
https://github.com/robots.txt 20130301000000 - -\n\
https://github.com/robots.txt 20150301000000 200 BBB200\n\
https://github.com/robots.txt 20200301000000 404 DDD404\n\
https://github.com/robots.txt 20240301000000 200 EEE200\n";

    #[test]
    fn distinct_versions_keeps_one_row_per_digest_newest_first() {
        let listing = distinct_versions(GITHUB_LIKE);

        let fetched: Vec<(&str, &str)> = listing
            .fetchable
            .iter()
            .map(|c| (c.timestamp.as_str(), c.digest.as_deref().unwrap()))
            .collect();
        // BBB200 appears twice (edited, then reverted); only its newest
        // occurrence survives. Newest first throughout.
        assert_eq!(
            fetched,
            vec![
                ("20240301000000", "EEE200"),
                ("20150301000000", "BBB200"),
                ("20120301000000", "CCC200"),
            ]
        );
        // The original URL travels with each row, scheme included.
        assert_eq!(
            listing.fetchable[2].original,
            "https://github.com/robots.txt"
        );
    }

    #[test]
    fn distinct_versions_sets_aside_captures_recorded_as_errors() {
        let listing = distinct_versions(GITHUB_LIKE);
        let skipped: Vec<(&str, Option<&str>)> = listing
            .skipped
            .iter()
            .map(|c| (c.timestamp.as_str(), c.status.as_deref()))
            .collect();
        // The 401 from 2007, the 404 from 2020, and a row with no status at
        // all — none has directives to read.
        assert_eq!(
            skipped,
            vec![
                ("20200301000000", Some("404")),
                ("20130301000000", None),
                ("20070917000000", Some("401")),
            ]
        );
        assert_eq!(
            describe_skipped(&listing.skipped),
            "3 skipped (401 ×1, 404 ×1, no status ×1)"
        );
    }

    #[test]
    fn index_rows_that_are_not_the_requested_layout_are_rejected() {
        assert!(parse_index_row("").is_none());
        assert!(parse_index_row("<html>Service unavailable</html>").is_none());
        assert!(parse_index_row("https://example.com/robots.txt 20200101000000 200").is_none());
        // A malformed timestamp cannot be replayed.
        assert!(parse_index_row("https://example.com/robots.txt 2020 200 ABC").is_none());
        // A URL containing a space still parses, because the split is from
        // the right.
        let row =
            parse_index_row("https://example.com/a b/robots.txt 20200101000000 200 ABC").unwrap();
        assert_eq!(row.original, "https://example.com/a b/robots.txt");
        assert_eq!(row.status.as_deref(), Some("200"));
    }

    #[test]
    fn an_empty_listing_has_nothing_to_fetch() {
        assert_eq!(distinct_versions(""), VersionListing::default());
    }

    #[tokio::test]
    async fn list_versions_queries_the_index_and_replay_reads_the_capture() {
        let mut server = mockito::Server::new_async().await;
        let index = server
            .mock("GET", "/cdx/search/cdx")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("url".into(), "example.com/robots.txt".into()),
                mockito::Matcher::UrlEncoded("collapse".into(), "digest".into()),
                mockito::Matcher::UrlEncoded("from".into(), "20100101000000".into()),
            ]))
            .with_status(200)
            .with_body(
                "https://example.com/robots.txt 20150101000000 200 AAA\n\
                 https://example.com/robots.txt 20160101000000 404 BBB\n",
            )
            .expect(1)
            .create_async()
            .await;
        let replay = server
            .mock(
                "GET",
                "/web/20150101000000id_/https://example.com/robots.txt",
            )
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("Disallow: /old-admin\n")
            .expect(1)
            .create_async()
            .await;
        let gone = server
            .mock(
                "GET",
                "/web/20160101000000id_/https://example.com/robots.txt",
            )
            .with_status(404)
            .create_async()
            .await;

        let settings = ArchivedDiscovery::new(10)
            .with_window(Some("20100101000000".to_string()), None)
            .with_origin(server.url());
        let client = reqwest::Client::new();

        let listing = list_versions(&client, &settings, "example.com/robots.txt", 0, None)
            .await
            .unwrap();
        assert_eq!(listing.fetchable.len(), 1);
        assert_eq!(listing.skipped.len(), 1);

        match replay_capture(
            &client,
            settings.origin(),
            &listing.fetchable[0],
            1024,
            None,
        )
        .await
        .unwrap()
        {
            Replay::Body { text, content_type } => {
                assert_eq!(text, "Disallow: /old-admin\n");
                assert_eq!(content_type.as_deref(), Some("text/plain"));
            }
            other => panic!("expected a body, got {other:?}"),
        }
        match replay_capture(&client, settings.origin(), &listing.skipped[0], 1024, None)
            .await
            .unwrap()
        {
            Replay::Unavailable(status) => assert_eq!(status, 404),
            other => panic!("expected unavailable, got {other:?}"),
        }
        index.assert();
        replay.assert();
        gone.assert();
    }
}
