//! Filters pushed **down into** the archive's CDX query, as opposed to the
//! post-processing filters in [`crate::filters`].
//!
//! A CDX index already records the HTTP status code and MIME type of every
//! capture, so asking the server to apply those predicates is free. Deriving
//! them locally would instead mean re-requesting every URL over the network —
//! which is what `--check-status` does, and why it stays a separate, opt-in
//! feature.
//!
//! # Two dialects, and why the difference matters
//!
//! The archives urx queries run two different CDX implementations. They
//! disagree on both field names *and* match semantics, and a wrong guess
//! produces no error — just silently empty results (on Arquivo.pt, an
//! unmatched regex made the server hang rather than return):
//!
//! | | status field | mime field | value semantics |
//! |---|---|---|---|
//! | [`CdxDialect::Classic`] — `web.archive.org` | `statuscode` | `mimetype` | regular expression |
//! | [`CdxDialect::Pywb`] — Common Crawl, Arquivo.pt | `status` | `mime` | exact string |
//!
//! Both negate with a `!` prefix, and both AND repeated `filter=` parameters
//! together. That AND is the right semantics for exclusions ("not 404 *and*
//! not 500") but the wrong one for a positive list ("200 *or* 301"), which is
//! why a multi-value positive list becomes one regex alternation on `Classic`
//! and is refused on `Pywb` — see [`ArchiveFilters::unsupported_positives`].

/// Which CDX implementation a provider's index server runs. See the module
/// docs — these are not interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdxDialect {
    /// The classic Internet Archive CDX server: `statuscode`/`mimetype`, and
    /// values are treated as regular expressions.
    Classic,
    /// pywb-derived index servers: `status`/`mime`, and values must match the
    /// captured field exactly.
    Pywb,
}

impl CdxDialect {
    /// Field name holding the captured HTTP status code.
    fn status_field(self) -> &'static str {
        match self {
            CdxDialect::Classic => "statuscode",
            CdxDialect::Pywb => "status",
        }
    }

    /// Field name holding the captured MIME type.
    fn mime_field(self) -> &'static str {
        match self {
            CdxDialect::Classic => "mimetype",
            CdxDialect::Pywb => "mime",
        }
    }

    /// Whether the server evaluates a filter value as a regular expression.
    /// Only regex dialects can express "any of these" in a single parameter.
    fn supports_regex(self) -> bool {
        matches!(self, CdxDialect::Classic)
    }
}

/// Percent-encode a filter value so regex metacharacters, `/`, and `+` reach
/// the server unmangled and cannot terminate the query parameter early.
fn encode_value(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Server-side CDX predicates shared by every archive that exposes a CDX API.
///
/// Values are stored exactly as the user typed them. They are *not* wrapped or
/// anchored: `^(200)$` is a valid regex to a [`CdxDialect::Classic`] server but
/// matches nothing on a [`CdxDialect::Pywb`] one, so rewriting user input would
/// break one dialect or the other.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveFilters {
    /// Only captures at or after this 14-digit CDX timestamp.
    pub from: Option<String>,
    /// Only captures at or before this 14-digit CDX timestamp.
    pub to: Option<String>,
    /// Keep only captures whose status code matches one of these.
    pub status: Vec<String>,
    /// Drop captures whose status code matches any of these.
    pub exclude_status: Vec<String>,
    /// Keep only captures whose MIME type matches one of these.
    pub mime: Vec<String>,
    /// Drop captures whose MIME type matches any of these.
    pub exclude_mime: Vec<String>,
}

impl ArchiveFilters {
    /// Build from raw CLI lists, dropping blank entries.
    pub fn from_cli_lists(
        from: Option<String>,
        to: Option<String>,
        status: &[String],
        exclude_status: &[String],
        mime: &[String],
        exclude_mime: &[String],
    ) -> Self {
        ArchiveFilters {
            from,
            to,
            status: clean(status),
            exclude_status: clean(exclude_status),
            mime: clean(mime),
            exclude_mime: clean(exclude_mime),
        }
    }

    /// True when nothing would be appended to a query.
    pub fn is_empty(&self) -> bool {
        self.from.is_none()
            && self.to.is_none()
            && self.status.is_empty()
            && self.exclude_status.is_empty()
            && self.mime.is_empty()
            && self.exclude_mime.is_empty()
    }

