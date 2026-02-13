use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use imgui::TextureId;

use crate::core::image_loader::DecodedImage;

#[derive(Clone, Copy)]
pub struct UploadedTexture {
    pub id: TextureId,
    pub width: usize,
    pub height: usize,
}

const DEFAULT_MAX_CACHE_SIZE: usize = 20;

struct TextureRecord {
    gl_id: u32,
    width: usize,
    height: usize,
    last_used: u32,
}

pub struct TextureManager {
    textures: HashMap<PathBuf, TextureRecord>,
    max_texture_size: i32,
    max_cache_size: usize,
    access_counter: u32,
}

impl TextureManager {
    /// Create an empty texture cache with the given OpenGL max texture size limit.
    pub fn new(max_texture_size: i32) -> Self {
        Self {
            textures: HashMap::new(),
            max_texture_size,
            max_cache_size: DEFAULT_MAX_CACHE_SIZE,
            access_counter: 0,
        }
    }

    /// Return existing texture for a path, or upload a new OpenGL texture.
    pub fn get_or_upload(&mut self, path: &Path, decoded: &DecodedImage) -> Result<UploadedTexture> {
        if let Some(existing) = self.textures.get_mut(path) {
            self.access_counter += 1;
            existing.last_used = self.access_counter;
            return Ok(UploadedTexture {
                id: TextureId::new(existing.gl_id as usize),
                width: existing.width,
                height: existing.height,
            });
        }

        if decoded.pixels.is_empty() || decoded.width == 0 || decoded.height == 0 {
            bail!("cannot upload empty image buffer");
        }

        if decoded.width as i32 > self.max_texture_size || decoded.height as i32 > self.max_texture_size {
            bail!(
                "image size {}x{} exceeds OpenGL max texture size {}",
                decoded.width,
                decoded.height,
                self.max_texture_size
            );
        }

        let mut gl_id: u32 = 0;
        unsafe {
            gl::GenTextures(1, &mut gl_id);
            if gl_id == 0 {
                bail!("failed to allocate OpenGL texture");
            }

            gl::BindTexture(gl::TEXTURE_2D, gl_id);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                decoded.width as i32,
                decoded.height as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                decoded.pixels.as_ptr().cast(),
            );
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        self.evict_if_full();

        self.access_counter += 1;
        let record = TextureRecord {
            gl_id,
            width: decoded.width,
            height: decoded.height,
            last_used: self.access_counter,
        };

        self.textures.insert(path.to_path_buf(), record);

        Ok(UploadedTexture {
            id: TextureId::new(gl_id as usize),
            width: decoded.width,
            height: decoded.height,
        })
    }

    /// Evict the least-recently-used entry if the cache is at capacity.
    fn evict_if_full(&mut self) {
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
                unsafe {
                    gl::DeleteTextures(1, &record.gl_id);
                }
            }
        }
    }

    /// Free all OpenGL textures owned by this manager.
    pub fn clear(&mut self) {
        for record in self.textures.drain().map(|(_, record)| record) {
            unsafe {
                gl::DeleteTextures(1, &record.gl_id);
            }
        }
    }
}

impl Drop for TextureManager {
    fn drop(&mut self) {
        self.clear();
    }
}
