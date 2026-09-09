use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::admin::{domain_matches, is_expired, CacheAdmin, EntryMeta};
use super::types::{CacheBackend, CacheEntry, CacheKey};

/// Redis-based cache implementation
/// This is only available when the "redis-cache" feature is enabled
#[cfg(feature = "redis-cache")]
pub struct RedisCache {
    client: redis::Client,
    /// Kept for `urx cache stats`, which reports where the cache lives. Printed
    /// only through `redact_redis_url`.
    url: String,
}

#[cfg(feature = "redis-cache")]
impl RedisCache {
    /// Create a new Redis cache
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url).context("Failed to create Redis client")?;

        // Test the connection
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .context("Failed to connect to Redis")?;

        redis::cmd("PING")
            .query_async::<()>(&mut conn)
            .await
            .context("Redis ping failed")?;

        Ok(Self {
            client,
            url: redis_url.to_string(),
        })
    }

    /// Generate a Redis key from a cache key
    fn redis_key(&self, key: &CacheKey) -> String {
        format!("urx:cache:{}", key)
    }

    /// Generate a Redis key for metadata
    fn redis_meta_key(&self, key: &CacheKey) -> String {
        format!("urx:meta:{}", key)
    }

    /// Every entry paired with the metadata key it was read from, which the
    /// deletions need in order to address it again.
    async fn entries_with_keys(&self) -> Result<Vec<(String, EntryMeta)>> {
        let mut conn = self.connect().await?;
        let meta_keys = Self::scan_keys(&mut conn, "urx:meta:*").await?;

        let mut entries = Vec::with_capacity(meta_keys.len());
        for meta_key in &meta_keys {
            let (cache_key, meta_key) = Self::pair(meta_key);

            let meta: Option<String> = redis::cmd("GET")
                .arg(&meta_key)
                .query_async(&mut conn)
                .await
                .context("Failed to get metadata from Redis")?;
            let Some(meta) = meta else {
                // Expired or deleted between the scan and the read.
                continue;
            };
            let Ok(meta) = serde_json::from_str::<serde_json::Value>(&meta) else {
                continue;
            };
            let (Some(domain), Some(timestamp)) = (
                meta["domain"].as_str(),
                meta["timestamp"]
                    .as_str()
                    .and_then(|t| t.parse::<DateTime<Utc>>().ok()),
            ) else {
                continue;
            };

            // The URL count lives in the payload, not the metadata, so it costs
            // a second read. Only `urx cache` pays it — the scan path never
            // touches this.
            let payload: Option<String> = redis::cmd("GET")
                .arg(&cache_key)
                .query_async(&mut conn)
                .await
                .context("Failed to get cache entry from Redis")?;
            let url_count = payload
                .and_then(|json| serde_json::from_str::<CacheEntry>(&json).ok())
                .map(|entry| entry.urls.len())
                .unwrap_or(0);

            entries.push((
                meta_key,
                EntryMeta {
                    domain: domain.to_string(),
                    url_count,
                    timestamp,
                },
            ));
        }
        Ok(entries)
    }

    async fn connect(&self) -> Result<redis::aio::MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .context("Failed to connect to Redis")
    }

    /// Every key matching `pattern`, collected with `SCAN` rather than `KEYS`.
    ///
    /// `KEYS` blocks the server for the whole sweep, which is tolerable inside
    /// a scan that touches one key but not for an admin command pointed at a
    /// shared cache holding every domain the team has ever scanned.
    async fn scan_keys(
        conn: &mut redis::aio::MultiplexedConnection,
        pattern: &str,
    ) -> Result<Vec<String>> {
        let mut cursor: u64 = 0;
        let mut keys = Vec::new();
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .arg("COUNT")
                .arg(500)
                .query_async(conn)
                .await
                .context("Failed to scan keys in Redis")?;
            keys.extend(batch);
            if next == 0 {
                break;
            }
            cursor = next;
        }
        // SCAN may hand back the same key twice across cursor iterations.
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    /// The cache/meta key pair for one entry, given either half.
    fn pair(meta_key: &str) -> (String, String) {
        (
            meta_key.replace("urx:meta:", "urx:cache:"),
            meta_key.to_string(),
        )
    }

    /// Delete the entries whose metadata keys are listed, returning how many
    /// entries actually went.
    async fn delete_meta_keys(
        conn: &mut redis::aio::MultiplexedConnection,
        meta_keys: &[String],
    ) -> Result<usize> {
        let mut deleted = 0usize;
        for meta_key in meta_keys {
            let (cache_key, meta_key) = Self::pair(meta_key);
            let removed: usize = redis::cmd("DEL")
                .arg(&cache_key)
                .arg(&meta_key)
                .query_async(conn)
                .await
                .context("Failed to delete entry from Redis")?;
            // DEL counts keys, and one entry is two of them; an entry whose
            // payload or metadata is already gone still counts as one entry.
            if removed > 0 {
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}

#[cfg(feature = "redis-cache")]
#[async_trait]
impl CacheBackend for RedisCache {
    async fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("Failed to connect to Redis")?;

        let redis_key = self.redis_key(key);
        let value: Option<String> = redis::cmd("GET")
            .arg(&redis_key)
            .query_async(&mut conn)
            .await
            .context("Failed to get value from Redis")?;

        match value {
            Some(json_str) => {
                let entry: CacheEntry =
                    serde_json::from_str(&json_str).context("Failed to deserialize cache entry")?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    async fn set(&self, key: &CacheKey, entry: &CacheEntry) -> Result<()> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("Failed to connect to Redis")?;

        let redis_key = self.redis_key(key);
        let json_str = serde_json::to_string(entry).context("Failed to serialize cache entry")?;

        redis::cmd("SET")
            .arg(&redis_key)
            .arg(&json_str)
            .query_async::<()>(&mut conn)
            .await
            .context("Failed to set value in Redis")?;

        // Also store metadata for cleanup purposes
        let meta_key = self.redis_meta_key(key);
        let meta_data = serde_json::json!({
            "domain": key.domain,
            "providers": key.providers,
            "timestamp": entry.timestamp.to_rfc3339()
        });

        redis::cmd("SET")
            .arg(&meta_key)
            .arg(meta_data.to_string())
            .query_async::<()>(&mut conn)
            .await
            .context("Failed to set metadata in Redis")?;

        Ok(())
    }

    async fn delete(&self, key: &CacheKey) -> Result<()> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("Failed to connect to Redis")?;

        let redis_key = self.redis_key(key);
        let meta_key = self.redis_meta_key(key);

        redis::cmd("DEL")
            .arg(&redis_key)
            .arg(&meta_key)
            .query_async::<()>(&mut conn)
            .await
            .context("Failed to delete from Redis")?;

        Ok(())
    }

    async fn cleanup_expired(&self, ttl_seconds: u64) -> Result<()> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("Failed to connect to Redis")?;

        // `Duration::seconds` panics past its bounds and `ttl_seconds as i64`
        // wraps a huge TTL negative, which would push the cutoff into the future
        // and wipe every entry. The SQLite backend already guards this; share
        // the same helper so the two cannot diverge again.
        let cutoff_time = super::types::expiry_cutoff(ttl_seconds);

        // Get all metadata keys
        let meta_keys: Vec<String> = redis::cmd("KEYS")
            .arg("urx:meta:*")
            .query_async(&mut conn)
            .await
            .context("Failed to get metadata keys from Redis")?;

        for meta_key in meta_keys {
            let meta_value: Option<String> = redis::cmd("GET")
                .arg(&meta_key)
                .query_async(&mut conn)
                .await
                .context("Failed to get metadata from Redis")?;

            if let Some(meta_str) = meta_value {
                if let Ok(meta_json) = serde_json::from_str::<serde_json::Value>(&meta_str) {
                    if let Some(timestamp_str) = meta_json["timestamp"].as_str() {
                        if let Ok(timestamp) = timestamp_str.parse::<DateTime<Utc>>() {
                            if timestamp < cutoff_time {
                                // This entry is expired, delete it
                                let cache_key = meta_key.replace("urx:meta:", "urx:cache:");
                                redis::cmd("DEL")
                                    .arg(&cache_key)
                                    .arg(&meta_key)
                                    .query_async::<()>(&mut conn)
                                    .await
                                    .context("Failed to delete expired entry from Redis")?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn exists(&self, key: &CacheKey) -> Result<bool> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("Failed to connect to Redis")?;

        let redis_key = self.redis_key(key);
        let exists: bool = redis::cmd("EXISTS")
            .arg(&redis_key)
            .query_async(&mut conn)
            .await
            .context("Failed to check existence in Redis")?;

        Ok(exists)
    }
}

#[cfg(feature = "redis-cache")]
#[async_trait]
impl CacheAdmin for RedisCache {
    fn backend_name(&self) -> &'static str {
        "redis"
    }

    fn location(&self) -> String {
        // `--redis-url` routinely carries a password and this string is printed
        // (and written into `-f json` output people paste into issues).
        super::admin::redact_redis_url(&self.url)
    }

    async fn entries(&self) -> Result<Vec<EntryMeta>> {
        Ok(self
            .entries_with_keys()
            .await?
            .into_iter()
            .map(|(_, entry)| entry)
            .collect())
    }

    async fn size_bytes(&self) -> Result<Option<u64>> {
        // `MEMORY USAGE` is not available on every Redis-compatible server, so
        // this reports the bytes of the stored values instead — portable, and
        // the number that actually tracks what urx put there.
        let mut conn = self.connect().await?;
        let mut total = 0u64;
        for pattern in ["urx:cache:*", "urx:meta:*"] {
            for key in Self::scan_keys(&mut conn, pattern).await? {
                let len: u64 = redis::cmd("STRLEN")
                    .arg(&key)
                    .query_async(&mut conn)
                    .await
                    .context("Failed to measure a Redis value")?;
                total += len;
            }
        }
        Ok(Some(total))
    }

    async fn delete_expired(&self, ttl_seconds: u64) -> Result<usize> {
        let expired: Vec<String> = self
            .entries_with_keys()
            .await?
            .into_iter()
            .filter(|(_, entry)| is_expired(entry.timestamp, ttl_seconds))
            .map(|(meta_key, _)| meta_key)
            .collect();

        let mut conn = self.connect().await?;
        Self::delete_meta_keys(&mut conn, &expired).await
    }

    async fn delete_domains(&self, patterns: &[String]) -> Result<usize> {
        let doomed: Vec<String> = self
            .entries_with_keys()
            .await?
            .into_iter()
            .filter(|(_, entry)| patterns.iter().any(|p| domain_matches(p, &entry.domain)))
            .map(|(meta_key, _)| meta_key)
            .collect();

        let mut conn = self.connect().await?;
        Self::delete_meta_keys(&mut conn, &doomed).await
    }

    async fn clear(&self) -> Result<usize> {
        let mut conn = self.connect().await?;
        let meta_keys = Self::scan_keys(&mut conn, "urx:meta:*").await?;
        let deleted = Self::delete_meta_keys(&mut conn, &meta_keys).await?;

        // Payloads whose metadata key is already gone would otherwise be
        // orphaned forever: nothing else ever scans for them.
        let orphans = Self::scan_keys(&mut conn, "urx:cache:*").await?;
        if !orphans.is_empty() {
            let mut cmd = redis::cmd("DEL");
            for key in &orphans {
                cmd.arg(key);
            }
            cmd.query_async::<usize>(&mut conn)
                .await
                .context("Failed to delete orphaned entries from Redis")?;
        }

        Ok(deleted)
    }
}

#[cfg(test)]
#[cfg(feature = "redis-cache")]
mod tests {
    use super::*;
    use crate::cache::types::CacheFilters;

    async fn create_test_redis() -> Result<RedisCache> {
        // This test requires a Redis server running on localhost:6379
        // Skip if Redis is not available
        RedisCache::new("redis://127.0.0.1:6379").await
    }

    #[tokio::test]
    #[ignore] // Ignored by default since it requires Redis server
    async fn test_redis_cache_basic_operations() -> Result<()> {
        let cache = match create_test_redis().await {
            Ok(cache) => cache,
            Err(_) => {
                println!("Redis server not available, skipping test");
                return Ok(());
            }
        };

        let filters = CacheFilters {
            strict: true,
            ..Default::default()
        };

        let key = CacheKey::new("example.com", &["wayback".to_string()], &filters);
        let entry = CacheEntry::new(vec!["https://example.com/page1".to_string()]);

        // Clean up any existing data
        let _ = cache.delete(&key).await;

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
    #[ignore] // Ignored by default since it requires Redis server
    async fn test_redis_cache_cleanup_expired() -> Result<()> {
        let cache = match create_test_redis().await {
            Ok(cache) => cache,
            Err(_) => {
                println!("Redis server not available, skipping test");
                return Ok(());
            }
        };

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
}
