//! `-H/--header`, `--cookie` and `--user-agent`: the request headers urx sends
//! to the *target*.
//!
//! Every live-request feature urx has — `--check-status`, `--extract-links`,
//! `--extract-js-endpoints`, `--expand-specs` — re-requests collected URLs from
//! the site itself, and until now it could only do so anonymously. Against
//! anything behind a login or a WAF that is the difference between a useful run
//! and a page of 403s, which is why httpx, katana, ffuf and nuclei all take a
//! `-H`. urx was the one link in that pipeline that could not.
//!
//! # Why these headers do not reach the archives
//!
//! They are sent only by the components that talk to the target. A provider
//! queries web.archive.org, index.commoncrawl.org, otx.alienvault.com; so does
//! `--archive-body`, which replays captures from the Wayback Machine. Handing
//! `-H "Authorization: Bearer …"` to those hosts would mail the target's
//! credentials to a third party that archives what it receives — a footgun with
//! no upside, since none of them would do anything with the header anyway. The
//! two providers that *do* fetch from the target, `robots` and `sitemap`, send
//! them like the testers do. See [`crate::providers::Provider::with_headers`].

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, COOKIE, USER_AGENT};
use std::sync::Arc;

/// The parsed `-H` / `--cookie` / `--user-agent` set, ready to hand to a
/// client builder.
///
/// Empty by default and behind an `Arc`, because every provider and tester
/// carries a copy and the overwhelmingly common case is that there is nothing
/// to carry.
#[derive(Debug, Clone, Default)]
pub struct CustomHeaders(Option<Arc<HeaderMap>>);

