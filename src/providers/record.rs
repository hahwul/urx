//! What a provider reports about a URL, beyond the URL itself.
//!
//! A CDX row already carries the capture timestamp, MIME type, HTTP status and
//! content digest of every capture. The bytes arrive whether we read them or
//! not, so dropping them at parse time throws away the only cheap source of
//! "when was this alive, and what was it" the tool has.
//!
//! # Aggregation happens twice, with one rule
//!
//! An archive indexes *captures*, not URLs: one URL routinely has thousands of
//! rows. Providers therefore fold their rows down to one [`UrlRecord`] per URL
//! as they page — that is what [`RecordSet`] is for, and returning every
//! capture instead would mean holding millions of rows for a large domain —
//! and the runner then folds those per-provider records together across
//! providers. Both levels call the same [`CaptureMeta::merge`], so a URL seen
//! once by Wayback and once by Arquivo aggregates exactly like a URL captured
//! twice by Wayback alone.
//!
//! The merge is order-independent by construction — `first_seen`/`last_seen`
//! are a min/max, and the single-valued fields are decided by a total order on
//! `(timestamp, value)` — so the output does not depend on which provider
//! happened to answer first.

use std::collections::{BTreeSet, HashMap};

/// CDX writes a field it has no value for as a single dash. Treat that, and an
/// empty string, as "absent" rather than letting `"-"` reach the output.
fn clean_field(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        None
    } else {
        Some(value.to_string())
    }
}

/// Accept a capture timestamp only in the canonical 14-digit CDX form
/// (`YYYYMMDDhhmmss`). Everything downstream — the min/max that build
/// `first_seen`/`last_seen`, and the comparison that picks the most recent
/// MIME type — compares these as plain strings, which is only correct while
/// every value has the same width and alphabet.
fn clean_timestamp(value: &str) -> Option<String> {
    let value = clean_field(value)?;
    if value.len() == 14 && value.bytes().all(|b| b.is_ascii_digit()) {
        Some(value)
    } else {
        None
    }
}

/// A single-valued field together with the capture timestamp it came from, so
/// that merging keeps the value from the most recent capture.
///
/// Ordering on the `(timestamp, value)` pair is what makes the result
/// independent of merge order. `None` sorts below `Some`, so a capture that
/// carried no timestamp only ever fills an empty slot, and an exact tie on the
/// timestamp is broken by the value itself rather than by arrival order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Dated {
    value: Option<String>,
    at: Option<String>,
}

impl Dated {
    fn sort_key(&self) -> (Option<&str>, Option<&str>) {
        (self.at.as_deref(), self.value.as_deref())
    }

    /// Replace the held value when `value` comes from a strictly "later" pair.
    /// A `None` value is never an improvement and is ignored outright.
    fn offer(&mut self, value: Option<&str>, at: Option<&str>) {
        if value.is_none() {
            return;
        }
        if (at, value) > self.sort_key() {
            self.value = value.map(str::to_string);
            self.at = at.map(str::to_string);
        }
    }

    fn merge(&mut self, other: &Dated) {
        self.offer(other.value.as_deref(), other.at.as_deref());
    }

    fn get(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// The value together with the timestamp of the capture it came from.
    fn dated(&self) -> Option<(&str, &str)> {
        Some((self.at.as_deref()?, self.value.as_deref()?))
    }
}

/// Archive metadata for one URL, aggregated over every capture that reported
/// it. Built empty for providers that have nothing to say (see
/// [`UrlRecord::bare`]) — an absent field stays absent all the way to the
/// output rather than being invented.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureMeta {
    /// Oldest capture timestamp, 14-digit CDX form.
    first_seen: Option<String>,
    /// Newest capture timestamp, 14-digit CDX form.
    last_seen: Option<String>,
    /// MIME type recorded by the most recent capture that had one.
    mime: Dated,
    /// HTTP status the *archive* recorded at capture time. Deliberately
    /// distinct from the `status` `--check-status` produces, which is a live
    /// re-request made now.
    archive_status: Dated,
    /// Content digests over every capture. A URL whose digest never changes
    /// was never edited; one with many was. Kept as a set because that is what
    /// the data is — the output picks a single representative.
    digests: BTreeSet<String>,
    /// Digest of the most recent capture that recorded one, dated by that
    /// capture. `digests` alone cannot say *which* body a given timestamp
    /// served, and `--archive-body` needs exactly that pairing: it replays the
    /// newest capture and must know its digest to skip other URLs whose newest
    /// capture served the same bytes.
    newest_digest: Dated,
}

