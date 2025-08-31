use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::types::{CacheBackend, CacheEntry, CacheKey};

/// Redis-based cache implementation
/// This is only available when the "redis-cache" feature is enabled
#[cfg(feature = "redis-cache")]
pub struct RedisCache {
    client: redis::Client,
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

        Ok(Self { client })
    }

    /// Generate a Redis key from a cache key
    fn redis_key(&self, key: &CacheKey) -> String {
        format!("urx:cache:{}", key)
    }

    /// Generate a Redis key for metadata
    fn redis_meta_key(&self, key: &CacheKey) -> String {
        format!("urx:meta:{}", key)
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

        let cutoff_time = Utc::now() - chrono::Duration::seconds(ttl_seconds as i64);

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
            subs: false,
            extensions: vec![],
            exclude_extensions: vec![],
            patterns: vec![],
            exclude_patterns: vec![],
            presets: vec![],
            min_length: None,
            max_length: None,
            strict: true,
            normalize_url: false,
            merge_endpoint: false,
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
            subs: false,
            extensions: vec![],
            exclude_extensions: vec![],
            patterns: vec![],
            exclude_patterns: vec![],
            presets: vec![],
            min_length: None,
            max_length: None,
            strict: true,
            normalize_url: false,
            merge_endpoint: false,
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
