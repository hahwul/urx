//! Client-side filters over the archive metadata a run collected.
//!
//! urx already carries `first_seen`, `last_seen`, `mime`, `archive_status` and
//! `digest` for every URL a CDX provider reported (see
//! [`crate::providers::CaptureMeta`]), but until now the only way to *filter*
//! on those was [`crate::providers::filters`] — predicates pushed down into the
//! archive's own query. Those have two limits this module exists to lift:
//!
//! 1. They only reach CDX-backed providers. A run that also enables `otx` or
//!    `urlscan` gets a result set where half the URLs were filtered and half
//!    were not.
//! 2. The two CDX dialects disagree. pywb-derived servers match values exactly
//!    and AND repeated filters together, so a positive multi-value list
//!    (`--archive-status 200,301`) is unsatisfiable there and urx drops the
//!    filter with a warning rather than send a query that returns nothing.
//!
//! A filter that runs *after* collection has neither problem: it sees one
//! merged [`CaptureMeta`] per URL regardless of which provider produced it, and
//! "any of these" is just `Iterator::any`. The cost is that it cannot reduce
//! what the archive sends over the wire — which is what the archive-side
//! filters are for. The two are complementary, not alternatives.
//!
//! # URLs with no metadata
//!
//! Most URLs in a mixed run carry no metadata at all: the non-CDX providers
//! (`otx`, `vt`, `urlscan`, `github`, `bevigil`, `robots`, `sitemap`) have no
//! capture index, `--files` input is just a list of strings, and a cache hit
//! stores URLs only. Filtering those is a decision, not a lookup, and this
//! module splits it by the direction of the predicate:
//!
//! * A **positive** predicate (`--meta-mime`, `--meta-status`, any date bound)
//!   asks "is this value one of these?", which an absent value cannot answer.
//!   The URL is dropped, and reported separately as [`MetaVerdict::Unknown`] so
//!   the caller can say *why* — a run whose result set collapsed because it hit
//!   the cache is otherwise indistinguishable from a target with nothing to
//!   find.
//! * A **negative** predicate (`--meta-exclude-mime`, `--meta-exclude-status`)
//!   asks "is this value one of these?" in order to drop it. An absent value is
//!   not a match, so the URL survives. This is the same rule the rest of urx
//!   already follows: `--filter-regex` and `--exclude-patterns` drop only what
//!   positively matches, and `--exclude-status` keeps URLs whose status never
//!   resolved.

use anyhow::{bail, Result};

use crate::providers::{normalize_cdx_timestamp, CaptureMeta};

/// Whether one recorded status matches one user pattern.
///
/// The pattern language is the one `--include-status` established: three
/// characters, where `x` (either case) stands for any single character —
/// `200`, `20x`, `5xx`. The comparison is on the string the archive recorded
/// rather than a parsed `u16` because a CDX `statuscode` field is only *usually*
/// a number; a value urx cannot parse must simply not match, not panic.
///
/// This deliberately mirrors `StatusChecker::status_matches_pattern` instead of
/// calling it: that one is a private method on the live-request tester, and the
/// two filters are otherwise unrelated (one reads what the archive stored, the
/// other re-requests the URL now). See the handoff note — folding them into one
/// shared helper is worth doing once both sides are owned by the same change.
fn status_matches_pattern(recorded: &str, pattern: &str) -> bool {
    if recorded.len() != pattern.len() {
        return false;
    }
    recorded
        .chars()
        .zip(pattern.chars())
        .all(|(r, p)| p == 'x' || p == 'X' || p == r)
}

/// Reject a status pattern that could never match anything, so a typo is a
/// startup error rather than a silently empty result set.
///
/// HTTP status codes are three digits, so a three-character pattern is the only
/// shape `status_matches_pattern` can ever match — `--meta-status 2` would be
/// accepted and then quietly match nothing at all.
fn validate_status_pattern(pattern: &str) -> Result<()> {
    let ok = pattern.len() == 3
        && pattern
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'x' || c == 'X');
    if !ok {
        bail!(
            "Invalid status pattern {pattern:?}: expected a three-digit code (200) \
             or a wildcard where x stands for any digit (20x, 5xx)"
        );
    }
    Ok(())
}

/// The bare type of a MIME value, without any `; charset=…` parameters.
///
/// CDX normally records a bare `text/html`, but not always — Common Crawl in
/// particular passes the `Content-Type` header through, parameters and all.
/// Comparing the raw field would make `--meta-mime text/html` miss exactly the
/// captures that recorded the most detail.
fn mime_essence(value: &str) -> &str {
    value.split(';').next().unwrap_or(value).trim()
}

