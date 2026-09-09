use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use tokio::task;

use super::admin::{domain_matches, is_expired, CacheAdmin, EntryMeta};
use super::types::{CacheBackend, CacheEntry, CacheKey};

/// SQLite-based cache implementation
pub struct SqliteCache {
    db_path: std::path::PathBuf,
}

impl SqliteCache {
    /// Create a new SQLite cache
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();

        // Create parent directory if it doesn't exist
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create cache directory")?;
        }

        let cache = Self { db_path };
        cache.initialize_db().await?;
        Ok(cache)
    }

    /// Initialize the database schema
    async fn initialize_db(&self) -> Result<()> {
        let db_path = self.db_path.clone();

        task::spawn_blocking(move || {
            let conn = Connection::open(&db_path).context("Failed to open SQLite database")?;

            conn.execute(
                r#"
                CREATE TABLE IF NOT EXISTS url_cache (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    cache_key TEXT UNIQUE NOT NULL,
                    domain TEXT NOT NULL,
                    providers TEXT NOT NULL,
                    filters_hash TEXT NOT NULL,
                    urls TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
                )
                "#,
                [],
            )
            .context("Failed to create cache table")?;

            // Create index for better performance
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_cache_key ON url_cache(cache_key)",
                [],
            )
            .context("Failed to create cache key index")?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_domain ON url_cache(domain)",
                [],
            )
            .context("Failed to create domain index")?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_timestamp ON url_cache(timestamp)",
                [],
            )
            .context("Failed to create timestamp index")?;

            Ok::<(), anyhow::Error>(())
        })
        .await??;

        Ok(())
    }

    /// Execute a database operation in a blocking task
    async fn with_connection<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let db_path = self.db_path.clone();
        task::spawn_blocking(move || {
            let conn = Connection::open(&db_path).context("Failed to open SQLite database")?;
            f(&conn)
        })
        .await?
    }

    /// Delete rows by primary key inside one transaction, reclaiming space when
    /// the delete was large enough to be worth a `VACUUM`.
    async fn delete_ids(&self, ids: Vec<i64>) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }

        self.with_connection(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut deleted = 0usize;
            {
                let mut stmt = tx.prepare("DELETE FROM url_cache WHERE id = ?1")?;
                for id in &ids {
                    deleted += stmt.execute(params![id])?;
                }
            }
            tx.commit()?;

            // Matches `cleanup_expired`: reclaiming pages costs a full rewrite,
            // so it is only worth it once a meaningful number of rows went.
            if deleted > 10 {
                conn.execute("VACUUM", [])?;
            }
            Ok(deleted)
        })
        .await
    }
}

#[async_trait]
impl CacheBackend for SqliteCache {
    async fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
        let cache_key = format!("{}", key);

        self.with_connection(move |conn| {
            let mut stmt =
                conn.prepare("SELECT urls, timestamp FROM url_cache WHERE cache_key = ?1")?;

            let result = stmt
                .query_row(params![cache_key], |row| {
                    let urls_json: String = row.get(0)?;
                    let timestamp_str: String = row.get(1)?;

                    let urls: Vec<String> = serde_json::from_str(&urls_json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                    let timestamp: DateTime<Utc> = timestamp_str.parse().map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                    Ok(CacheEntry { urls, timestamp })
                })
                .optional()?;

            Ok(result)
        })
        .await
    }

    async fn set(&self, key: &CacheKey, entry: &CacheEntry) -> Result<()> {
        let cache_key = format!("{}", key);
        let domain = key.domain.clone();
        let providers = serde_json::to_string(&key.providers)?;
        let filters_hash = key.filters_hash.clone();
        let urls = serde_json::to_string(&entry.urls)?;
        let timestamp = entry.timestamp.to_rfc3339();

        self.with_connection(move |conn| {
            conn.execute(
                r#"
                INSERT OR REPLACE INTO url_cache
                (cache_key, domain, providers, filters_hash, urls, timestamp)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![cache_key, domain, providers, filters_hash, urls, timestamp],
            )?;
            Ok(())
        })
        .await
    }

    async fn delete(&self, key: &CacheKey) -> Result<()> {
        let cache_key = format!("{}", key);

        self.with_connection(move |conn| {
            conn.execute(
                "DELETE FROM url_cache WHERE cache_key = ?1",
                params![cache_key],
            )?;
            Ok(())
        })
        .await
    }

    async fn cleanup_expired(&self, ttl_seconds: u64) -> Result<()> {
        // See `expiry_cutoff`: `--cache-ttl` is an unvalidated u64 and the raw
        // chrono conversion either panics or wraps into the future.
        let cutoff_str = super::types::expiry_cutoff(ttl_seconds).to_rfc3339();

        self.with_connection(move |conn| {
            let deleted = conn.execute(
                "DELETE FROM url_cache WHERE timestamp < ?1",
                params![cutoff_str],
            )?;

            // Also vacuum the database if we deleted a significant number of entries
            if deleted > 10 {
                conn.execute("VACUUM", [])?;
            }

            Ok(())
        })
        .await
    }

