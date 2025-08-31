use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use tokio::task;

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
            tokio::fs::create_dir_all(parent).await
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
            let conn = Connection::open(&db_path)
                .context("Failed to open SQLite database")?;

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
            ).context("Failed to create cache table")?;

            // Create index for better performance
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_cache_key ON url_cache(cache_key)",
                [],
            ).context("Failed to create cache key index")?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_domain ON url_cache(domain)",
                [],
            ).context("Failed to create domain index")?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_timestamp ON url_cache(timestamp)",
                [],
            ).context("Failed to create timestamp index")?;

            Ok::<(), anyhow::Error>(())
        }).await??;

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
            let conn = Connection::open(&db_path)
                .context("Failed to open SQLite database")?;
            f(&conn)
        }).await?
    }
}

#[async_trait]
impl CacheBackend for SqliteCache {
    async fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
        let cache_key = key.to_string();
        
        self.with_connection(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT urls, timestamp FROM url_cache WHERE cache_key = ?1"
            )?;

            let result = stmt.query_row(params![cache_key], |row| {
                let urls_json: String = row.get(0)?;
                let timestamp_str: String = row.get(1)?;
                
                let urls: Vec<String> = serde_json::from_str(&urls_json)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        0, rusqlite::types::Type::Text, Box::new(e)
                    ))?;
                
                let timestamp: DateTime<Utc> = timestamp_str.parse()
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        1, rusqlite::types::Type::Text, Box::new(e)
                    ))?;
                
                Ok(CacheEntry { urls, timestamp })
            }).optional()?;

            Ok(result)
        }).await
    }

    async fn set(&self, key: &CacheKey, entry: &CacheEntry) -> Result<()> {
        let cache_key = key.to_string();
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
        }).await
    }

    async fn delete(&self, key: &CacheKey) -> Result<()> {
        let cache_key = key.to_string();
        
        self.with_connection(move |conn| {
            conn.execute(
                "DELETE FROM url_cache WHERE cache_key = ?1",
                params![cache_key],
            )?;
            Ok(())
        }).await
    }

    async fn cleanup_expired(&self, ttl_seconds: u64) -> Result<()> {
        let cutoff_time = Utc::now() - chrono::Duration::seconds(ttl_seconds as i64);
        let cutoff_str = cutoff_time.to_rfc3339();

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
        }).await
    }

    async fn exists(&self, key: &CacheKey) -> Result<bool> {
        let cache_key = key.to_string();
        
        self.with_connection(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM url_cache WHERE cache_key = ?1",
                params![cache_key],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::cache::types::CacheFilters;

    #[tokio::test]
    async fn test_sqlite_cache_basic_operations() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");
        
        let cache = SqliteCache::new(&db_path).await?;
        
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

    #[tokio::test]
    async fn test_sqlite_cache_multiple_entries() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");
        
        let cache = SqliteCache::new(&db_path).await?;
        
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