    /// Names of the positive predicates this dialect cannot express, so the
    /// caller can warn instead of silently returning nothing.
    ///
    /// An exact-match server ANDs repeated `filter=` parameters, so a
    /// multi-value positive list ("200 or 301") is unsatisfiable there and is
    /// dropped rather than sent. Single values and every exclusion list are
    /// always expressible.
    pub fn unsupported_positives(&self, dialect: CdxDialect) -> Vec<&'static str> {
        if dialect.supports_regex() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.status.len() > 1 {
            out.push("--archive-status");
        }
        if self.mime.len() > 1 {
            out.push("--archive-mime");
        }
        out
    }

    /// Render these filters as query-string parameters for `dialect`.
    ///
    /// The result is either empty or starts with `&`, so callers can append it
    /// to an already-parameterised query base without further checks.
    pub fn query_params(&self, dialect: CdxDialect) -> String {
        let mut out = String::new();

        if let Some(ts) = &self.from {
            out.push_str("&from=");
            out.push_str(ts);
        }
        if let Some(ts) = &self.to {
            out.push_str("&to=");
            out.push_str(ts);
        }

        push_positive(&mut out, dialect, dialect.status_field(), &self.status);
        push_negative(
            &mut out,
            dialect,
            dialect.status_field(),
            &self.exclude_status,
        );
        push_positive(&mut out, dialect, dialect.mime_field(), &self.mime);
        push_negative(&mut out, dialect, dialect.mime_field(), &self.exclude_mime);

        out
    }
}

/// Append one `filter=` parameter.
fn push_filter(out: &mut String, negate: bool, field: &str, value: &str) {
    out.push_str("&filter=");
    if negate {
        out.push('!');
    }
    out.push_str(field);
    out.push(':');
    out.push_str(&encode_value(value));
}

/// A positive list needs OR semantics, which repeated (AND-ed) parameters
/// cannot provide. One value is always fine; several collapse into a regex
/// alternation where the dialect supports it, and are dropped where it does not
/// (the caller warns via [`ArchiveFilters::unsupported_positives`]).
fn push_positive(out: &mut String, dialect: CdxDialect, field: &str, values: &[String]) {
    match values.len() {
        0 => {}
        1 => push_filter(out, false, field, &values[0]),
        _ if dialect.supports_regex() => {
            push_filter(out, false, field, &format!("({})", values.join("|")))
        }
        _ => {}
    }
}

/// An exclusion list wants AND semantics ("not this *and* not that"), which is
/// exactly what repeated parameters give — so every dialect handles any number
/// of values. The regex dialect still folds them into one parameter to keep the
/// query short.
fn push_negative(out: &mut String, dialect: CdxDialect, field: &str, values: &[String]) {
    match values.len() {
        0 => {}
        1 => push_filter(out, true, field, &values[0]),
        _ if dialect.supports_regex() => {
            push_filter(out, true, field, &format!("({})", values.join("|")))
        }
        _ => {
            for v in values {
                push_filter(out, true, field, v);
            }
        }
    }
}

/// Trim entries and drop blanks.
fn clean(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(String::from)
        .collect()
}

