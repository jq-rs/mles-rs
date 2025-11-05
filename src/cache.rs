/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 */
use indexmap::IndexMap;
use siphasher::sip::SipHasher24;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::Mutex;

const MB: usize = 1024 * 1024;
const DEFAULT_FILE_SIZE_MB: usize = 10;
const MAX_FILE_SIZE: usize = DEFAULT_FILE_SIZE_MB * MB; // 10MB max single file size

#[derive(Clone, Eq, PartialEq, Debug)]
struct CacheKey {
    path: String,
    compression: String,
}

// Custom Hash implementation to optimize key hashing
impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Pre-compute hash using SipHasher24 for better performance
        let mut hasher = SipHasher24::new();
        self.path.hash(&mut hasher);
        self.compression.hash(&mut hasher);
        state.write_u64(hasher.finish());
    }
}

#[derive(Debug)]
struct CacheEntry {
    data: Vec<u8>,
}

impl CacheEntry {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

#[derive(Debug)]
pub(crate) struct CompressionCache {
    entries: IndexMap<CacheKey, CacheEntry>,
    current_size: usize,
    max_size: usize,
}

impl CompressionCache {
    pub fn new(max_size_mb: usize) -> Self {
        // No pre-allocation if caching is disabled
        if max_size_mb == 0 {
            return Self {
                entries: IndexMap::new(),
                current_size: 0,
                max_size: 0,
            };
        }

        // Pre-allocate space for better insertion performance
        // Assuming average compression ratio of 4:1 and 10MB files
        let estimated_entries = max_size_mb / DEFAULT_FILE_SIZE_MB * 4;
        Self {
            entries: IndexMap::with_capacity(estimated_entries),
            current_size: 0,
            max_size: max_size_mb * MB,
        }
    }

    fn make_space(&mut self, required_size: usize) {
        // Skip if cache is disabled or file is too large
        if self.max_size == 0 || required_size > MAX_FILE_SIZE {
            return;
        }

        // Remove oldest entries until we have enough space
        while self.current_size + required_size > self.max_size && !self.entries.is_empty() {
            // Use swap_remove_index since we don't care about order when removing from the end
            if let Some((_, entry)) = self.entries.swap_remove_index(self.entries.len() - 1) {
                self.current_size -= entry.data.len();
            }
        }
    }

    pub fn get(&mut self, path: &str, compression: &str) -> Option<Vec<u8>> {
        let key = CacheKey {
            path: path.to_string(),
            compression: compression.to_string(),
        };

        if let Some(i) = self.entries.get_index_of(&key) {
            let entry = self.entries.get_index_mut(i).unwrap().1;
            let data = entry.data.clone();
            // Move to front (most recently used) using shift operations
            let (k, v) = self.entries.shift_remove_index(i).unwrap();
            self.entries.shift_insert(0, k, v);
            Some(data)
        } else {
            None
        }
    }

    pub fn insert(&mut self, path: &str, compression: &str, data: Vec<u8>) {
        let size = data.len();

        // Skip if file is too large
        if size > MAX_FILE_SIZE {
            return;
        }

        self.make_space(size);

        // Only insert if cache is enabled and we have enough space
        if self.max_size > 0 && self.current_size + size <= self.max_size {
            let key = CacheKey {
                path: path.to_string(),
                compression: compression.to_string(),
            };
            self.current_size += size;
            self.entries.shift_insert(0, key, CacheEntry::new(data));
        }
    }
}

// Thread-safe wrapper
pub(crate) type SharedCache = Arc<Mutex<CompressionCache>>;

pub(crate) fn create_cache(max_size_mb: usize) -> SharedCache {
    Arc::new(Mutex::new(CompressionCache::new(max_size_mb)))
}
