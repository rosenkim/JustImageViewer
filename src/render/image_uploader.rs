use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::{core::{image_manager::ImageManager, media::MediaEntry}, render::imgui_textures};
use crate::render::imgui_textures::ImguiTextures;
use anyhow::{Result, bail};
use imgui::TextureId;
use wgpu::{Device, Queue};

pub struct UploadedTexture {
    pub id: TextureId,
    pub width: usize,
    pub height: usize,
}

struct TextureRecord {
    texture_id: TextureId,
    width: usize,
    height: usize,
    last_used: u32,
}

pub struct ImageUploader {
    textures: HashMap<PathBuf, TextureRecord>,
    max_texture_size: u32,
    max_cache_size: usize,
    access_counter: u32,
}

impl ImageUploader {
    /// Create texture cache and decoded image cache.
    pub fn new(
        max_texture_size: u32,
        max_cache_size: usize,
    ) -> Self {
        Self {
            textures: HashMap::new(),
            max_texture_size,
            max_cache_size,
            access_counter: 0,
        }
    }

    /// Return existing texture for a path, or upload a new GPU texture.
    pub fn get_or_upload(
        &mut self,
        path: &Path,
        entry: &MediaEntry,
        device: &Device,
        queue: &Queue,
        renderer: &mut imgui_wgpu::Renderer,
        imgui_textures: &mut ImguiTextures,
        image_manager: &mut ImageManager,
    ) -> Result<UploadedTexture> {
        if let Some(existing) = self.textures.get_mut(path) {
            self.access_counter += 1;
            existing.last_used = self.access_counter;
            return Ok(UploadedTexture {
                id: existing.texture_id,
                width: existing.width,
                height: existing.height,
            });
        }

        let decoded = match image_manager.get_or_load_rgba(&entry.path) {
            Ok(decoded) => decoded,
            Err(e) => {
                bail!("failed to load image: {}", e);
            }
        };

        if decoded.width as u32 > self.max_texture_size
            || decoded.height as u32 > self.max_texture_size
        {
            bail!(
                "image size {}x{} exceeds GPU max texture size {}",
                decoded.width,
                decoded.height,
                self.max_texture_size
            );
        }

        // Use ImguiTextures to create the texture
        let texture_id = imgui_textures.create_from_rgba_data(
            device,
            queue,
            renderer,
            decoded.width as u32,
            decoded.height as u32,
            &decoded.pixels,
            true, // Use linear filter as before
        )?;

        self.evict_if_full(renderer, imgui_textures);

        self.access_counter += 1;
        let record = TextureRecord {
            texture_id,
            width: decoded.width,
            height: decoded.height,
            last_used: self.access_counter,
        };

        self.textures.insert(path.to_path_buf(), record);

        Ok(UploadedTexture {
            id: texture_id,
            width: decoded.width,
            height: decoded.height,
        })
    }

    /// Evict the least-recently-used entry if the cache is at capacity.
    fn evict_if_full(&mut self, renderer: &mut imgui_wgpu::Renderer, imgui_textures: &mut ImguiTextures) {
        if self.textures.len() < self.max_cache_size {
            return;
        }

        let oldest_key = self
            .textures
            .iter()
            .min_by_key(|(_, record)| record.last_used)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest_key {
            if let Some(record) = self.textures.remove(&key) {
                log::debug!("Evicting cached texture: {}", key.display());
                imgui_textures.remove(renderer, record.texture_id);
            }
        }
    }

    /// Free all GPU textures owned by this manager.
    pub fn clear(&mut self, renderer: &mut imgui_wgpu::Renderer, imgui_textures: &mut ImguiTextures) {
        self.textures.clear();
        imgui_textures.clear(renderer);
    }
}
