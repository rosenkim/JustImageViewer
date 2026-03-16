use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use std::sync::Arc;

struct CachedImage {
    binary: Arc<Vec<u8>>,
    last_used: u64,
}

/// Cache binary data in an LRU cache.
pub struct ImageManager {
    binary_cache: HashMap<PathBuf, CachedImage>,
    max_cache_count: usize,
    access_counter: u64,
}

impl ImageManager {
    /// Create an image manager with binary cache capacity.
    pub fn new(max_cache_count: usize) -> Self {
        Self {
            binary_cache: HashMap::new(),
            max_cache_count: max_cache_count.max(1),
            access_counter: 0,
        }
    }

    /// Remove least-recently-used binary when cache is full.
    pub fn evict_if_full(&mut self) {
        if self.binary_cache.len() < self.max_cache_count {
            return;
        }

        let oldest_key = self
            .binary_cache
            .iter()
            .min_by_key(|(_, record)| record.last_used)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest_key {
            log::debug!("Evicting binary cache: {}", key.display());
            self.binary_cache.remove(&key);
        }
    }

    /// Return a clone of the cached binary for `path`, if present.
    /// Used to pass data to a background decode thread without holding &mut self.
    pub fn take_cached_binary(&mut self, path: &Path) -> Option<Arc<Vec<u8>>> {
        if let Some(record) = self.binary_cache.get_mut(path) {
            self.access_counter += 1;
            record.last_used = self.access_counter;
            Some(record.binary.clone())
        } else {
            None
        }
    }

    /// Store a binary that was freshly read from disk (e.g. by the async decode thread).
    /// Does nothing if the path is already cached.
    pub fn store_binary(&mut self, path: PathBuf, binary: Vec<u8>) {
        if self.binary_cache.contains_key(&path) {
            return;
        }
        self.evict_if_full();
        self.access_counter += 1;
        self.binary_cache.insert(path, CachedImage {
            binary: Arc::new(binary),
            last_used: self.access_counter,
        });
    }

    pub fn clear(&mut self) {
        self.binary_cache.clear();
    }
}

