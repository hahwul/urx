mod admin;
mod command;
mod sqlite;
mod types;

#[cfg(feature = "redis-cache")]
mod redis_impl;

#[cfg(feature = "redis-cache")]
pub use admin::redact_redis_url;
pub use admin::CacheAdmin;
use admin::MissingCache;
pub use command::Command;
pub use sqlite::SqliteCache;
pub use types::{CacheBackend, CacheEntry, CacheFilters, CacheKey};

#[cfg(feature = "redis-cache")]
pub use redis_impl::RedisCache;

use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

/// `--cache-type`: the backend that holds the cache. `redis` is always
/// accepted and reports at run time when it was not compiled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CacheType {
    Sqlite,
    Redis,
}

/// Where the SQLite cache lives when `--cache-path` / `[cache].cache_path`
/// say nothing.
pub fn default_sqlite_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".urx").join("cache.db")
}

/// Open the cache `--cache-type` names for whole-store inspection.
///
/// Separate from `create_cache_manager` because that returns a
/// [`CacheManager`], which deliberately exposes only per-key operations, and
/// because `--no-cache` is irrelevant here: it turns caching off for a *scan*,
/// while `urx cache` is asking about the store itself.
pub async fn open_admin(args: &crate::cli::Args) -> Result<Box<dyn CacheAdmin>> {
    match args.cache_type {
        CacheType::Sqlite => {
            let path = args.cache_path.clone().unwrap_or_else(default_sqlite_path);
            // Looking at the cache must not create one — see [`MissingCache`].
            if !path.exists() {
                return Ok(Box::new(MissingCache {
                    location: path.display().to_string(),
                }));
            }
            Ok(Box::new(SqliteCache::new(path).await?))
        }
        #[cfg(feature = "redis-cache")]
        CacheType::Redis => {
            let Some(redis_url) = &args.redis_url else {
                anyhow::bail!("Redis cache type selected but no --redis-url provided");
            };
            Ok(Box::new(RedisCache::new(redis_url).await?))
        }
        #[cfg(not(feature = "redis-cache"))]
        CacheType::Redis => anyhow::bail!(
            "Redis cache support is not compiled in. Rebuild with `--features redis-cache`, or use --cache-type sqlite."
        ),
    }
}

/// Run one `urx cache <SUBCOMMAND>`.
pub async fn run_command(args: &crate::cli::Args, command: &Command) -> Result<()> {
    let Command::Cache { action } = command;
    command::run(args, action).await
}

/// Cache manager that provides a unified interface for different cache backends
pub struct CacheManager {
    backend: Box<dyn CacheBackend>,
}

impl CacheManager {
    /// Create a new cache manager with SQLite backend
    pub async fn new_sqlite<P: AsRef<std::path::Path>>(db_path: P) -> Result<Self> {
        let backend = Box::new(SqliteCache::new(db_path).await?);
        Ok(Self { backend })
    }

    /// Create a new cache manager with Redis backend (if feature is enabled)
    #[cfg(feature = "redis-cache")]
    pub async fn new_redis(redis_url: &str) -> Result<Self> {
        let backend = Box::new(RedisCache::new(redis_url).await?);
        Ok(Self { backend })
    }

    /// The entry for `key` if it is younger than `ttl_seconds`. An expired
    /// entry is deleted on the way out and reads as a miss.
    pub async fn get_fresh(&self, key: &CacheKey, ttl_seconds: u64) -> Result<Option<CacheEntry>> {
        // A failed read (corrupt or foreign row) is a miss, not a dead run.
        match self.backend.get(key).await.ok().flatten() {
            Some(entry) if entry.is_expired(ttl_seconds) => {
                let _ = self.backend.delete(key).await;
                Ok(None)
            }
            fresh => Ok(fresh),
        }
    }

    /// Store URLs in cache.
    ///
    /// A key already holding an `--incremental` baseline keeps being one: the
    /// URLs are added to it rather than replacing it, so a normal run neither
    /// shrinks the baseline nor strips the flag that keeps it out of the sweep.
    pub async fn store_urls(&self, key: &CacheKey, entry: &CacheEntry) -> Result<()> {
        match self.backend.get(key).await.ok().flatten() {
            Some(existing) if existing.baseline => {
                let mut urls: HashSet<String> = existing.urls.into_iter().collect();
                urls.extend(entry.urls.iter().cloned());
                let mut merged = CacheEntry::new(urls.into_iter().collect());
                merged.baseline = true;
                self.backend.set(key, &merged).await
            }
            _ => self.backend.set(key, entry).await,
        }
    }