/// Whether one recorded MIME type matches one user pattern.
///
/// Case-insensitive (MIME types are, per RFC 2045), compared against the
/// essence rather than the raw field, and a trailing `*` makes the rest of the
/// pattern a prefix — which is what lets `image/*` mean "any image" without
/// spelling out a subtype list.
fn mime_matches_pattern(recorded: &str, pattern: &str) -> bool {
    let recorded = mime_essence(recorded);
    match pattern.strip_suffix('*') {
        // `str::get` rather than a slice: a MIME value is not guaranteed ASCII,
        // and indexing one mid-character would panic instead of failing to
        // match. A non-boundary index simply yields `None`, i.e. no match.
        Some(prefix) => recorded
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix)),
        None => recorded.eq_ignore_ascii_case(pattern),
    }
}

/// Split, trim and lower-case a repeated comma-separated flag.
///
/// Blank entries are dropped rather than kept: an empty MIME pattern is a
/// prefix of nothing on the positive side but, on the exclusion side, `""` with
/// a trailing-`*` reading would drop every URL — the same trailing-comma
/// footgun [`crate::filters::url_filter`] guards against for `--patterns`.
fn normalize_values(raw: &[String]) -> Vec<String> {
    raw.iter()
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// What [`MetaFilter`] decided about one URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaVerdict {
    /// Every configured predicate was satisfied.
    Keep,
    /// The URL carried the metadata and it failed a predicate.
    Reject,
    /// A positive predicate was configured and this URL has no value for the
    /// field at all — see the module docs. Counted apart from [`Self::Reject`]
    /// because it usually means the *run*, not the URL, is the problem.
    Unknown,
}

/// How a whole result set fared, for the `--verbose` report.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MetaFilterStats {
    /// URLs that satisfied every predicate.
    pub kept: usize,
    /// URLs whose metadata failed a predicate.
    pub rejected: usize,
    /// URLs dropped only because they carry no metadata to test.
    pub unknown: usize,
}

impl MetaFilterStats {
    fn record(&mut self, verdict: MetaVerdict) {
        match verdict {
            MetaVerdict::Keep => self.kept += 1,
            MetaVerdict::Reject => self.rejected += 1,
            MetaVerdict::Unknown => self.unknown += 1,
        }
    }

    /// True when the only thing that stood between this run and a result was
    /// missing metadata. That is the shape of a cache hit or a `--files` run,
    /// and is worth saying out loud even without `--verbose`.
    pub fn dropped_everything_for_lack_of_metadata(&self) -> bool {
        self.kept == 0 && self.rejected == 0 && self.unknown > 0
    }
}

/// Post-collection predicates over [`CaptureMeta`].
///
/// Empty by default, in which case [`MetaFilter::verdict`] keeps everything and
/// [`MetaFilter::is_empty`] lets the caller skip the pass entirely.
#[derive(Debug, Default, Clone)]
pub struct MetaFilter {
    /// Inclusive lower bound on `first_seen`, 14-digit CDX form.
    first_seen_after: Option<String>,
    /// Inclusive upper bound on `first_seen`.
    first_seen_before: Option<String>,
    last_seen_after: Option<String>,
    last_seen_before: Option<String>,
    /// Keep only these MIME types; empty means "no opinion".
    mime: Vec<String>,
    /// Drop these MIME types.
    exclude_mime: Vec<String>,
    /// Keep only these archive statuses.
    archive_status: Vec<String>,
    /// Drop these archive statuses.
    exclude_archive_status: Vec<String>,
}

