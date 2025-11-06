/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

const MB: usize = 1024 * 1024;
const DEFAULT_FILE_SIZE_MB: usize = 1;
const MAX_FILE_SIZE: usize = DEFAULT_FILE_SIZE_MB * MB;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
struct CacheKey {
    path: String,
    compression: String,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    data: Arc<Vec<u8>>,
    size: usize,
}

impl CacheEntry {
    fn new(data: Vec<u8>) -> Self {
        let size = data.len();
        Self {
            data: Arc::new(data),
            size,
        }
    }
}

#[derive(Debug)]
pub(crate) struct CompressionCache {
    cache: LruCache<CacheKey, CacheEntry>,
    current_size: usize,
    max_size: usize,
}

impl CompressionCache {
    pub fn new(max_size_mb: usize) -> Self {
        if max_size_mb == 0 {
            return Self {
                cache: LruCache::unbounded(),
                current_size: 0,
                max_size: 0,
            };
        }

        // Estimate maximum number of entries based on average compressed file size
        // Assuming 4:1 compression ratio on 1MB files = ~250KB per entry
        let estimated_entries = (max_size_mb * 4).max(100);
        let capacity = NonZeroUsize::new(estimated_entries).unwrap();

        Self {
            cache: LruCache::new(capacity),
            current_size: 0,
            max_size: max_size_mb * MB,
        }
    }

    fn make_space(&mut self, required_size: usize) {
        if self.max_size == 0 || required_size > MAX_FILE_SIZE {
            return;
        }

        // Remove least recently used entries until we have space
        while self.current_size + required_size > self.max_size {
            if let Some((_, entry)) = self.cache.pop_lru() {
                self.current_size -= entry.size;
            } else {
                break;
            }
        }
    }

    pub fn get(&mut self, path: &str, compression: &str) -> Option<Arc<Vec<u8>>> {
        let key = CacheKey {
            path: path.to_string(),
            compression: compression.to_string(),
        };

        // O(1) lookup and automatic LRU update!
        self.cache.get(&key).map(|entry| Arc::clone(&entry.data))
    }

    pub fn insert(&mut self, path: &str, compression: &str, data: Vec<u8>) {
        let size = data.len();

        if size > MAX_FILE_SIZE {
            return;
        }

        let key = CacheKey {
            path: path.to_string(),
            compression: compression.to_string(),
        };

        // Remove existing entry if present (to update size tracking)
        if let Some(old_entry) = self.cache.pop(&key) {
            self.current_size -= old_entry.size;
        }

        self.make_space(size);

        if self.max_size > 0 && self.current_size + size <= self.max_size {
            self.current_size += size;
            self.cache.put(key, CacheEntry::new(data));
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    #[allow(dead_code)]
    pub fn current_size(&self) -> usize {
        self.current_size
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.cache.clear();
        self.current_size = 0;
    }
}

// Thread-safe wrapper
pub(crate) type SharedCache = Arc<RwLock<CompressionCache>>;

pub(crate) fn create_cache(max_size_mb: usize) -> SharedCache {
    Arc::new(RwLock::new(CompressionCache::new(max_size_mb)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_operations() {
        let mut cache = CompressionCache::new(10);

        cache.insert("file1.js", "br", vec![1, 2, 3]);

        // Get returns Arc - O(1) and updates LRU automatically!
        let data = cache.get("file1.js", "br").unwrap();
        assert_eq!(*data, vec![1, 2, 3]);

        // Can share the Arc
        let data2 = Arc::clone(&data);
        assert_eq!(*data2, vec![1, 2, 3]);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = CompressionCache::new(1); // 1MB

        let large_data = vec![0u8; 500_000]; // 500KB
        cache.insert("file1", "br", large_data.clone());
        cache.insert("file2", "br", large_data.clone());

        // Both should be present
        assert!(cache.get("file1", "br").is_some());
        assert!(cache.get("file2", "br").is_some());

        // Access file1 to make it most recently used
        cache.get("file1", "br");

        // Add another - should evict file2 (least recently used)
        cache.insert("file3", "br", large_data);
        assert!(cache.get("file1", "br").is_some()); // Still present (recently used)
        assert!(cache.get("file2", "br").is_none());  // Evicted (least recently used)
        assert!(cache.get("file3", "br").is_some());
    }

    #[test]
    fn test_update_existing() {
        let mut cache = CompressionCache::new(10);

        cache.insert("a", "br", vec![1, 2, 3]);
        cache.insert("a", "br", vec![4, 5, 6, 7]); // Update

        let data = cache.get("a", "br").unwrap();
        assert_eq!(*data, vec![4, 5, 6, 7]);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_max_file_size() {
        let mut cache = CompressionCache::new(10);

        // Try to insert file larger than MAX_FILE_SIZE (1MB)
        let huge_data = vec![0u8; 2_000_000]; // 2MB
        cache.insert("huge", "br", huge_data);

        // Should not be cached
        assert!(cache.get("huge", "br").is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_access_updates_lru() {
        let mut cache = CompressionCache::new(1); // 1MB

        let data = vec![0u8; 400_000]; // 400KB each
        cache.insert("a", "br", data.clone());
        cache.insert("b", "br", data.clone());

        // Access 'a' to make it recently used
        cache.get("a", "br");

        // Insert 'c' - should evict 'b' (least recently used), not 'a'
        cache.insert("c", "br", data);

        assert!(cache.get("a", "br").is_some());
        assert!(cache.get("b", "br").is_none()); // Evicted
        assert!(cache.get("c", "br").is_some());
    }
}