impl CustomHeaders {
    /// Parse the CLI forms into one header map.
    ///
    /// `headers` are `Name: value` strings, `cookie` becomes a `Cookie`
    /// header and `user_agent` a `User-Agent` one. A later value for a name
    /// replaces an earlier one — reqwest collapses duplicate default-header
    /// keys to the last one regardless, so appending would only produce a
    /// header the user could not see and could not remove.
    ///
    /// # Errors
    ///
    /// Returns an error naming the offending argument when a header has no
    /// `:`, an empty name, or a name or value that is not legal in HTTP. These
    /// are typos in a command line, so they stop the run rather than being
    /// dropped silently into a set of requests the user believes are
    /// authenticated.
    pub fn parse(
        headers: &[String],
        cookie: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<Self> {
        let mut map = HeaderMap::new();

        for raw in headers {
            let (name, value) = raw
                .split_once(':')
                .with_context(|| format!("Invalid --header {raw:?}: expected \"Name: value\""))?;
            let name = name.trim();
            if name.is_empty() {
                bail!("Invalid --header {raw:?}: the header name is empty");
            }
            // Only the *first* space after the colon is separator; the rest of
            // the value is the user's. `-H "X-Trace:  a b "` means ` a b `
            // minus that one space, which is what curl does too.
            let value = value.strip_prefix(' ').unwrap_or(value);
            let name = HeaderName::try_from(name).with_context(|| {
                format!("Invalid --header {raw:?}: {name:?} is not a legal HTTP header name")
            })?;
            let value = HeaderValue::try_from(value).with_context(|| {
                format!("Invalid --header {raw:?}: the value is not legal in an HTTP header")
            })?;
            map.insert(name, value);
        }

        if let Some(cookie) = cookie {
            let value = HeaderValue::try_from(cookie)
                .context("Invalid --cookie: the value is not legal in an HTTP header")?;
            map.insert(COOKIE, value);
        }

        if let Some(ua) = user_agent {
            let value = HeaderValue::try_from(ua)
                .context("Invalid --user-agent: the value is not legal in an HTTP header")?;
            map.insert(USER_AGENT, value);
        }

        Ok(if map.is_empty() {
            CustomHeaders(None)
        } else {
            CustomHeaders(Some(Arc::new(map)))
        })
    }

    /// Whether anything was supplied.
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// How many distinct headers will be sent, for the `-v` line that tells
    /// the user their `-H` took effect.
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |map| map.keys_len())
    }

    /// The map, for [`super::client::HttpClientConfig::build`].
    pub(super) fn map(&self) -> Option<&HeaderMap> {
        self.0.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> CustomHeaders {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        CustomHeaders::parse(&owned, None, None).unwrap()
    }

    #[test]
    fn nothing_supplied_is_nothing_carried() {
        let headers = CustomHeaders::parse(&[], None, None).unwrap();
        assert!(headers.is_empty());
        assert_eq!(headers.len(), 0);
        assert!(headers.map().is_none());
    }

    #[test]
    fn a_header_is_split_on_the_first_colon_only() {
        // A value can legitimately contain a colon — `Referer: https://x/`
        // being the obvious one — so splitting on every colon would corrupt it.
        let headers = parsed(&["Referer: https://example.com/a"]);
        let map = headers.map().unwrap();
        assert_eq!(map["referer"], "https://example.com/a");
    }

    #[test]
    fn only_one_separator_space_is_eaten() {
        let headers = parsed(&["X-A:no-space", "X-B: one", "X-C:  two"]);
        let map = headers.map().unwrap();
        assert_eq!(map["x-a"], "no-space");
        assert_eq!(map["x-b"], "one");
        assert_eq!(map["x-c"], " two");
    }

    #[test]
    fn cookie_and_user_agent_get_their_own_flags() {
        let headers = CustomHeaders::parse(&[], Some("sid=abc; k=v"), Some("urx-test/1")).unwrap();
        let map = headers.map().unwrap();
        assert_eq!(headers.len(), 2);
        assert_eq!(map[COOKIE], "sid=abc; k=v");
        assert_eq!(map[USER_AGENT], "urx-test/1");
    }

    #[test]
    fn a_repeated_name_keeps_the_last_value() {
        // reqwest collapses duplicate default-header keys to the last one, so
        // this is the behaviour the user will observe either way.
        let headers = parsed(&["X-Env: staging", "X-Env: prod"]);
        assert_eq!(headers.map().unwrap()["x-env"], "prod");
    }

    #[test]
    fn a_user_agent_typed_as_a_header_counts_as_one() {
        let headers = parsed(&["User-Agent: from-H/1"]);
        assert_eq!(headers.map().unwrap()[USER_AGENT], "from-H/1");
    }

    #[test]
    fn the_dedicated_flag_wins_over_the_same_header_typed_by_hand() {
        // --user-agent is applied last on purpose: of the two spellings it is
        // the specific one, so it should be the one that decides.
        let raw = vec!["User-Agent: from-H/1".to_string()];
        let headers = CustomHeaders::parse(&raw, None, Some("from-flag/1")).unwrap();
        assert_eq!(headers.map().unwrap()[USER_AGENT], "from-flag/1");
    }

    #[test]
    fn malformed_headers_stop_the_run_rather_than_going_out_unnoticed() {
        // Each of these reads as "I sent an authenticated request" while
        // sending an anonymous one, so none of them may be dropped quietly.
        for bad in [
            "no-colon-at-all",
            ": empty name",
            "Bad Name: x",
            "X-Newline: a\nb",
        ] {
            let err = CustomHeaders::parse(&[bad.to_string()], None, None)
                .expect_err("{bad} should be rejected");
            // The argument is quoted back debug-escaped, so a value with a
            // newline in it stays on one readable line.
            assert!(
                format!("{err}").contains(&format!("{bad:?}")),
                "the error should name the offending argument: {err}"
            );
        }
    }

    #[test]
    fn an_empty_value_is_legal() {
        // `-H "X-Empty:"` is a real thing to want: some WAF rules key off the
        // presence of a header, not its content.
        let headers = parsed(&["X-Empty:"]);
        assert_eq!(headers.map().unwrap()["x-empty"], "");
    }
}
