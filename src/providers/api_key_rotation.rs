use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Thread-safe API key rotation manager
#[derive(Clone, Debug)]
pub struct ApiKeyRotator {
    keys: Vec<String>,
    counter: Arc<AtomicUsize>,
}

impl ApiKeyRotator {
    /// Create a new API key rotator with the given keys
    pub fn new(keys: Vec<String>) -> Self {
        ApiKeyRotator {
            keys,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Get the next API key in rotation
    pub fn next_key(&self) -> Option<String> {
        if self.keys.is_empty() {
            return None;
        }

        let index = self.counter.fetch_add(1, Ordering::Relaxed) % self.keys.len();
        Some(self.keys[index].clone())
    }

    /// Get the current key without advancing the rotation
    #[allow(dead_code)]
    pub fn current_key(&self) -> Option<String> {
        if self.keys.is_empty() {
            return None;
        }

        let index = self.counter.load(Ordering::Relaxed) % self.keys.len();
        Some(self.keys[index].clone())
    }

    /// Check if the rotator has any keys
    pub fn has_keys(&self) -> bool {
        !self.keys.is_empty()
    }

    /// Get the number of available keys
    #[allow(dead_code)]
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::thread;

    #[test]
    fn test_new_rotator() {
        let keys = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let rotator = ApiKeyRotator::new(keys.clone());

        assert_eq!(rotator.key_count(), 3);
        assert!(rotator.has_keys());
    }

    #[test]
    fn test_empty_rotator() {
        let rotator = ApiKeyRotator::new(vec![]);

        assert_eq!(rotator.key_count(), 0);
        assert!(!rotator.has_keys());
        assert!(rotator.next_key().is_none());
        assert!(rotator.current_key().is_none());
    }

    #[test]
    fn test_single_key_rotation() {
        let keys = vec!["single_key".to_string()];
        let rotator = ApiKeyRotator::new(keys);

        // Should always return the same key
        for _ in 0..5 {
            assert_eq!(rotator.next_key(), Some("single_key".to_string()));
        }
    }

    #[test]
    fn test_multiple_key_rotation() {
        let keys = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let rotator = ApiKeyRotator::new(keys);

        // Test rotation order
        assert_eq!(rotator.next_key(), Some("key1".to_string()));
        assert_eq!(rotator.next_key(), Some("key2".to_string()));
        assert_eq!(rotator.next_key(), Some("key3".to_string()));
        assert_eq!(rotator.next_key(), Some("key1".to_string())); // Should wrap around
        assert_eq!(rotator.next_key(), Some("key2".to_string()));
    }

    #[test]
    fn test_current_key() {
        let keys = vec!["key1".to_string(), "key2".to_string()];
        let rotator = ApiKeyRotator::new(keys);

        // Current key should be key1 initially
        assert_eq!(rotator.current_key(), Some("key1".to_string()));

        // After next_key(), current should change
        assert_eq!(rotator.next_key(), Some("key1".to_string()));
        assert_eq!(rotator.current_key(), Some("key2".to_string()));

        // Call current_key multiple times - should not advance
        assert_eq!(rotator.current_key(), Some("key2".to_string()));
        assert_eq!(rotator.current_key(), Some("key2".to_string()));
    }

    #[test]
    fn test_thread_safety() {
        let keys = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];
        let rotator = Arc::new(ApiKeyRotator::new(keys));

        let mut handles = vec![];
        let mut all_keys = vec![];

        // Spawn multiple threads to get keys concurrently
        for _ in 0..10 {
            let rotator_clone = rotator.clone();
            let handle = thread::spawn(move || {
                let mut thread_keys = vec![];
                for _ in 0..3 {
                    if let Some(key) = rotator_clone.next_key() {
                        thread_keys.push(key);
                    }
                }
                thread_keys
            });
            handles.push(handle);
        }

        // Collect all keys from all threads
        for handle in handles {
            let thread_keys = handle.join().unwrap();
            all_keys.extend(thread_keys);
        }

        // Verify we got the expected number of keys
        assert_eq!(all_keys.len(), 30); // 10 threads * 3 keys each

        // Verify all keys are from our original set
        let unique_keys: HashSet<String> = all_keys.into_iter().collect();
        let expected_keys: HashSet<String> = ["key1", "key2", "key3"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert!(unique_keys.is_subset(&expected_keys));
        assert_eq!(unique_keys, expected_keys);
    }
}