impl CaptureMeta {
    /// Build the metadata of one CDX capture. Values are taken as the archive
    /// wrote them, minus CDX's `-` null marker.
    pub fn capture(
        timestamp: Option<&str>,
        mime: Option<&str>,
        archive_status: Option<&str>,
        digest: Option<&str>,
    ) -> Self {
        let timestamp = timestamp.and_then(clean_timestamp);
        let mime = mime.and_then(clean_field);
        let archive_status = archive_status.and_then(clean_field);

        CaptureMeta {
            first_seen: timestamp.clone(),
            last_seen: timestamp.clone(),
            mime: Dated {
                value: mime,
                at: timestamp.clone(),
            },
            archive_status: Dated {
                value: archive_status,
                at: timestamp.clone(),
            },
            digests: digest.and_then(clean_field).into_iter().collect(),
            newest_digest: Dated {
                value: digest.and_then(clean_field),
                at: timestamp,
            },
        }
    }

    /// True when there is nothing to report, i.e. this URL came from a
    /// provider that has no capture index (or from a cache hit, which stores
    /// URLs only).
    pub fn is_empty(&self) -> bool {
        self.first_seen.is_none()
            && self.last_seen.is_none()
            && self.mime.get().is_none()
            && self.archive_status.get().is_none()
            && self.digests.is_empty()
    }

    /// Fold another view of the same URL in: `first_seen`/`last_seen` widen to
    /// cover both, the single-valued fields keep the most recent capture's
    /// value, and the digests union.
    pub fn merge(&mut self, other: &CaptureMeta) {
        if let Some(ts) = &other.first_seen {
            if self
                .first_seen
                .as_deref()
                .is_none_or(|cur| ts.as_str() < cur)
            {
                self.first_seen = Some(ts.clone());
            }
        }
        if let Some(ts) = &other.last_seen {
            if self
                .last_seen
                .as_deref()
                .is_none_or(|cur| ts.as_str() > cur)
            {
                self.last_seen = Some(ts.clone());
            }
        }
        self.mime.merge(&other.mime);
        self.archive_status.merge(&other.archive_status);
        self.digests.extend(other.digests.iter().cloned());
        self.newest_digest.merge(&other.newest_digest);
    }

    /// Oldest capture timestamp, 14-digit CDX form.
    pub fn first_seen(&self) -> Option<&str> {
        self.first_seen.as_deref()
    }

    /// Newest capture timestamp, 14-digit CDX form.
    pub fn last_seen(&self) -> Option<&str> {
        self.last_seen.as_deref()
    }

    /// MIME type of the most recent capture that recorded one.
    pub fn mime(&self) -> Option<&str> {
        self.mime.get()
    }

    /// HTTP status the archive recorded for the most recent capture that had
    /// one. Not a live status — see [`CaptureMeta::archive_status`]'s field
    /// documentation.
    pub fn archive_status(&self) -> Option<&str> {
        self.archive_status.get()
    }

    /// One representative content digest. A URL can have as many digests as it
    /// had distinct bodies; the smallest is picked so repeated runs over the
    /// same data agree.
    pub fn digest(&self) -> Option<&str> {
        self.digests.iter().next().map(String::as_str)
    }

    /// Every distinct content digest seen for this URL.
    pub fn digests(&self) -> impl Iterator<Item = &str> {
        self.digests.iter().map(String::as_str)
    }

    /// The `(timestamp, digest)` of the most recent capture that recorded a
    /// digest — the capture `--archive-body` replays, paired with the identity
    /// of the body that replay will return.
    pub fn newest_capture(&self) -> Option<(&str, &str)> {
        self.newest_digest.dated()
    }
}

/// One URL as a provider reported it, carrying whatever archive metadata that
/// provider had. Providers without a capture index return [`UrlRecord::bare`]
/// records — an empty [`CaptureMeta`], never a fabricated one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlRecord {
    pub url: String,
    pub meta: CaptureMeta,
}

impl UrlRecord {
    /// A URL with no archive metadata attached.
    pub fn bare(url: String) -> Self {
        UrlRecord {
            url,
            meta: CaptureMeta::default(),
        }
    }

    /// A URL together with the metadata of the captures behind it.
    pub fn new(url: String, meta: CaptureMeta) -> Self {
        UrlRecord { url, meta }
    }
}

/// Accumulator that folds capture rows into one record per URL as they arrive.
///
/// Providers page through *capture-level* indexes: the same URL recurs across
/// pages (Arquivo returns one row per capture and ignores `collapse=urlkey`
/// entirely), so merging on arrival is what keeps peak memory proportional to
/// the number of distinct URLs rather than to the number of captures — which
/// for a large domain differ by orders of magnitude.
#[derive(Debug, Default)]
pub struct RecordSet {
    by_url: HashMap<String, CaptureMeta>,
}

impl RecordSet {
    pub fn new() -> Self {
        RecordSet::default()
    }

