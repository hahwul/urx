//! Whole-store inspection and maintenance, behind `urx cache …`.
//!
//! [`CacheAdmin`] is deliberately a second trait rather than five more methods
//! on [`CacheBackend`](super::CacheBackend). The scan path only ever addresses
//! one entry at a time by key; these operations sweep the entire store. Keeping
//! them apart means a backend that exists only to serve a scan — including the
//! in-memory ones the tests build — doesn't have to grow methods it never
//! calls.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;

/// One stored entry, reduced to the three facts every report is derived from.
#[derive(Debug, Clone)]
pub struct EntryMeta {
    pub domain: String,
    /// How many URLs this entry holds.
    pub url_count: usize,
    /// When the scan that produced the entry finished.
    pub timestamp: DateTime<Utc>,
}

/// The whole-store operations `urx cache` needs from a backend.
///
/// Only the deletions are backend-specific; every number reported by `stats`
/// and `list` is computed from [`CacheAdmin::entries`] by the free functions
/// below, so the two backends cannot drift apart in what they report.
#[async_trait]
pub trait CacheAdmin: Send + Sync {
    /// `sqlite` or `redis`, as `--cache-type` spells it.
    fn backend_name(&self) -> &'static str;

    /// Where the data lives, safe to print (any Redis password is redacted).
    fn location(&self) -> String;

    /// Every entry in the store.
    async fn entries(&self) -> Result<Vec<EntryMeta>>;

    /// Bytes the cache occupies, or `None` when the backend can't say.
    async fn size_bytes(&self) -> Result<Option<u64>>;

    /// Delete entries older than `ttl_seconds`. Returns how many went.
    async fn delete_expired(&self, ttl_seconds: u64) -> Result<usize>;

    /// Delete every entry whose domain matches one of `patterns`
    /// (see [`domain_matches`]). Returns how many went.
    async fn delete_domains(&self, patterns: &[String]) -> Result<usize>;

    /// Delete everything. Returns how many entries went.
    async fn clear(&self) -> Result<usize>;
}

/// Stands in for a SQLite cache file that does not exist yet.
///
/// `SqliteCache::new` creates the parent directory and runs the schema
/// migration on open, so opening the store just to look at it would leave
/// behind the very database `urx cache stats` had just reported as empty.
/// Reporting through a backend that holds nothing keeps all five subcommands
/// on one code path instead of special-casing each.
pub struct MissingCache {
    pub location: String,
}

#[async_trait]
impl CacheAdmin for MissingCache {
    fn backend_name(&self) -> &'static str {
        "sqlite"
    }

    fn location(&self) -> String {
        format!("{} (does not exist yet)", self.location)
    }

    async fn entries(&self) -> Result<Vec<EntryMeta>> {
        Ok(Vec::new())
    }

    async fn size_bytes(&self) -> Result<Option<u64>> {
        Ok(Some(0))
    }

    async fn delete_expired(&self, _ttl_seconds: u64) -> Result<usize> {
        Ok(0)
    }

    async fn delete_domains(&self, _patterns: &[String]) -> Result<usize> {
        Ok(0)
    }

    async fn clear(&self) -> Result<usize> {
        Ok(0)
    }
}

/// One domain's footprint in the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainSummary {
    pub domain: String,
    /// Entries stored for this domain — one per distinct scan configuration
    /// (provider set + filters), so a domain scanned two ways has two.
    pub entries: usize,
    /// How many of those are past the TTL.
    pub expired_entries: usize,
    /// URLs stored, summed across this domain's entries. Two entries answer
    /// two different questions, so a URL present in both counts twice: this is
    /// "URLs stored", not "distinct URLs known".
    pub urls: usize,
    /// Timestamp of the newest entry.
    pub last_scan: DateTime<Utc>,
    /// Seconds until the newest entry expires; `None` once it already has.
    pub ttl_remaining: Option<i64>,
}

/// Store-wide summary, the payload of `urx cache stats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheStats {
    pub backend: String,
    pub location: String,
    /// The TTL the expiry columns were computed against (`--cache-ttl`).
    pub ttl_seconds: u64,
    pub entries: usize,
    pub domains: usize,
    pub urls: usize,
    pub expired_entries: usize,
    pub oldest: Option<DateTime<Utc>>,
    pub newest: Option<DateTime<Utc>>,
    /// SQLite: the database file. Redis: the bytes of the stored values.
    pub size_bytes: Option<u64>,
}

