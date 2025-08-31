use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Cache key that uniquely identifies a scan configuration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    pub domain: String,
    pub providers: Vec<String>,
    pub filters_hash: String,
}

impl CacheKey {
    /// Create a new cache key from scan parameters
    pub fn new(
        domain: &str,
        providers: &[String],
        filters: &CacheFilters,
    ) -> Self {
        let mut providers = providers.to_vec();
        providers.sort(); // Ensure consistent ordering
        
        let filters_hash = filters.compute_hash();
        
        Self {
            domain: domain.to_string(),
            providers,
            filters_hash,
        }
    }

    /// Generate a unique string representation for storage
    pub fn to_string(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.domain);
        hasher.update(&self.providers.join(","));
        hasher.update(&self.filters_hash);
        format!("{:x}", hasher.finalize())
    }
}

/// Represents the filtering configuration used in a scan
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheFilters {
    pub subs: bool,
    pub extensions: Vec<String>,
    pub exclude_extensions: Vec<String>,
    pub patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub presets: Vec<String>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub strict: bool,
    pub normalize_url: bool,
    pub merge_endpoint: bool,
}

impl CacheFilters {
    /// Compute a hash of the filter configuration
    pub fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        
        hasher.update(if self.subs { "1" } else { "0" });
        hasher.update(&self.extensions.join(","));
        hasher.update(&self.exclude_extensions.join(","));
        hasher.update(&self.patterns.join(","));
        hasher.update(&self.exclude_patterns.join(","));
        hasher.update(&self.presets.join(","));
        hasher.update(&self.min_length.map(|l| l.to_string()).unwrap_or_default());
        hasher.update(&self.max_length.map(|l| l.to_string()).unwrap_or_default());
        hasher.update(if self.strict { "1" } else { "0" });
        hasher.update(if self.normalize_url { "1" } else { "0" });
        hasher.update(if self.merge_endpoint { "1" } else { "0" });
        
        format!("{:x}", hasher.finalize())
    }
}

/// Cache entry containing URLs and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub urls: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

impl CacheEntry {
    /// Create a new cache entry
    pub fn new(urls: Vec<String>) -> Self {
        Self {
            urls,
            timestamp: Utc::now(),
        }
    }

    /// Check if the cache entry is expired
    pub fn is_expired(&self, ttl_seconds: u64) -> bool {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(self.timestamp).num_seconds() as u64;
        elapsed >= ttl_seconds
    }
}

/// Trait defining the interface for cache backends
#[async_trait]
pub trait CacheBackend: Send + Sync {
    /// Get a cache entry by key
    async fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>>;
    
    /// Set a cache entry
    async fn set(&self, key: &CacheKey, entry: &CacheEntry) -> Result<()>;
    
    /// Delete a cache entry
    async fn delete(&self, key: &CacheKey) -> Result<()>;
    
    /// Clean up expired entries
    async fn cleanup_expired(&self, ttl_seconds: u64) -> Result<()>;
    
    /// Check if a key exists in the cache
    async fn exists(&self, key: &CacheKey) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_creation() {
        let filters = CacheFilters {
            subs: true,
            extensions: vec!["js".to_string(), "php".to_string()],
            exclude_extensions: vec!["jpg".to_string()],
            patterns: vec!["admin".to_string()],
            exclude_patterns: vec!["logout".to_string()],
            presets: vec!["no-images".to_string()],
            min_length: Some(10),
            max_length: Some(100),
            strict: true,
            normalize_url: true,
            merge_endpoint: false,
        };

        let key = CacheKey::new(
            "example.com",
            &["wayback".to_string(), "cc".to_string()],
            &filters,
        );

        assert_eq!(key.domain, "example.com");
        assert_eq!(key.providers, vec!["cc", "wayback"]); // sorted
        assert!(!key.filters_hash.is_empty());
    }

    #[test]
    fn test_cache_filters_hash_consistency() {
        let filters1 = CacheFilters {
            subs: true,
            extensions: vec!["js".to_string(), "php".to_string()],
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

        let filters2 = CacheFilters {
            subs: true,
            extensions: vec!["js".to_string(), "php".to_string()],
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

        assert_eq!(filters1.compute_hash(), filters2.compute_hash());
    }

    #[test]
    fn test_cache_filters_hash_different() {
        let filters1 = CacheFilters {
            subs: true,
            extensions: vec!["js".to_string()],
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

        let filters2 = CacheFilters {
            subs: false, // Different
            extensions: vec!["js".to_string()],
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

        assert_ne!(filters1.compute_hash(), filters2.compute_hash());
    }

    #[test]
    fn test_cache_entry_expiry() {
        let mut entry = CacheEntry::new(vec!["https://example.com".to_string()]);
        
        // Fresh entry should not be expired
        assert!(!entry.is_expired(3600)); // 1 hour TTL
        
        // Simulate old entry
        entry.timestamp = Utc::now() - chrono::Duration::hours(2);
        assert!(entry.is_expired(3600)); // Should be expired
    }

    #[test]
    fn test_cache_key_string_representation() {
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
        let key2 = CacheKey::new("example.com", &["wayback".to_string()], &filters);
        let key3 = CacheKey::new("different.com", &["wayback".to_string()], &filters);

        // Same keys should have same string representation
        assert_eq!(key1.to_string(), key2.to_string());
        
        // Different keys should have different string representation
        assert_ne!(key1.to_string(), key3.to_string());
    }
}