    /// Fold one capture row in, merging with any previous capture of the same
    /// URL.
    pub fn insert(&mut self, record: UrlRecord) {
        self.by_url
            .entry(record.url)
            .or_default()
            .merge(&record.meta);
    }

    /// Fold a whole page of rows in.
    pub fn extend(&mut self, records: impl IntoIterator<Item = UrlRecord>) {
        for record in records {
            self.insert(record);
        }
    }

    /// Number of distinct URLs held.
    pub fn len(&self) -> usize {
        self.by_url.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_url.is_empty()
    }

    /// The records, one per URL, sorted by URL — the order every provider
    /// returns and the batch path expects.
    pub fn into_sorted(self) -> Vec<UrlRecord> {
        let mut out: Vec<UrlRecord> = self
            .by_url
            .into_iter()
            .map(|(url, meta)| UrlRecord::new(url, meta))
            .collect();
        out.sort_by(|a, b| a.url.cmp(&b.url));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(ts: &str, mime: &str, status: &str, digest: &str) -> CaptureMeta {
        CaptureMeta::capture(Some(ts), Some(mime), Some(status), Some(digest))
    }

    #[test]
    fn bare_records_carry_nothing() {
        let record = UrlRecord::bare("https://example.com".to_string());
        assert!(record.meta.is_empty());
        assert_eq!(record.meta.first_seen(), None);
        assert_eq!(record.meta.mime(), None);
        assert_eq!(record.meta.digest(), None);
    }

    #[test]
    fn a_single_capture_is_both_first_and_last_seen() {
        let meta = capture("20200101000000", "text/html", "200", "ABC");
        assert_eq!(meta.first_seen(), Some("20200101000000"));
        assert_eq!(meta.last_seen(), Some("20200101000000"));
        assert_eq!(meta.mime(), Some("text/html"));
        assert_eq!(meta.archive_status(), Some("200"));
        assert_eq!(meta.digest(), Some("ABC"));
        assert!(!meta.is_empty());
    }

    #[test]
    fn cdx_null_markers_do_not_reach_the_output() {
        let meta = CaptureMeta::capture(Some("20200101000000"), Some("-"), Some(""), Some("-"));
        assert_eq!(meta.mime(), None);
        assert_eq!(meta.archive_status(), None);
        assert_eq!(meta.digest(), None);
        assert_eq!(meta.first_seen(), Some("20200101000000"));
    }

    #[test]
    fn a_malformed_timestamp_is_dropped_rather_than_compared() {
        // Comparing "2020" against "20200101000000" as strings would make the
        // short value look older than every real one.
        let meta = CaptureMeta::capture(Some("2020"), Some("text/html"), None, None);
        assert_eq!(meta.first_seen(), None);
        assert_eq!(meta.last_seen(), None);
        // The MIME type survives; only the unusable timestamp is dropped.
        assert_eq!(meta.mime(), Some("text/html"));
    }

    #[test]
    fn merging_widens_the_range_and_keeps_the_newest_values() {
        let mut old = capture("20050101000000", "text/plain", "404", "OLD");
        let new = capture("20240101000000", "text/html", "200", "NEW");
        old.merge(&new);

        assert_eq!(old.first_seen(), Some("20050101000000"));
        assert_eq!(old.last_seen(), Some("20240101000000"));
        assert_eq!(old.mime(), Some("text/html"));
        assert_eq!(old.archive_status(), Some("200"));
        assert_eq!(old.digests().collect::<Vec<_>>(), vec!["NEW", "OLD"]);
    }

    #[test]
    fn merge_order_does_not_change_the_result() {
        let a = capture("20050101000000", "text/plain", "404", "OLD");
        let b = capture("20240101000000", "text/html", "200", "NEW");

        let mut forward = a.clone();
        forward.merge(&b);
        let mut backward = b.clone();
        backward.merge(&a);

        assert_eq!(forward, backward);
    }

    #[test]
    fn an_older_capture_never_overwrites_a_newer_value() {
        let mut newest = capture("20240101000000", "text/html", "200", "NEW");
        newest.merge(&capture("20050101000000", "text/plain", "404", "OLD"));
        assert_eq!(newest.mime(), Some("text/html"));
        assert_eq!(newest.archive_status(), Some("200"));
    }

    #[test]
    fn a_newer_capture_missing_a_field_keeps_the_older_known_value() {
        // The newest Wayback row for a URL is often a `warc/revisit` with no
        // status; that must not erase the MIME type an earlier capture knew.
        let mut meta = capture("20050101000000", "text/html", "200", "OLD");
        meta.merge(&CaptureMeta::capture(
            Some("20240101000000"),
            None,
            None,
            Some("NEW"),
        ));
        assert_eq!(meta.last_seen(), Some("20240101000000"));
        assert_eq!(meta.mime(), Some("text/html"));
        assert_eq!(meta.archive_status(), Some("200"));
    }

    #[test]
    fn an_undated_value_only_fills_an_empty_slot() {
        let mut undated = CaptureMeta::capture(None, Some("application/octet-stream"), None, None);
        assert_eq!(undated.mime(), Some("application/octet-stream"));

        undated.merge(&capture("20240101000000", "text/html", "200", "NEW"));
        assert_eq!(undated.mime(), Some("text/html"));

        // ...and the reverse direction agrees.
        let mut dated = capture("20240101000000", "text/html", "200", "NEW");
        dated.merge(&CaptureMeta::capture(
            None,
            Some("application/octet-stream"),
            None,
            None,
        ));
        assert_eq!(dated.mime(), Some("text/html"));
    }

    #[test]
    fn merging_an_empty_meta_changes_nothing() {
        let mut meta = capture("20240101000000", "text/html", "200", "NEW");
        let before = meta.clone();
        meta.merge(&CaptureMeta::default());
        assert_eq!(meta, before);
    }

    #[test]
    fn ties_on_the_timestamp_resolve_the_same_way_in_both_directions() {
        let a = capture("20240101000000", "text/html", "200", "A");
        let b = capture("20240101000000", "text/plain", "404", "B");

        let mut forward = a.clone();
        forward.merge(&b);
        let mut backward = b.clone();
        backward.merge(&a);

        assert_eq!(forward, backward);
    }

    #[test]
    fn a_record_set_folds_every_capture_of_one_url_together() {
        let records = vec![
            UrlRecord::new(
                "https://example.com/b".to_string(),
                capture("20240101000000", "text/html", "200", "NEW"),
            ),
            UrlRecord::new(
                "https://example.com/a".to_string(),
                capture("20050101000000", "text/plain", "404", "OLD"),
            ),
            UrlRecord::new(
                "https://example.com/a".to_string(),
                capture("20240101000000", "text/html", "200", "NEW"),
            ),
        ];

        let mut set = RecordSet::new();
        set.extend(records);
        assert_eq!(set.len(), 2);
        let collapsed = set.into_sorted();
        assert_eq!(collapsed.len(), 2);
        // Sorted by URL.
        assert_eq!(collapsed[0].url, "https://example.com/a");
        assert_eq!(collapsed[1].url, "https://example.com/b");

        let a = &collapsed[0].meta;
        assert_eq!(a.first_seen(), Some("20050101000000"));
        assert_eq!(a.last_seen(), Some("20240101000000"));
        assert_eq!(a.mime(), Some("text/html"));
        assert_eq!(a.digests().collect::<Vec<_>>(), vec!["NEW", "OLD"]);
    }

    #[test]
    fn newest_capture_pairs_the_latest_timestamp_with_its_own_digest() {
        // `digest()` picks the smallest digest for stable output; that is not
        // necessarily the body the newest capture served. Replaying the newest
        // capture and deduplicating on the smallest digest would skip URLs
        // whose bytes were never fetched.
        let mut meta = capture("20050101000000", "text/html", "200", "AAA-OLD");
        meta.merge(&capture("20240101000000", "text/html", "200", "ZZZ-NEW"));

        assert_eq!(meta.digest(), Some("AAA-OLD"));
        assert_eq!(meta.newest_capture(), Some(("20240101000000", "ZZZ-NEW")));

        // A newer capture that recorded no digest (a `-` revisit row) must not
        // detach the timestamp from the digest it belongs to.
        meta.merge(&CaptureMeta::capture(
            Some("20250101000000"),
            None,
            None,
            None,
        ));
        assert_eq!(meta.last_seen(), Some("20250101000000"));
        assert_eq!(meta.newest_capture(), Some(("20240101000000", "ZZZ-NEW")));

        // Nothing to replay without a digest-bearing capture.
        assert_eq!(CaptureMeta::default().newest_capture(), None);
        assert_eq!(
            CaptureMeta::capture(None, None, None, Some("X")).newest_capture(),
            None
        );
    }

    #[test]
    fn a_record_set_dedupes_bare_records_too() {
        let mut set = RecordSet::new();
        set.extend(vec![
            UrlRecord::bare("https://example.com/a".to_string()),
            UrlRecord::bare("https://example.com/a".to_string()),
        ]);
        let collapsed = set.into_sorted();
        assert_eq!(collapsed.len(), 1);
        assert!(collapsed[0].meta.is_empty());
    }

    #[test]
    fn an_empty_record_set_reports_itself_empty() {
        let mut set = RecordSet::new();
        assert!(set.is_empty());
        set.insert(UrlRecord::bare("https://example.com/a".to_string()));
        assert!(!set.is_empty());
        assert_eq!(set.len(), 1);
    }
}