    async fn exists(&self, key: &CacheKey) -> Result<bool> {
        let cache_key = format!("{}", key);

        self.with_connection(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM url_cache WHERE cache_key = ?1",
                params![cache_key],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
        .await
    }
}

#[async_trait]
impl CacheAdmin for SqliteCache {
    fn backend_name(&self) -> &'static str {
        "sqlite"
    }

    fn location(&self) -> String {
        self.db_path.display().to_string()
    }

    async fn entries(&self) -> Result<Vec<EntryMeta>> {
        self.with_connection(move |conn| {
            let mut stmt = conn.prepare("SELECT domain, urls, timestamp FROM url_cache")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;

            let mut entries = Vec::new();
            for row in rows {
                let (domain, urls_json, timestamp) = row?;
                // A row whose payload no longer parses is reported rather than
                // aborting the whole report: `urx cache` is the tool you reach
                // for *because* the cache looks wrong, so it has to survive a
                // corrupt row. It still counts as an entry, with zero URLs.
                let url_count = serde_json::from_str::<Vec<String>>(&urls_json)
                    .map(|urls| urls.len())
                    .unwrap_or(0);
                let Ok(timestamp) = timestamp.parse::<DateTime<Utc>>() else {
                    continue;
                };
                entries.push(EntryMeta {
                    domain,
                    url_count,
                    timestamp,
                });
            }
            Ok(entries)
        })
        .await
    }

    async fn size_bytes(&self) -> Result<Option<u64>> {
        let db_path = self.db_path.clone();
        // The write-ahead log and shared-memory files are part of what the
        // cache costs on disk, so a size that ignored them would read low right
        // after a big scan.
        Ok(task::spawn_blocking(move || {
            let mut total = 0u64;
            let mut found = false;
            for suffix in ["", "-wal", "-shm"] {
                let mut path = db_path.clone().into_os_string();
                path.push(suffix);
                if let Ok(meta) = std::fs::metadata(std::path::PathBuf::from(path)) {
                    total += meta.len();
                    found = true;
                }
            }
            found.then_some(total)
        })
        .await?)
    }

    async fn delete_expired(&self, ttl_seconds: u64) -> Result<usize> {
        // Selected and tested in Rust rather than compared as SQL strings, so
        // the sweep uses exactly the expiry rule `CacheEntry::is_expired` uses
        // — including its clock-skew guard — and a row stored in some other
        // timestamp format can't be deleted by a lexicographic accident.
        let doomed: Vec<i64> = self
            .with_connection(move |conn| {
                let mut stmt = conn.prepare("SELECT id, timestamp FROM url_cache")?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows
                    .into_iter()
                    .filter_map(|(id, ts)| {
                        let ts = ts.parse::<DateTime<Utc>>().ok()?;
                        is_expired(ts, ttl_seconds).then_some(id)
                    })
                    .collect())
            })
            .await?;

