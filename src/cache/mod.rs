mod admin;
mod command;
mod sqlite;
mod types;

#[cfg(feature = "redis-cache")]
mod redis_impl;

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
    match args.cache_type.as_str() {
        "sqlite" => {
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
        "redis" => {
            let Some(redis_url) = &args.redis_url else {
                anyhow::bail!("Redis cache type selected but no --redis-url provided");
            };
            Ok(Box::new(RedisCache::new(redis_url).await?))
        }
        #[cfg(not(feature = "redis-cache"))]
        "redis" => anyhow::bail!(
            "Redis cache support is not compiled in. Rebuild with `--features redis-cache`, or use --cache-type sqlite."
        ),
        other => anyhow::bail!("Unknown cache type '{other}'. Use 'sqlite' or 'redis'"),
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

    /// Get cached URLs for a domain and configuration
    pub async fn get_cached_urls(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
        self.backend.get(key).await
    }

    /// Store URLs in cache
    pub async fn store_urls(&self, key: &CacheKey, entry: &CacheEntry) -> Result<()> {
        self.backend.set(key, entry).await
    }

    /// Check if cache entry is still valid based on TTL
    pub async fn is_valid(&self, key: &CacheKey, ttl_seconds: u64) -> Result<bool> {
        if let Some(entry) = self.backend.get(key).await? {
            if entry.is_expired(ttl_seconds) {
                // Remove expired entry proactively
                let _ = self.backend.delete(key).await;
                Ok(false)
            } else {
                Ok(true)
            }
        } else {
            Ok(false)
        }
    }

    /// Get only new URLs compared to cached results (for incremental scanning)
    pub async fn get_new_urls(
        &self,
        key: &CacheKey,
        new_urls: &HashSet<String>,
    ) -> Result<HashSet<String>> {
        if !self.backend.exists(key).await? {
            return Ok(new_urls.clone());
        }
        if let Some(cached_entry) = self.backend.get(key).await? {
            let cached_urls: HashSet<String> = cached_entry.urls.into_iter().collect();
            Ok(new_urls.difference(&cached_urls).cloned().collect())
        } else {
            // No cached data, all URLs are new
            Ok(new_urls.clone())
        }
    }

    /// Clear expired cache entries
    pub async fn cleanup_expired(&self, ttl_seconds: u64) -> Result<()> {
        self.backend.cleanup_expired(ttl_seconds).await
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

        let entry = CacheEntry {
            urls: vec![
                "https://example.com/page1".to_string(),
                "https://example.com/page2".to_string(),
            ],
            timestamp: chrono::Utc::now(),
        };

        // Store and retrieve
        cache.store_urls(&key, &entry).await?;
        let retrieved = cache.get_cached_urls(&key).await?;

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
        let initial_entry = CacheEntry {
            urls: vec![
                "https://example.com/page1".to_string(),
                "https://example.com/page2".to_string(),
            ],
            timestamp: chrono::Utc::now(),
        };
        cache.store_urls(&key, &initial_entry).await?;

        // New scan with some overlapping URLs
        let new_urls: HashSet<String> = vec![
            "https://example.com/page2".to_string(), // existing
            "https://example.com/page3".to_string(), // new
            "https://example.com/page4".to_string(), // new
        ]
        .into_iter()
        .collect();

        let incremental_urls = cache.get_new_urls(&key, &new_urls).await?;

        // Should only return new URLs
        assert_eq!(incremental_urls.len(), 2);
        assert!(incremental_urls.contains("https://example.com/page3"));
        assert!(incremental_urls.contains("https://example.com/page4"));
        assert!(!incremental_urls.contains("https://example.com/page2"));

        Ok(())
    }
}