/// Whether an entry written at `timestamp` is past `ttl_seconds`.
///
/// Shares [`super::types::CacheEntry::is_expired`]'s clock-skew rule: an entry
/// dated in the future is not expired, it is early.
pub fn is_expired(timestamp: DateTime<Utc>, ttl_seconds: u64) -> bool {
    ttl_remaining(timestamp, ttl_seconds).is_none()
}

/// Seconds left before an entry written at `timestamp` expires, or `None` when
/// it already has.
///
/// `--cache-ttl` is an unvalidated `u64`, so the subtraction saturates rather
/// than wrapping — the same hazard [`super::types::expiry_cutoff`] documents.
pub fn ttl_remaining(timestamp: DateTime<Utc>, ttl_seconds: u64) -> Option<i64> {
    let ttl = ttl_seconds.min(i64::MAX as u64) as i64;
    let elapsed = Utc::now().signed_duration_since(timestamp).num_seconds();
    let left = ttl.saturating_sub(elapsed);
    (left > 0).then_some(left)
}

/// Case-insensitive domain match. `*` is a wildcard standing for any run of
/// characters; a pattern without one has to equal the domain exactly.
///
/// Exact-by-default is deliberate, because the same matcher backs the
/// destructive `urx cache drop`: a substring rule would let
/// `drop example.com` also take out `notexample.com`. Ask for the loose match
/// explicitly with `*example.com*`, or for subdomains with `*.example.com`.
pub fn domain_matches(pattern: &str, domain: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let domain = domain.to_lowercase();

    if !pattern.contains('*') {
        return pattern == domain;
    }

    // Anchored at both ends: the first segment must be a prefix, the last a
    // suffix, and the middle segments must appear in order in between.
    let parts: Vec<&str> = pattern.split('*').collect();
    let last = parts.len() - 1;
    let mut rest = domain.as_str();

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(tail) => rest = tail,
                None => return false,
            }
        } else if i == last {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(idx) => rest = &rest[idx + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Collapse raw entries into one row per domain, newest scan first.
///
/// `pattern` filters by domain when given. Ordering is total (last scan
/// descending, then domain ascending) so two runs over an unchanged cache
/// print byte-identical output.
pub fn summarize_domains(
    entries: &[EntryMeta],
    ttl_seconds: u64,
    pattern: Option<&str>,
) -> Vec<DomainSummary> {
    use std::collections::BTreeMap;

    let mut by_domain: BTreeMap<&str, DomainSummary> = BTreeMap::new();

    for entry in entries {
        if let Some(pat) = pattern {
            if !domain_matches(pat, &entry.domain) {
                continue;
            }
        }
        let expired = is_expired(entry.timestamp, ttl_seconds) as usize;
        by_domain
            .entry(entry.domain.as_str())
            .and_modify(|row| {
                row.entries += 1;
                row.expired_entries += expired;
                row.urls += entry.url_count;
                if entry.timestamp > row.last_scan {
                    row.last_scan = entry.timestamp;
                    row.ttl_remaining = ttl_remaining(entry.timestamp, ttl_seconds);
                }
            })
            .or_insert_with(|| DomainSummary {
                domain: entry.domain.clone(),
                entries: 1,
                expired_entries: expired,
                urls: entry.url_count,
                last_scan: entry.timestamp,
                ttl_remaining: ttl_remaining(entry.timestamp, ttl_seconds),
            });
    }

    let mut rows: Vec<DomainSummary> = by_domain.into_values().collect();
    rows.sort_by(|a, b| {
        b.last_scan
            .cmp(&a.last_scan)
            .then_with(|| a.domain.cmp(&b.domain))
    });
    rows
}

/// Roll raw entries up into the `urx cache stats` payload.
pub fn summarize_stats(
    entries: &[EntryMeta],
    ttl_seconds: u64,
    backend: &str,
    location: String,
    size_bytes: Option<u64>,
) -> CacheStats {
    use std::collections::HashSet;

    let domains: HashSet<&str> = entries.iter().map(|e| e.domain.as_str()).collect();

    CacheStats {
        backend: backend.to_string(),
        location,
        ttl_seconds,
        entries: entries.len(),
        domains: domains.len(),
        urls: entries.iter().map(|e| e.url_count).sum(),
        expired_entries: entries
            .iter()
            .filter(|e| is_expired(e.timestamp, ttl_seconds))
            .count(),
        oldest: entries.iter().map(|e| e.timestamp).min(),
        newest: entries.iter().map(|e| e.timestamp).max(),
        size_bytes,
    }
}

/// Strip the password out of a Redis URL before it is printed.
///
/// `--redis-url` routinely carries credentials (`redis://user:pw@host`) and
/// `urx cache stats` echoes the location back — including into `-f json`, which
/// people redirect into files and paste into issues.
///
/// Only the Redis backend calls this, but it stays compiled (and tested)
/// without the `redis-cache` feature: a credential-leak guard that only exists
/// in one build configuration is one nobody notices breaking.
#[cfg_attr(not(feature = "redis-cache"), allow(dead_code))]
pub fn redact_redis_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    // Only the authority section can hold credentials; a `@` after the first
    // `/` belongs to the path.
    let (authority, tail) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    let Some((userinfo, host)) = authority.rsplit_once('@') else {
        return url.to_string();
    };
    let user = userinfo.split_once(':').map_or(userinfo, |(u, _)| u);
    if user.is_empty() {
        format!("{scheme}://***@{host}{tail}")
    } else {
        format!("{scheme}://{user}:***@{host}{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(domain: &str, url_count: usize, age_secs: i64) -> EntryMeta {
        EntryMeta {
            domain: domain.to_string(),
            url_count,
            timestamp: Utc::now() - chrono::Duration::seconds(age_secs),
        }
    }

    #[tokio::test]
    async fn a_cache_that_does_not_exist_reports_empty_rather_than_being_created() {
        let missing = MissingCache {
            location: "/nonexistent/cache.db".to_string(),
        };
        assert!(missing.entries().await.unwrap().is_empty());
        assert_eq!(missing.size_bytes().await.unwrap(), Some(0));
        assert_eq!(missing.delete_expired(3600).await.unwrap(), 0);
        assert_eq!(
            missing
                .delete_domains(&["x.test".to_string()])
                .await
                .unwrap(),
            0
        );
        assert_eq!(missing.clear().await.unwrap(), 0);
        assert!(missing.location().contains("does not exist yet"));
        // Still a sqlite cache, so the report reads the same as a real one.
        assert_eq!(missing.backend_name(), "sqlite");
    }

    #[test]
    fn exact_match_is_the_default_so_drop_cannot_over_reach() {
        assert!(domain_matches("example.com", "example.com"));
        assert!(domain_matches("EXAMPLE.com", "example.COM"));
        // The cases a substring rule would have wrongly deleted.
        assert!(!domain_matches("example.com", "notexample.com"));
        assert!(!domain_matches("example.com", "example.com.evil.test"));
        assert!(!domain_matches("example.com", "api.example.com"));
    }

    #[test]
    fn wildcards_anchor_at_both_ends() {
        assert!(domain_matches("*.example.com", "api.example.com"));
        assert!(domain_matches("*.example.com", "a.b.example.com"));
        assert!(!domain_matches("*.example.com", "example.com"));
        assert!(!domain_matches("*.example.com", "api.example.com.evil"));

        assert!(domain_matches("example.*", "example.com"));
        assert!(domain_matches("example.*", "example.org"));
        assert!(!domain_matches("example.*", "www.example.com"));

        assert!(domain_matches("*example*", "notexample.com"));
        assert!(domain_matches("*", "anything.test"));
        assert!(domain_matches("a*b*c", "azzbzzc"));
        assert!(!domain_matches("a*b*c", "azzczzb"));
        // A wildcard must not let the tail overlap what the head consumed.
        assert!(domain_matches("ab*ab", "abab"));
        assert!(!domain_matches("ab*ab", "aba"));
    }

    #[test]
    fn ttl_remaining_counts_down_and_reports_expiry_as_none() {
        let fresh = Utc::now() - chrono::Duration::seconds(10);
        let left = ttl_remaining(fresh, 3600).expect("fresh entry has time left");
        assert!((3585..=3591).contains(&left), "{left}");
        assert!(!is_expired(fresh, 3600));

        let stale = Utc::now() - chrono::Duration::seconds(7200);
        assert_eq!(ttl_remaining(stale, 3600), None);
        assert!(is_expired(stale, 3600));
    }

    #[test]
    fn a_future_timestamp_is_early_not_expired() {
        // Matches CacheEntry::is_expired: an entry written by a machine whose
        // clock runs ahead must not be reported as ancient (and pruned).
        let future = Utc::now() + chrono::Duration::minutes(5);
        assert!(!is_expired(future, 3600));
        assert!(ttl_remaining(future, 3600).unwrap() > 3600);
    }

    #[test]
    fn a_huge_ttl_saturates_instead_of_wrapping_into_expiry() {
        // `--cache-ttl` is an unvalidated u64; `u64 as i64` wraps negative and
        // would report every entry as long expired.
        let entry = Utc::now() - chrono::Duration::seconds(60);
        for ttl in [u64::MAX, u64::MAX / 2, i64::MAX as u64] {
            assert!(!is_expired(entry, ttl), "ttl={ttl}");
            assert!(ttl_remaining(entry, ttl).is_some(), "ttl={ttl}");
        }
        // A future-dated entry under a huge TTL must not overflow either.
        let future = Utc::now() + chrono::Duration::minutes(5);
        assert!(ttl_remaining(future, u64::MAX).is_some());
    }

    #[test]
    fn domains_are_rolled_up_newest_first() {
        let entries = vec![
            entry("old.test", 10, 7200),
            entry("fresh.test", 5, 60),
            // Two configurations of the same domain collapse into one row.
            entry("fresh.test", 7, 30),
        ];

        let rows = summarize_domains(&entries, 3600, None);
        assert_eq!(rows.len(), 2);

        assert_eq!(rows[0].domain, "fresh.test");
        assert_eq!(rows[0].entries, 2);
        assert_eq!(rows[0].urls, 12);
        assert_eq!(rows[0].expired_entries, 0);
        // The newest of the two entries drives last_scan and the countdown.
        assert!(rows[0].ttl_remaining.unwrap() > 3500);

        assert_eq!(rows[1].domain, "old.test");
        assert_eq!(rows[1].expired_entries, 1);
        assert_eq!(rows[1].ttl_remaining, None);
    }

    #[test]
    fn a_domain_is_fresh_while_any_entry_is() {
        // One stale entry alongside a fresh one must not make the row read as
        // expired — the fresh entry is still being served.
        let entries = vec![entry("mixed.test", 1, 7200), entry("mixed.test", 2, 10)];
        let rows = summarize_domains(&entries, 3600, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].expired_entries, 1);
        assert!(rows[0].ttl_remaining.is_some());
    }

    #[test]
    fn list_pattern_filters_by_domain() {
        let entries = vec![
            entry("api.example.com", 1, 10),
            entry("example.com", 2, 20),
            entry("example.org", 3, 30),
        ];

        let names = |pat: &str| -> Vec<String> {
            summarize_domains(&entries, 3600, Some(pat))
                .into_iter()
                .map(|r| r.domain)
                .collect()
        };

        assert_eq!(names("example.com"), vec!["example.com"]);
        assert_eq!(names("*.example.com"), vec!["api.example.com"]);
        assert_eq!(names("example.*"), vec!["example.com", "example.org"]);
        assert!(names("absent.test").is_empty());
    }

    #[test]
    fn stats_count_entries_domains_urls_and_expiry() {
        let entries = vec![
            entry("a.test", 10, 7200),
            entry("a.test", 20, 60),
            entry("b.test", 5, 30),
        ];

        let stats = summarize_stats(&entries, 3600, "sqlite", "/tmp/cache.db".into(), Some(4096));

        assert_eq!(stats.entries, 3);
        assert_eq!(stats.domains, 2);
        assert_eq!(stats.urls, 35);
        assert_eq!(stats.expired_entries, 1);
        assert_eq!(stats.size_bytes, Some(4096));
        assert_eq!(stats.ttl_seconds, 3600);
        assert!(stats.oldest.unwrap() < stats.newest.unwrap());
    }

    #[test]
    fn stats_on_an_empty_cache_are_zeroed_not_absent() {
        let stats = summarize_stats(&[], 3600, "sqlite", "/tmp/cache.db".into(), Some(0));
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.domains, 0);
        assert_eq!(stats.urls, 0);
        assert_eq!(stats.oldest, None);
        assert_eq!(stats.newest, None);
    }

    #[test]
    fn redis_credentials_never_reach_the_output() {
        assert_eq!(
            redact_redis_url("redis://user:hunter2@cache.internal:6379/0"),
            "redis://user:***@cache.internal:6379/0"
        );
        // The password-only form Redis also accepts.
        assert_eq!(
            redact_redis_url("redis://:hunter2@cache.internal:6379"),
            "redis://***@cache.internal:6379"
        );
        // Nothing to hide, nothing changed.
        assert_eq!(
            redact_redis_url("redis://127.0.0.1:6379"),
            "redis://127.0.0.1:6379"
        );
        assert_eq!(redact_redis_url("not a url"), "not a url");
        // A `@` in the path is not credentials.
        assert_eq!(
            redact_redis_url("redis://127.0.0.1:6379/a@b"),
            "redis://127.0.0.1:6379/a@b"
        );
    }
}