impl MetaFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bound the `first_seen` timestamp.
    ///
    /// Dates are normalised exactly as `--from`/`--to` are, so a partial date
    /// widens toward the end of the range it bounds: `--meta-first-seen-after
    /// 2020` means "on or after 2020-01-01 00:00:00" and
    /// `--meta-first-seen-before 2020` means "on or before 2020-12-31 23:59:59".
    /// Giving both therefore reads as "first seen during 2020" rather than as
    /// an empty window.
    pub fn with_first_seen(
        &mut self,
        after: Option<&str>,
        before: Option<&str>,
    ) -> Result<&mut Self> {
        self.first_seen_after = parse_bound(after, "--meta-first-seen-after", false)?;
        self.first_seen_before = parse_bound(before, "--meta-first-seen-before", true)?;
        Ok(self)
    }

    /// Bound the `last_seen` timestamp. Same padding rules as
    /// [`MetaFilter::with_first_seen`].
    pub fn with_last_seen(
        &mut self,
        after: Option<&str>,
        before: Option<&str>,
    ) -> Result<&mut Self> {
        self.last_seen_after = parse_bound(after, "--meta-last-seen-after", false)?;
        self.last_seen_before = parse_bound(before, "--meta-last-seen-before", true)?;
        Ok(self)
    }

    /// Keep only `keep` MIME types and drop `drop` ones. Several values OR
    /// together on each side, and the exclusion is applied first.
    pub fn with_mime(&mut self, keep: &[String], drop: &[String]) -> &mut Self {
        self.mime = normalize_values(keep);
        self.exclude_mime = normalize_values(drop);
        self
    }

    /// Keep only `keep` archive statuses and drop `drop` ones. Values are the
    /// `--include-status` pattern language (`200`, `20x`, `5xx`); an
    /// unmatchable pattern is an error rather than a silent no-op.
    pub fn with_archive_status(&mut self, keep: &[String], drop: &[String]) -> Result<&mut Self> {
        let keep = normalize_values(keep);
        let drop = normalize_values(drop);
        for pattern in keep.iter().chain(drop.iter()) {
            validate_status_pattern(pattern)?;
        }
        self.archive_status = keep;
        self.exclude_archive_status = drop;
        Ok(self)
    }

    /// True when no predicate is configured, so the whole pass can be skipped.
    pub fn is_empty(&self) -> bool {
        self.first_seen_after.is_none()
            && self.first_seen_before.is_none()
            && self.last_seen_after.is_none()
            && self.last_seen_before.is_none()
            && self.mime.is_empty()
            && self.exclude_mime.is_empty()
            && self.archive_status.is_empty()
            && self.exclude_archive_status.is_empty()
    }

    /// Judge one URL's metadata. See the module docs for how an absent value is
    /// treated on each side. Private because [`MetaFilter::apply`] is the whole
    /// public surface: it is the one that also counts *why* each URL was
    /// dropped, which is the number callers actually need.
    fn verdict(&self, meta: &CaptureMeta) -> MetaVerdict {
        // Exclusions first, and only on a value that is actually present: "not
        // an image" is not a claim we can make about a URL with no MIME type.
        if let Some(recorded) = meta.mime() {
            if self
                .exclude_mime
                .iter()
                .any(|p| mime_matches_pattern(recorded, p))
            {
                return MetaVerdict::Reject;
            }
        }
        if let Some(recorded) = meta.archive_status() {
            if self
                .exclude_archive_status
                .iter()
                .any(|p| status_matches_pattern(recorded, p))
            {
                return MetaVerdict::Reject;
            }
        }

        if !self.mime.is_empty() {
            match meta.mime() {
                None => return MetaVerdict::Unknown,
                Some(recorded) => {
                    if !self.mime.iter().any(|p| mime_matches_pattern(recorded, p)) {
                        return MetaVerdict::Reject;
                    }
                }
            }
        }

        if !self.archive_status.is_empty() {
            match meta.archive_status() {
                None => return MetaVerdict::Unknown,
                Some(recorded) => {
                    if !self
                        .archive_status
                        .iter()
                        .any(|p| status_matches_pattern(recorded, p))
                    {
                        return MetaVerdict::Reject;
                    }
                }
            }
        }

        let first_seen = within_window(
            meta.first_seen(),
            self.first_seen_after.as_deref(),
            self.first_seen_before.as_deref(),
        );
        if first_seen != MetaVerdict::Keep {
            return first_seen;
        }

        within_window(
            meta.last_seen(),
            self.last_seen_after.as_deref(),
            self.last_seen_before.as_deref(),
        )
    }

    /// Judge a whole result set, returning the survivors in the order given
    /// alongside the counts behind the decision.
    ///
    /// `meta_of` resolves a URL to its metadata; a URL the run never reported
    /// (one the link extractor produced, say) has none, which the closure
    /// signals with an absent value and which is judged exactly like a URL from
    /// a provider with no capture index.
    pub fn apply<'a, F>(&self, urls: Vec<String>, meta_of: F) -> (Vec<String>, MetaFilterStats)
    where
        F: Fn(&str) -> Option<&'a CaptureMeta>,
    {
        let empty = CaptureMeta::default();
        let mut stats = MetaFilterStats::default();
        let kept = urls
            .into_iter()
            .filter(|url| {
                let meta = meta_of(url).unwrap_or(&empty);
                let verdict = self.verdict(meta);
                stats.record(verdict);
                verdict == MetaVerdict::Keep
            })
            .collect();
        (kept, stats)
    }
}