        self.delete_ids(doomed).await
    }

    async fn delete_domains(&self, patterns: &[String]) -> Result<usize> {
        let patterns = patterns.to_vec();
        let doomed: Vec<i64> = self
            .with_connection(move |conn| {
                let mut stmt = conn.prepare("SELECT id, domain FROM url_cache")?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows
                    .into_iter()
                    .filter(|(_, domain)| patterns.iter().any(|p| domain_matches(p, domain)))
                    .map(|(id, _)| id)
                    .collect())
            })
            .await?;

        self.delete_ids(doomed).await
    }

    async fn clear(&self) -> Result<usize> {
        self.with_connection(move |conn| {
            let deleted = conn.execute("DELETE FROM url_cache", [])?;
            // Always, unlike the incremental deletes: an emptied table should
            // hand the disk space back rather than leave a multi-megabyte file
            // that makes `urx cache stats` look like the clear did nothing.
            conn.execute("VACUUM", [])?;
            Ok(deleted)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::types::CacheFilters;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_sqlite_cache_basic_operations() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");

        let cache = SqliteCache::new(&db_path).await?;

        let filters = CacheFilters {
            strict: true,
            ..Default::default()
        };

        let key = CacheKey::new("example.com", &["wayback".to_string()], &filters);
        let entry = CacheEntry::new(vec!["https://example.com/page1".to_string()]);

        // Test exists (should be false initially)
        assert!(!cache.exists(&key).await?);

        // Test set
        cache.set(&key, &entry).await?;

        // Test exists (should be true now)
        assert!(cache.exists(&key).await?);

        // Test get
        let retrieved = cache.get(&key).await?;
        assert!(retrieved.is_some());
        let retrieved_entry = retrieved.unwrap();
        assert_eq!(retrieved_entry.urls, vec!["https://example.com/page1"]);

        // Test delete
        cache.delete(&key).await?;
        assert!(!cache.exists(&key).await?);

        Ok(())
    }

    #[tokio::test]
    async fn test_sqlite_cache_cleanup_expired() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");

        let cache = SqliteCache::new(&db_path).await?;

        let filters = CacheFilters {
            strict: true,
            ..Default::default()
        };

        let key = CacheKey::new("example.com", &["wayback".to_string()], &filters);

        // Create an old entry
        let mut old_entry = CacheEntry::new(vec!["https://example.com/old".to_string()]);
        old_entry.timestamp = Utc::now() - chrono::Duration::hours(2);

        cache.set(&key, &old_entry).await?;
        assert!(cache.exists(&key).await?);

        // Clean up expired entries (1 hour TTL)
        cache.cleanup_expired(3600).await?;

        // Entry should be gone
        assert!(!cache.exists(&key).await?);

        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_with_huge_ttl_keeps_entries_and_does_not_panic() -> Result<()> {
        // Regression: `--cache-ttl` is an unvalidated u64 fed straight to
        // chrono::Duration::seconds, which panics past its bounds — a large
        // value aborted the run at cleanup time.
        let temp_dir = tempdir()?;
        let cache = SqliteCache::new(temp_dir.path().join("test.db")).await?;

        let filters = CacheFilters::default();
        let key = CacheKey::new("example.com", &["wayback".to_string()], &filters);
        cache
            .set(&key, &CacheEntry::new(vec!["https://example.com/x".into()]))
            .await?;

        // A TTL this long means "never expire"; nothing may be deleted.
        cache.cleanup_expired(u64::MAX).await?;
        assert!(cache.exists(&key).await?);
        cache.cleanup_expired(10_000_000_000_000_000).await?;
        assert!(cache.exists(&key).await?);

        Ok(())
    }

    /// Store one entry under a distinct key, dated `age_secs` in the past.
    async fn seed(
        cache: &SqliteCache,
        domain: &str,
        tag: &str,
        urls: usize,
        age_secs: i64,
    ) -> Result<()> {
        let filters = CacheFilters {
            presets: vec![tag.to_string()],
            ..Default::default()
        };
        let key = CacheKey::new(domain, &["wayback".to_string()], &filters);
        let mut entry = CacheEntry::new(
            (0..urls)
                .map(|i| format!("https://{domain}/{tag}/{i}"))
                .collect(),
        );
        entry.timestamp = Utc::now() - chrono::Duration::seconds(age_secs);
        cache.set(&key, &entry).await
    }

    async fn admin_cache(dir: &tempfile::TempDir) -> Result<SqliteCache> {
        SqliteCache::new(dir.path().join("admin.db")).await
    }

    #[tokio::test]
    async fn admin_entries_report_domain_url_count_and_age() -> Result<()> {
        let dir = tempdir()?;
        let cache = admin_cache(&dir).await?;

        seed(&cache, "example.com", "a", 3, 60).await?;
        seed(&cache, "example.com", "b", 2, 7200).await?;
        seed(&cache, "other.test", "a", 1, 10).await?;

        let mut entries = cache.entries().await?;
        entries.sort_by_key(|e| e.timestamp);

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].domain, "example.com");
        assert_eq!(entries[0].url_count, 2);
        assert_eq!(
            entries.iter().map(|e| e.url_count).sum::<usize>(),
            6,
            "{entries:?}"
        );

        // The database file exists and is measured, WAL sidecars included.
        assert!(cache.size_bytes().await?.unwrap() > 0);
        Ok(())
    }

    #[tokio::test]
    async fn admin_reports_an_empty_cache_rather_than_failing() -> Result<()> {
        let dir = tempdir()?;
        let cache = admin_cache(&dir).await?;

        assert!(cache.entries().await?.is_empty());
        assert_eq!(cache.delete_expired(3600).await?, 0);
        assert_eq!(cache.delete_domains(&["example.com".to_string()]).await?, 0);
        assert_eq!(cache.clear().await?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn prune_removes_only_what_the_ttl_actually_covers() -> Result<()> {
        let dir = tempdir()?;
        let cache = admin_cache(&dir).await?;

        seed(&cache, "fresh.test", "a", 1, 60).await?;
        seed(&cache, "stale.test", "a", 1, 7200).await?;
        seed(&cache, "stale.test", "b", 1, 90_000).await?;

        assert_eq!(cache.delete_expired(3600).await?, 2);
        let left = cache.entries().await?;
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].domain, "fresh.test");

        // Idempotent: a second sweep has nothing left to take.
        assert_eq!(cache.delete_expired(3600).await?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn prune_with_a_huge_ttl_deletes_nothing() -> Result<()> {
        // The `--cache-ttl` overflow that would otherwise put the cutoff in the
        // future and wipe the cache — the same hazard `expiry_cutoff` guards.
        let dir = tempdir()?;
        let cache = admin_cache(&dir).await?;
        seed(&cache, "example.com", "a", 1, 90_000).await?;

        assert_eq!(cache.delete_expired(u64::MAX).await?, 0);
        assert_eq!(cache.entries().await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn drop_matches_domains_exactly_unless_a_wildcard_says_otherwise() -> Result<()> {
        let dir = tempdir()?;
        let cache = admin_cache(&dir).await?;

        seed(&cache, "example.com", "a", 1, 10).await?;
        seed(&cache, "example.com", "b", 1, 20).await?;
        seed(&cache, "api.example.com", "a", 1, 30).await?;
        seed(&cache, "notexample.com", "a", 1, 40).await?;

        // Exact: both entries for the domain go, and nothing that merely
        // contains the name does.
        assert_eq!(cache.delete_domains(&["example.com".to_string()]).await?, 2);
        let left: Vec<String> = cache
            .entries()
            .await?
            .into_iter()
            .map(|e| e.domain)
            .collect();
        assert!(left.contains(&"api.example.com".to_string()), "{left:?}");
        assert!(left.contains(&"notexample.com".to_string()), "{left:?}");

        // Wildcards reach subdomains when asked to.
        assert_eq!(
            cache.delete_domains(&["*.example.com".to_string()]).await?,
            1
        );
        assert_eq!(cache.entries().await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn clear_empties_the_table_and_reclaims_the_file() -> Result<()> {
        let dir = tempdir()?;
        let cache = admin_cache(&dir).await?;

        for i in 0..15 {
            seed(&cache, &format!("d{i}.test"), "a", 40, 10).await?;
        }
        let before = cache.size_bytes().await?.unwrap();

        assert_eq!(cache.clear().await?, 15);
        assert!(cache.entries().await?.is_empty());
        // VACUUM runs unconditionally on clear, so the file must not stay at
        // its full size and make `cache stats` look like nothing happened.
        assert!(
            cache.size_bytes().await?.unwrap() < before,
            "file did not shrink after clear"
        );

        // The cache is still usable afterwards.
        seed(&cache, "again.test", "a", 1, 0).await?;
        assert_eq!(cache.entries().await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn a_corrupt_row_is_reported_not_fatal() -> Result<()> {
        // `urx cache` is the tool you reach for *because* the cache looks
        // wrong, so a row whose payload no longer parses must not abort the
        // report. It still counts as an entry, with zero URLs.
        let dir = tempdir()?;
        let cache = admin_cache(&dir).await?;
        seed(&cache, "good.test", "a", 2, 10).await?;

        let db_path = dir.path().join("admin.db");
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = Connection::open(&db_path)?;
            conn.execute(
                "INSERT INTO url_cache (cache_key, domain, providers, filters_hash, urls, timestamp)
                 VALUES ('broken', 'bad.test', '[]', 'h', 'not json', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await??;

        let entries = cache.entries().await?;
        assert_eq!(entries.len(), 2, "{entries:?}");
        let bad = entries.iter().find(|e| e.domain == "bad.test").unwrap();
        assert_eq!(bad.url_count, 0);

        // ...and it can still be dropped.
        assert_eq!(cache.delete_domains(&["bad.test".to_string()]).await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_sqlite_cache_multiple_entries() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");

        let cache = SqliteCache::new(&db_path).await?;

        let filters = CacheFilters {
            strict: true,
            ..Default::default()
        };

        let key1 = CacheKey::new("example.com", &["wayback".to_string()], &filters);
        let key2 = CacheKey::new("test.com", &["wayback".to_string()], &filters);

        let entry1 = CacheEntry::new(vec!["https://example.com/page1".to_string()]);
        let entry2 = CacheEntry::new(vec!["https://test.com/page1".to_string()]);

        cache.set(&key1, &entry1).await?;
        cache.set(&key2, &entry2).await?;

        // Both should exist
        assert!(cache.exists(&key1).await?);
        assert!(cache.exists(&key2).await?);

        // Retrieve and verify
        let retrieved1 = cache.get(&key1).await?.unwrap();
        let retrieved2 = cache.get(&key2).await?.unwrap();

        assert_eq!(retrieved1.urls, vec!["https://example.com/page1"]);
        assert_eq!(retrieved2.urls, vec!["https://test.com/page1"]);

        Ok(())
    }
}