/// Normalise a user-supplied date into a 14-digit CDX timestamp
/// (`YYYYMMDDhhmmss`). Accepts `YYYY`, `YYYYMM`, `YYYYMMDD` and the full
/// 14-digit form. When `end_of_range` is true the missing tail is padded
/// toward the end of the range (`31 23:59:59`) rather than the start
/// (`01 00:00:00`) — pass `false` for `--from`, `true` for `--to`.
/// Returns `None` for malformed input so the CLI can warn.
pub fn normalize_cdx_timestamp(input: &str, end_of_range: bool) -> Option<String> {
    let digits: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    if !matches!(digits.len(), 4 | 6 | 8 | 14) {
        return None;
    }

    let year: u32 = digits.get(0..4)?.parse().ok()?;
    if !(1996..=9999).contains(&year) {
        // CDX coverage only starts in 1996; reject anything earlier.
        return None;
    }

    // Pad each segment toward the appropriate end of the range.
    let month = match digits.get(4..6) {
        Some(s) => {
            let m: u32 = s.parse().ok()?;
            if !(1..=12).contains(&m) {
                return None;
            }
            format!("{m:02}")
        }
        None => {
            if end_of_range {
                "12".to_string()
            } else {
                "01".to_string()
            }
        }
    };
    let day = match digits.get(6..8) {
        Some(s) => {
            let d: u32 = s.parse().ok()?;
            if !(1..=31).contains(&d) {
                return None;
            }
            format!("{d:02}")
        }
        None => {
            if end_of_range {
                // Widening is intended for `to`; CDX clamps impossible dates
                // (e.g. Feb 31) gracefully rather than rejecting them.
                "31".to_string()
            } else {
                "01".to_string()
            }
        }
    };
    let tail = match digits.get(8..14) {
        Some(s) => s.to_string(),
        None => {
            if end_of_range {
                "235959".to_string()
            } else {
                "000000".to_string()
            }
        }
    };

    Some(format!("{year:04}{month}{day}{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn filters(
        status: &[&str],
        excl_status: &[&str],
        mime: &[&str],
        excl_mime: &[&str],
    ) -> ArchiveFilters {
        ArchiveFilters::from_cli_lists(
            None,
            None,
            &s(status),
            &s(excl_status),
            &s(mime),
            &s(excl_mime),
        )
    }

    #[test]
    fn test_empty_filters_render_nothing() {
        let f = ArchiveFilters::default();
        assert!(f.is_empty());
        assert_eq!(f.query_params(CdxDialect::Classic), "");
        assert_eq!(f.query_params(CdxDialect::Pywb), "");
    }

    #[test]
    fn test_classic_dialect_field_names() {
        let q = filters(&["200"], &["404"], &["text/html"], &[]).query_params(CdxDialect::Classic);
        assert!(q.contains("&filter=statuscode:200"), "{q}");
        assert!(q.contains("&filter=!statuscode:404"), "{q}");
        assert!(q.contains("&filter=mimetype:text%2Fhtml"), "{q}");
        // pywb's names must never leak into a classic query.
        assert!(!q.contains("filter=status:"), "{q}");
        assert!(!q.contains("filter=mime:"), "{q}");
    }

    #[test]
    fn test_pywb_dialect_field_names() {
        let q = filters(&["200"], &[], &[], &["text/html"]).query_params(CdxDialect::Pywb);
        assert!(q.contains("&filter=status:200"), "{q}");
        assert!(q.contains("&filter=!mime:text%2Fhtml"), "{q}");
        assert!(!q.contains("statuscode"), "{q}");
        assert!(!q.contains("mimetype"), "{q}");
    }

    #[test]
    fn test_values_are_never_anchored_or_rewritten() {
        // An anchored regex matches nothing on pywb and once made Arquivo.pt
        // hang, so urx must send exactly what the user typed.
        let q = filters(&["200"], &[], &[], &[]).query_params(CdxDialect::Pywb);
        assert_eq!(q, "&filter=status:200");
    }

    #[test]
    fn test_classic_positive_list_becomes_one_alternation() {
        // Repeated filter= params are AND-ed, so a positive list must collapse
        // into a single regex alternation to mean OR.
        let q = filters(&["200", "301"], &[], &[], &[]).query_params(CdxDialect::Classic);
        assert_eq!(q.matches("&filter=").count(), 1, "{q}");
        assert!(q.contains("statuscode:%28200%7C301%29"), "{q}");
    }

    #[test]
    fn test_pywb_positive_list_is_dropped_not_sent_wrong() {
        // Exact-match servers cannot express OR; sending it anyway would yield
        // an empty result set that looks like "this domain has no 200s".
        let f = filters(&["200", "301"], &[], &[], &[]);
        assert_eq!(f.query_params(CdxDialect::Pywb), "");
        assert_eq!(
            f.unsupported_positives(CdxDialect::Pywb),
            vec!["--archive-status"]
        );
        // The regex dialect has no such limitation.
        assert!(f.unsupported_positives(CdxDialect::Classic).is_empty());
    }

    #[test]
    fn test_pywb_single_positive_is_supported() {
        let f = filters(&["200"], &[], &["text/html"], &[]);
        assert!(f.unsupported_positives(CdxDialect::Pywb).is_empty());
        assert_eq!(
            f.query_params(CdxDialect::Pywb).matches("&filter=").count(),
            2
        );
    }

    #[test]
    fn test_exclusion_list_works_on_both_dialects() {
        // "not 404 AND not 500" is exactly what repeated params mean, so every
        // dialect can express it — pywb as two params, classic folded into one.
        let f = filters(&[], &["404", "500"], &[], &[]);

        let pywb = f.query_params(CdxDialect::Pywb);
        assert_eq!(pywb.matches("&filter=").count(), 2, "{pywb}");
        assert!(
            pywb.contains("!status:404") && pywb.contains("!status:500"),
            "{pywb}"
        );

        let classic = f.query_params(CdxDialect::Classic);
        assert_eq!(classic.matches("&filter=").count(), 1, "{classic}");
        assert!(classic.contains("!statuscode:%28404%7C500%29"), "{classic}");

        assert!(f.unsupported_positives(CdxDialect::Pywb).is_empty());
    }

    #[test]
    fn test_values_are_percent_encoded() {
        // '+' in a MIME type would otherwise decode to a space server-side.
        let q = filters(&[], &[], &["application/xhtml+xml"], &[]).query_params(CdxDialect::Pywb);
        assert!(q.contains("%2B"), "'+' must be encoded: {q}");
        assert!(!q.contains("xhtml+xml"), "{q}");
    }

    #[test]
    fn test_value_cannot_inject_query_params() {
        let q = filters(&["200&limit=1"], &[], &[], &[]).query_params(CdxDialect::Classic);
        // Exactly one '&' — the one introducing our own filter param.
        assert_eq!(q.matches('&').count(), 1, "{q}");
    }

    #[test]
    fn test_blank_terms_are_dropped() {
        let f = filters(&["", "  "], &[], &[], &[]);
        assert!(f.status.is_empty());
        assert!(f.is_empty());
    }

    #[test]
    fn test_from_to_render_before_filters() {
        let f = ArchiveFilters::from_cli_lists(
            Some("20200101000000".to_string()),
            Some("20201231235959".to_string()),
            &s(&["200"]),
            &[],
            &[],
            &[],
        );
        let q = f.query_params(CdxDialect::Classic);
        assert!(
            q.starts_with("&from=20200101000000&to=20201231235959"),
            "{q}"
        );
    }

    #[test]
    fn test_normalize_cdx_timestamp_year_only() {
        assert_eq!(
            normalize_cdx_timestamp("2020", false).as_deref(),
            Some("20200101000000")
        );
        assert_eq!(
            normalize_cdx_timestamp("2020", true).as_deref(),
            Some("20201231235959")
        );
    }

    #[test]
    fn test_normalize_cdx_timestamp_year_month() {
        assert_eq!(
            normalize_cdx_timestamp("202003", false).as_deref(),
            Some("20200301000000")
        );
        assert_eq!(
            normalize_cdx_timestamp("202003", true).as_deref(),
            Some("20200331235959")
        );
    }

    #[test]
    fn test_normalize_cdx_timestamp_day_and_full() {
        assert_eq!(
            normalize_cdx_timestamp("20200315", false).as_deref(),
            Some("20200315000000")
        );
        assert_eq!(
            normalize_cdx_timestamp("20200315123045", false).as_deref(),
            Some("20200315123045")
        );
        // Separators are stripped before parsing.
        assert_eq!(
            normalize_cdx_timestamp("2020-03-15", false).as_deref(),
            Some("20200315000000")
        );
    }

    #[test]
    fn test_normalize_cdx_timestamp_rejects_invalid() {
        // Wrong digit count
        assert!(normalize_cdx_timestamp("20203", false).is_none());
        // Month out of range
        assert!(normalize_cdx_timestamp("202013", false).is_none());
        // Day out of range
        assert!(normalize_cdx_timestamp("20200300", false).is_none());
        // Before CDX coverage
        assert!(normalize_cdx_timestamp("1995", false).is_none());
        // Not a date at all
        assert!(normalize_cdx_timestamp("oops", false).is_none());
    }
}