/// Normalise one date bound, naming the flag in the error.
///
/// Unlike `--from`/`--to`, which warn and carry on because a dropped
/// archive-side filter still produces a usable (if wider) result, a bad bound
/// here is fatal: this filter decides the final result set, so silently
/// ignoring the bound would hand back URLs the user explicitly excluded.
fn parse_bound(raw: Option<&str>, flag: &str, end_of_range: bool) -> Result<Option<String>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    match normalize_cdx_timestamp(raw, end_of_range) {
        Some(ts) => Ok(Some(ts)),
        None => bail!(
            "Invalid date in {flag}={raw:?}: expected YYYY, YYYYMM, YYYYMMDD, or YYYYMMDDhhmmss"
        ),
    }
}

/// Test one timestamp against an optional inclusive window.
///
/// The comparison is lexicographic, which is chronological because every stored
/// timestamp is the 14-digit CDX form and every bound was padded to it — see
/// `CaptureMeta`'s `clean_timestamp`, which drops any capture timestamp that is
/// not exactly 14 digits for this reason.
fn within_window(value: Option<&str>, after: Option<&str>, before: Option<&str>) -> MetaVerdict {
    if after.is_none() && before.is_none() {
        return MetaVerdict::Keep;
    }
    let Some(value) = value else {
        return MetaVerdict::Unknown;
    };
    if after.is_some_and(|bound| value < bound) || before.is_some_and(|bound| value > bound) {
        return MetaVerdict::Reject;
    }
    MetaVerdict::Keep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(ts: &str, mime: &str, status: &str) -> CaptureMeta {
        CaptureMeta::capture(
            Some(ts),
            (!mime.is_empty()).then_some(mime),
            (!status.is_empty()).then_some(status),
            None,
        )
    }

    fn v(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_unconfigured_filter_keeps_everything_including_bare_urls() {
        let filter = MetaFilter::new();
        assert!(filter.is_empty());
        assert_eq!(
            filter.verdict(&CaptureMeta::default()),
            MetaVerdict::Keep,
            "a URL with no metadata must survive a filter that asks nothing of it"
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html", "200")),
            MetaVerdict::Keep
        );
    }

    #[test]
    fn a_partial_date_pads_toward_the_end_of_the_range_it_bounds() {
        let mut filter = MetaFilter::new();
        filter.with_first_seen(Some("2020"), Some("2020")).unwrap();
        assert_eq!(filter.first_seen_after.as_deref(), Some("20200101000000"));
        assert_eq!(filter.first_seen_before.as_deref(), Some("20201231235959"));

        // ...so naming the same year twice means "during 2020", not "never".
        assert_eq!(
            filter.verdict(&meta("20200615120000", "", "")),
            MetaVerdict::Keep
        );
        assert_eq!(
            filter.verdict(&meta("20191231235959", "", "")),
            MetaVerdict::Reject
        );
        assert_eq!(
            filter.verdict(&meta("20210101000000", "", "")),
            MetaVerdict::Reject
        );
    }

    #[test]
    fn date_bounds_are_inclusive_at_both_ends() {
        let mut filter = MetaFilter::new();
        filter
            .with_last_seen(Some("20200101000000"), Some("20201231235959"))
            .unwrap();
        assert_eq!(
            filter.verdict(&meta("20200101000000", "", "")),
            MetaVerdict::Keep
        );
        assert_eq!(
            filter.verdict(&meta("20201231235959", "", "")),
            MetaVerdict::Keep
        );
    }

    #[test]
    fn first_and_last_seen_are_bounded_independently() {
        // "went live before 2010 and was still alive after 2020" — a query that
        // needs both fields and cannot be expressed archive-side at all.
        let mut filter = MetaFilter::new();
        filter.with_first_seen(None, Some("2009")).unwrap();
        filter.with_last_seen(Some("2020"), None).unwrap();

        let mut long_lived = meta("20050101000000", "", "");
        long_lived.merge(&meta("20240101000000", "", ""));
        assert_eq!(filter.verdict(&long_lived), MetaVerdict::Keep);

        // Old, but dead before 2020.
        let mut short_lived = meta("20050101000000", "", "");
        short_lived.merge(&meta("20060101000000", "", ""));
        assert_eq!(filter.verdict(&short_lived), MetaVerdict::Reject);

        // Alive today, but too young.
        assert_eq!(
            filter.verdict(&meta("20240101000000", "", "")),
            MetaVerdict::Reject
        );
    }

    #[test]
    fn a_malformed_date_is_a_startup_error_not_a_dropped_filter() {
        let err = MetaFilter::new()
            .with_first_seen(Some("nope"), None)
            .expect_err("a bound urx cannot parse must not be silently ignored");
        assert!(err.to_string().contains("--meta-first-seen-after"), "{err}");

        // 1995 predates every CDX index, which `normalize_cdx_timestamp` rejects.
        assert!(MetaFilter::new()
            .with_last_seen(None, Some("1995"))
            .is_err());
    }

    #[test]
    fn a_blank_date_is_no_bound_at_all() {
        let mut filter = MetaFilter::new();
        filter.with_first_seen(Some("  "), Some("")).unwrap();
        assert!(filter.is_empty());
    }

    #[test]
    fn a_positive_mime_list_ors_its_values_together() {
        // The case the archive-side filter cannot express on pywb servers.
        let mut filter = MetaFilter::new();
        filter.with_mime(&v(&["text/html", "application/json"]), &[]);

        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html", "")),
            MetaVerdict::Keep
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "application/json", "")),
            MetaVerdict::Keep
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "image/png", "")),
            MetaVerdict::Reject
        );
    }

    #[test]
    fn mime_matching_ignores_case_and_content_type_parameters() {
        let mut filter = MetaFilter::new();
        filter.with_mime(&v(&["TEXT/HTML"]), &[]);
        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html; charset=UTF-8", "")),
            MetaVerdict::Keep
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "Text/Html", "")),
            MetaVerdict::Keep
        );
    }

    #[test]
    fn a_trailing_star_makes_the_mime_pattern_a_prefix() {
        let mut filter = MetaFilter::new();
        filter.with_mime(&v(&["image/*"]), &[]);
        assert_eq!(
            filter.verdict(&meta("20200101000000", "image/png", "")),
            MetaVerdict::Keep
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "image/svg+xml", "")),
            MetaVerdict::Keep
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html", "")),
            MetaVerdict::Reject
        );
        // A prefix, not a substring: the pattern still has to start the value.
        assert_eq!(
            filter.verdict(&meta("20200101000000", "application/image/png", "")),
            MetaVerdict::Reject
        );
    }

    #[test]
    fn a_star_pattern_shorter_than_nothing_does_not_panic_on_multibyte_values() {
        // `image/*` compared against a value whose bytes are not ASCII must not
        // slice mid-character.
        let mut filter = MetaFilter::new();
        filter.with_mime(&v(&["im*"]), &[]);
        assert_eq!(
            filter.verdict(&meta("20200101000000", "ímage/png", "")),
            MetaVerdict::Reject
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "i", "")),
            MetaVerdict::Reject
        );
    }

    #[test]
    fn status_patterns_accept_wildcards_and_reject_typos() {
        let mut filter = MetaFilter::new();
        filter
            .with_archive_status(&v(&["20x", "301"]), &[])
            .unwrap();
        for keep in ["200", "204", "301"] {
            assert_eq!(
                filter.verdict(&meta("20200101000000", "", keep)),
                MetaVerdict::Keep,
                "{keep}"
            );
        }
        for reject in ["210", "302", "404"] {
            assert_eq!(
                filter.verdict(&meta("20200101000000", "", reject)),
                MetaVerdict::Reject,
                "{reject}"
            );
        }

        // A pattern that could never match is a startup error, not an empty run.
        let err = MetaFilter::new()
            .with_archive_status(&v(&["2"]), &[])
            .expect_err("a one-character status pattern matches no HTTP code");
        assert!(err.to_string().contains("20x"), "{err}");
        assert!(MetaFilter::new()
            .with_archive_status(&[], &v(&["4o4"]))
            .is_err());
    }

    #[test]
    fn a_non_numeric_archive_status_simply_fails_to_match() {
        // CDX `statuscode` is only usually a number.
        let mut filter = MetaFilter::new();
        filter.with_archive_status(&v(&["2xx"]), &[]).unwrap();
        assert_eq!(
            filter.verdict(&meta("20200101000000", "", "revisit")),
            MetaVerdict::Reject
        );
    }

    #[test]
    fn exclusions_drop_only_what_positively_matches() {
        let mut filter = MetaFilter::new();
        filter.with_mime(&[], &v(&["image/*"]));
        filter
            .with_archive_status(&[], &v(&["4xx", "5xx"]))
            .unwrap();

        assert_eq!(
            filter.verdict(&meta("20200101000000", "image/png", "200")),
            MetaVerdict::Reject
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html", "404")),
            MetaVerdict::Reject
        );
        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html", "200")),
            MetaVerdict::Keep
        );
        // ...and a URL with nothing to test survives an exclusion, because
        // "not an image" is not a claim we can make about an unknown type.
        assert_eq!(filter.verdict(&CaptureMeta::default()), MetaVerdict::Keep);
    }

    #[test]
    fn a_positive_predicate_drops_metadata_free_urls_as_unknown() {
        for filter in [
            {
                let mut f = MetaFilter::new();
                f.with_mime(&v(&["text/html"]), &[]);
                f
            },
            {
                let mut f = MetaFilter::new();
                f.with_archive_status(&v(&["200"]), &[]).unwrap();
                f
            },
            {
                let mut f = MetaFilter::new();
                f.with_last_seen(Some("2020"), None).unwrap();
                f
            },
        ] {
            assert_eq!(
                filter.verdict(&CaptureMeta::default()),
                MetaVerdict::Unknown,
                "an absent value cannot satisfy a positive predicate"
            );
        }
    }

    #[test]
    fn a_field_the_url_lacks_is_unknown_even_when_its_siblings_are_known() {
        // A Wayback `warc/revisit` row records a timestamp but no status; a
        // status predicate must not read that as "not 200".
        let mut filter = MetaFilter::new();
        filter.with_archive_status(&v(&["200"]), &[]).unwrap();
        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html", "")),
            MetaVerdict::Unknown
        );
    }

    #[test]
    fn apply_reports_why_each_url_was_dropped() {
        let known = meta("20200101000000", "text/html", "200");
        let wrong_mime = meta("20200101000000", "image/png", "200");

        let mut filter = MetaFilter::new();
        filter.with_mime(&v(&["text/html"]), &[]);

        let urls = vec![
            "https://example.com/keep".to_string(),
            "https://example.com/wrong".to_string(),
            "https://example.com/bare".to_string(),
        ];
        let (kept, stats) = filter.apply(urls, |url| match url {
            "https://example.com/keep" => Some(&known),
            "https://example.com/wrong" => Some(&wrong_mime),
            _ => None,
        });

        assert_eq!(kept, vec!["https://example.com/keep".to_string()]);
        assert_eq!(
            stats,
            MetaFilterStats {
                kept: 1,
                rejected: 1,
                unknown: 1,
            }
        );
        assert!(!stats.dropped_everything_for_lack_of_metadata());
    }

    #[test]
    fn a_result_set_with_no_metadata_at_all_is_reported_as_such() {
        // The shape of a cache hit or a --files run: nothing failed a
        // predicate, everything simply had nothing to test.
        let mut filter = MetaFilter::new();
        filter.with_last_seen(None, Some("2010")).unwrap();

        let urls = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        let (kept, stats) = filter.apply(urls, |_| None);

        assert!(kept.is_empty());
        assert_eq!(stats.unknown, 2);
        assert!(stats.dropped_everything_for_lack_of_metadata());
    }

    #[test]
    fn apply_preserves_the_order_it_was_given() {
        let known = meta("20200101000000", "text/html", "200");
        let mut filter = MetaFilter::new();
        filter.with_mime(&v(&["text/html"]), &[]);
        let urls = vec![
            "https://example.com/c".to_string(),
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ];
        let (kept, _) = filter.apply(urls.clone(), |_| Some(&known));
        assert_eq!(kept, urls);
    }

    #[test]
    fn comma_separated_values_split_and_blank_entries_drop_out() {
        let mut filter = MetaFilter::new();
        filter.with_mime(&v(&["text/html, application/json,"]), &[]);
        assert_eq!(filter.mime, vec!["text/html", "application/json"]);

        // A trailing comma on the exclusion side used to be the dangerous case:
        // an empty pattern must not become "drop everything".
        let mut filter = MetaFilter::new();
        filter.with_mime(&[], &v(&["image/*,"]));
        assert_eq!(filter.exclude_mime, vec!["image/*"]);
        assert_eq!(
            filter.verdict(&meta("20200101000000", "text/html", "")),
            MetaVerdict::Keep
        );
    }
}
