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

struct TextureRecord {
    gl_id: u32,
    width: usize,
    height: usize,
}

pub struct TextureManager {
    textures: HashMap<PathBuf, TextureRecord>,
}

impl TextureManager {
    /// Create an empty texture cache.
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    /// Return existing texture for a path, or upload a new OpenGL texture.
    pub fn get_or_upload(&mut self, path: &Path, decoded: &DecodedImage) -> Result<UploadedTexture> {
        if let Some(existing) = self.textures.get(path) {
            return Ok(UploadedTexture {
                id: TextureId::new(existing.gl_id as usize),
                width: existing.width,
                height: existing.height,
            });
        }

        if decoded.pixels.is_empty() || decoded.width == 0 || decoded.height == 0 {
            bail!("cannot upload empty image buffer");
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

        let record = TextureRecord {
            gl_id,
            width: decoded.width,
            height: decoded.height,
        };

        self.textures.insert(path.to_path_buf(), record);

        Ok(UploadedTexture {
            id: TextureId::new(gl_id as usize),
            width: decoded.width,
            height: decoded.height,
        })
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
