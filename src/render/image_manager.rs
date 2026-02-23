use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use imgui::TextureId;
use imgui_wgpu::{Renderer, Texture, TextureConfig};
use wgpu::{Device, Extent3d, Queue, TextureFormat};

use crate::core::image_loader::DecodedImage;

/// A reference to the single display texture together with the dimensions of
/// the image currently written into it. The Viewer widget uses this to render
/// the image at the correct aspect ratio.
#[derive(Clone, Copy)]
pub struct DisplayImage {
    pub id: TextureId,
    /// Width of the image that was copied into the texture (≤ texture width).
    pub width: usize,
    /// Height of the image that was copied into the texture (≤ texture height).
    pub height: usize,
    /// Width of the backing GPU texture (used for UV scaling).
    pub tex_width: usize,
    /// Height of the backing GPU texture (used for UV scaling).
    pub tex_height: usize,
}

pub struct ImageManager {
    /// Decoded bitmap cache keyed by file path.
    cache: HashMap<PathBuf, DecodedImage>,
    max_cache_size: usize,
    /// The single, pre-allocated GPU texture (max_size × max_size).
    texture_id: TextureId,
    /// Maximum dimension that the GPU supports.
    max_texture_size: u32,
}

impl ImageManager {
    /// Allocate the single display texture and return an `ImageManager`.
    ///
    /// The texture is created at `max_texture_size × max_texture_size` so that
    /// any image — regardless of resolution — can be copied into it without
    /// re-allocating GPU memory.
    pub fn init(
        max_texture_size: u32,
        max_cache_size: usize,
        device: &Device,
        queue: &Queue,
        renderer: &mut Renderer,
    ) -> Self {
        let texture_size = max_texture_size.min(16384);
        let texture = Texture::new(
            device,
            renderer,
            TextureConfig {
                size: Extent3d {
                    width: texture_size,
                    height: texture_size,
                    depth_or_array_layers: 1,
                },
                label: Some("image-manager display texture"),
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

        // Write transparent black so the texture is in a defined state before
        // any image is loaded.
        let pixel_count = (texture_size as usize) * (texture_size as usize);
        let clear_data = vec![0u8; pixel_count * 4];
        texture.write(queue, &clear_data, texture_size, texture_size);

        let texture_id = renderer.textures.insert(texture);

        Self {
            cache: HashMap::new(),
            max_cache_size,
            texture_id,
            max_texture_size: texture_size,
        }
    }

    /// Load an image into the manager.
    ///
    /// If the decoded bitmap is already in the cache the cached copy is used;
    /// otherwise `decoded` is inserted.  The image pixels are then written into
    /// the pre-allocated display texture and a [`DisplayImage`] describing the
    /// texture id and image dimensions is returned.
    pub fn load(
        &mut self,
        path: &Path,
        decoded: DecodedImage,
        queue: &Queue,
        renderer: &mut Renderer,
    ) -> Result<DisplayImage> {
        if decoded.pixels.is_empty() || decoded.width == 0 || decoded.height == 0 {
            bail!("cannot load empty image buffer");
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

        // Insert into cache (evicting LRU not implemented here — we keep it
        // simple: drop oldest when at capacity).
        if !self.cache.contains_key(path) {
            self.evict_if_full();
            self.cache.insert(path.to_path_buf(), decoded.clone());
        }

        let img = self.cache.get(path).expect("just inserted");

        // Copy into the pre-allocated texture.
        let texture = renderer
            .textures
            .get(self.texture_id)
            .expect("display texture must exist");

        texture.write(queue, &img.pixels, img.width as u32, img.height as u32);

        Ok(DisplayImage {
            id: self.texture_id,
            width: img.width,
            height: img.height,
            tex_width: self.max_texture_size as usize,
            tex_height: self.max_texture_size as usize,
        })
    }

    /// Evict a cached bitmap entry when the cache is at capacity.
    ///
    /// (Simple FIFO — we just remove the first entry we find. A full LRU can
    /// be added later if needed.)
    fn evict_if_full(&mut self) {
        if self.cache.len() < self.max_cache_size {
            return;
        }
        if let Some(key) = self.cache.keys().next().cloned() {
            log::debug!("Evicting cached bitmap: {}", key.display());
            self.cache.remove(&key);
        }
    }

    /// Remove the GPU texture.  Call this once before the renderer is dropped.
    pub fn destroy(&mut self, renderer: &mut Renderer) {
        renderer.textures.remove(self.texture_id);
        self.cache.clear();
    }
}
