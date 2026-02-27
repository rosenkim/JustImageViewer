use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::core::image_loader::DecodedImage;

struct CachedImage {
    binary: Vec<u8>,
    last_used: u64,
}

/// Load image files and keep raw binary data in an LRU cache.
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

    /// Return decoded image for `path`, using cached binary if available.
    pub fn get_or_load_rgba(&mut self, path: &Path) -> Result<DecodedImage> {
        if let Some(record) = self.binary_cache.get_mut(path) {
            self.access_counter += 1;
            record.last_used = self.access_counter;
            return decode_from_binary(&record.binary);
        }

        let binary = std::fs::read(path)?;
        let decoded = decode_from_binary(&binary)?;

        self.evict_if_full();

        self.access_counter += 1;
        self.binary_cache.insert(
            path.to_path_buf(),
            CachedImage {
                binary,
                last_used: self.access_counter,
            },
        );

        Ok(decoded)
    }

    /// Remove least-recently-used binary when cache is full.
    fn evict_if_full(&mut self) {
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

    pub fn clear(&mut self) {
        self.binary_cache.clear();
    }
}

fn decode_from_binary(binary: &[u8]) -> Result<DecodedImage> {
    let dyn_image = image::load_from_memory(binary)?;
    let rgba_image = dyn_image.to_rgba8();
    let (width, height) = rgba_image.dimensions();
    let pixels = std::sync::Arc::<[u8]>::from(rgba_image.into_raw());

    Ok(DecodedImage {
        width: width as usize,
        height: height as usize,
        pixels,
    })
}