    /// Fold `fresh` into the `--incremental` baseline for `key`, returning the
    /// URLs it had not seen.
    ///
    /// The baseline is the union of every run, never just the latest: a run
    /// cut short by `--max-time` or a provider error would otherwise shrink it,
    /// and the next good run re-report everything it lost as new.
    pub async fn update_baseline(
        &self,
        key: &CacheKey,
        fresh: HashSet<String>,
    ) -> Result<HashSet<String>> {
        // Not a miss: an unreadable baseline would report every URL as new.
        let mut known: HashSet<String> = match self.backend.get(key).await? {
            Some(entry) => entry.urls.into_iter().collect(),
            None => HashSet::new(),
        };
        let new: HashSet<String> = fresh.difference(&known).cloned().collect();
        known.extend(new.iter().cloned());

        let mut entry = CacheEntry::new(known.into_iter().collect());
        entry.baseline = true;
        self.backend.set(key, &entry).await?;
        Ok(new)
    }

    /// The post-run sweep: delete entries older than `ttl_seconds`, except
    /// `--incremental` baselines, which are read with no TTL and which this run
    /// may not even own. A no-op for a backend without an admin side.
    pub async fn delete_expired(&self, ttl_seconds: u64) -> Result<usize> {
        match self.backend.admin() {
            Some(admin) => admin.delete_expired(ttl_seconds, true).await,
            None => Ok(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(backend: Box<dyn CacheBackend>) -> Self {
        Self { backend }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cache_manager_sqlite() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");

        let cache = CacheManager::new_sqlite(&db_path).await?;

        let key = CacheKey {
            domain: "example.com".to_string(),
            providers: vec!["wayback".to_string()],
            filters_hash: "test_hash".to_string(),
        };

        let entry = CacheEntry::new(vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ]);

        // Store and retrieve
        cache.store_urls(&key, &entry).await?;
        let retrieved = cache.get_fresh(&key, 3600).await?;

        assert!(retrieved.is_some());
        let retrieved_entry = retrieved.unwrap();
        assert_eq!(retrieved_entry.urls.len(), 2);
        assert!(retrieved_entry
            .urls
            .contains(&"https://example.com/page1".to_string()));
        assert!(retrieved_entry
            .urls
            .contains(&"https://example.com/page2".to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn test_incremental_scanning() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");

        let cache = CacheManager::new_sqlite(&db_path).await?;

        let key = CacheKey {
            domain: "example.com".to_string(),
            providers: vec!["wayback".to_string()],
            filters_hash: "test_hash".to_string(),
        };

        // Store initial URLs
        let initial_entry = CacheEntry::new(vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ]);
        cache.store_urls(&key, &initial_entry).await?;

        // New scan with some overlapping URLs
        let new_urls: HashSet<String> = vec![
            "https://example.com/page2".to_string(), // existing
            "https://example.com/page3".to_string(), // new
            "https://example.com/page4".to_string(), // new
        ]
        .into_iter()
        .collect();

        let incremental_urls = cache.update_baseline(&key, new_urls).await?;

        // Should only return new URLs
        assert_eq!(incremental_urls.len(), 2);
        assert!(incremental_urls.contains("https://example.com/page3"));
        assert!(incremental_urls.contains("https://example.com/page4"));
        assert!(!incremental_urls.contains("https://example.com/page2"));

        // A later run that saw less (cut short, or a provider down) must not
        // shrink the baseline: page1 is still known, nothing is new.
        let partial = HashSet::from(["https://example.com/page1".to_string()]);
        assert!(cache.update_baseline(&key, partial).await?.is_empty());
        let baseline = cache.get_fresh(&key, 3600).await?.unwrap();
        assert_eq!(baseline.urls.len(), 4);
        assert!(baseline.baseline);

        // ...and the post-run sweep, whatever its TTL, leaves it alone.
        assert_eq!(cache.delete_expired(0).await?, 0);
        assert!(cache.get_fresh(&key, 3600).await?.is_some());

        // A normal run on the same key adds to the baseline; it neither
        // shrinks it nor strips the flag the sweep relies on.
        let normal = CacheEntry::new(vec!["https://example.com/page5".to_string()]);
        cache.store_urls(&key, &normal).await?;
        let baseline = cache.get_fresh(&key, 3600).await?.unwrap();
        assert_eq!(baseline.urls.len(), 5);
        assert!(baseline.baseline);

        Ok(())
    }
}
