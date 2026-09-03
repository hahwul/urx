//! Replaying what the Wayback Machine stored, not just what it indexed.
//!
//! The CDX index says *that* a URL was captured and when; the replay endpoint
//! hands back the bytes of that capture. The two features built on it —
//! `--archive-body` and `--archived-discovery` — both need the same address
//! and the same care about what comes back, so that lives here rather than in
//! either caller.

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
}
