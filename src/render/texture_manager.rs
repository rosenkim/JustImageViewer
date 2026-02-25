use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Result, bail};
use imgui::TextureId;
use imgui_wgpu::{Renderer, Texture, TextureConfig};
use wgpu::{Device, Extent3d, Queue, TextureFormat};

use crate::core::image_loader::DecodedImage;

#[derive(Clone)]
pub struct UploadedTexture {
    pub id: TextureId,
    pub width: usize,
    pub height: usize,
    pub pixels: Arc<[u8]>,
}

struct TextureRecord {
    texture_id: TextureId,
    width: usize,
    height: usize,
    pixels: Arc<[u8]>,
    last_used: u32,
}

pub struct TextureManager {
    textures: HashMap<PathBuf, TextureRecord>,
    max_texture_size: u32,
    max_cache_size: usize,
    access_counter: u32,
}

impl TextureManager {
    /// Create an empty texture cache with the given GPU max texture size limit.
    pub fn new(max_texture_size: u32, max_cache_size: usize) -> Self {
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
        decoded: &DecodedImage,
        device: &Device,
        queue: &Queue,
        renderer: &mut Renderer,
    ) -> Result<UploadedTexture> {
        if let Some(existing) = self.textures.get_mut(path) {
            self.access_counter += 1;
            existing.last_used = self.access_counter;
            return Ok(UploadedTexture {
                id: existing.texture_id,
                width: existing.width,
                height: existing.height,
                pixels: existing.pixels.clone(),
            });
        }

        if decoded.pixels.is_empty() || decoded.width == 0 || decoded.height == 0 {
            bail!("cannot upload empty image buffer");
        }

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

        let texture = Texture::new(
            device,
            renderer,
            TextureConfig {
                size: Extent3d {
                    width: decoded.width as u32,
                    height: decoded.height as u32,
                    depth_or_array_layers: 1,
                },
                label: Some("image-viewer texture"),
                format: Some(TextureFormat::Rgba8UnormSrgb),
                mip_level_count: 1,
                sampler_desc: wgpu::SamplerDescriptor {
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    mipmap_filter: wgpu::FilterMode::Nearest,
                    ..wgpu::SamplerDescriptor::default()
                },
                ..TextureConfig::default()
            },
        );
        texture.write(
            queue,
            &decoded.pixels,
            decoded.width as u32,
            decoded.height as u32,
        );
        let texture_id = renderer.textures.insert(texture);

        self.evict_if_full(renderer);

        let pixels = Arc::<[u8]>::from(decoded.pixels.clone());
        self.access_counter += 1;
        let record = TextureRecord {
            texture_id,
            width: decoded.width,
            height: decoded.height,
            pixels: pixels.clone(),
            last_used: self.access_counter,
        };

        self.textures.insert(path.to_path_buf(), record);

        Ok(UploadedTexture {
            id: texture_id,
            width: decoded.width,
            height: decoded.height,
            pixels,
        })
    }

    /// Evict the least-recently-used entry if the cache is at capacity.
    fn evict_if_full(&mut self, renderer: &mut Renderer) {
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
                renderer.textures.remove(record.texture_id);
            }
        }
    }

    /// Free all GPU textures owned by this manager.
    pub fn clear(&mut self, renderer: &mut Renderer) {
        for record in self.textures.drain().map(|(_, record)| record) {
            renderer.textures.remove(record.texture_id);
        }
    }
}
