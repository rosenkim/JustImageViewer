use std::collections::HashSet;

use anyhow::{Result, bail};
use imgui::TextureId;
use imgui_wgpu::{Renderer, Texture, TextureConfig};
use wgpu::{Device, Extent3d, Queue, TextureFormat};

pub struct ImguiTextures {
    texture_ids: HashSet<TextureId>,
}

impl ImguiTextures {
    pub fn new() -> Self {
        Self {
            texture_ids: HashSet::new(),
        }
    }

    pub fn create_from_rgba_data(
        &mut self,
        device: &Device,
        queue: &Queue,
        renderer: &mut Renderer,
        width: u32,
        height: u32,
        data: &[u8],
        linear_filter: bool
    ) -> Result<TextureId> {
        let expected_len = rgba_len(width, height)?;
        if data.len() != expected_len {
            bail!(
                "invalid RGBA data length: expected {}, got {}",
                expected_len,
                data.len()
            );
        }

        let sampler_desc: wgpu::SamplerDescriptor = if linear_filter {
            wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..wgpu::SamplerDescriptor::default()
            }
        } else {
            wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..wgpu::SamplerDescriptor::default()
            }
        };

        let texture = Texture::new(
            device,
            renderer,
            TextureConfig {
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                label: Some("imgui texture"),
                format: Some(TextureFormat::Rgba8UnormSrgb),
                mip_level_count: 1,
                sampler_desc,
                ..TextureConfig::default()
            },
        );

        texture.write(queue, data, width, height);
        let texture_id = renderer.textures.insert(texture);
        self.texture_ids.insert(texture_id);
        Ok(texture_id)
    }

    pub fn create_filled_color(
        &mut self,
        device: &Device,
        queue: &Queue,
        renderer: &mut Renderer,
        width: u32,
        height: u32,
        rgba: [u8; 4],
        linear_filter: bool,
    ) -> Result<TextureId> {
        let pixel_count = pixel_count(width, height)?;
        let mut data = vec![0; pixel_count * 4];

        for chunk in data.chunks_exact_mut(4) {
            chunk.copy_from_slice(&rgba);
        }

        self.create_from_rgba_data(device, queue, renderer, width, height, &data, linear_filter)
    }

    pub fn remove(
        &mut self,
        renderer: &mut Renderer,
        texture_id: TextureId,
    ) -> bool {
        let removed = self.texture_ids.remove(&texture_id);
        if removed {
            renderer.textures.remove(texture_id);
        }
        removed
    }

    pub fn texture_count(&self) -> usize {
        self.texture_ids.len()
    }

    pub fn clear(&mut self, renderer: &mut Renderer) {
        for texture_id in self.texture_ids.drain() {
            renderer.textures.remove(texture_id);
        }
    }

    pub fn create_atlas_texture(
        &mut self,
        device: &Device,
        queue: &Queue,
        renderer: &mut Renderer,
        size: u32,
    ) -> Result<TextureId> {
        let pixel_count = pixel_count(size, size)?;
        let data = vec![0u8; pixel_count * 4];
        self.create_from_rgba_data(device, queue, renderer, size, size, &data, false)
    }

    pub fn update_sub_region(
        &self,
        queue: &Queue,
        renderer: &Renderer,
        texture_id: TextureId,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba_data: &[u8],
    ) -> Result<()> {
        let expected_len = rgba_len(width, height)?;
        if rgba_data.len() != expected_len {
            bail!(
                "invalid RGBA data length: expected {}, got {}",
                expected_len,
                rgba_data.len()
            );
        }

        let texture = renderer.textures.get(texture_id)
            .ok_or_else(|| anyhow::anyhow!("texture not found"))?;

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        Ok(())
    }
}

fn pixel_count(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        bail!("width and height must be greater than zero");
    }

    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow::anyhow!("image size overflow: {}x{}", width, height))?;
    Ok(pixels)
}

fn rgba_len(width: u32, height: u32) -> Result<usize> {
    let pixels = pixel_count(width, height)?;
    let rgba_len = pixels
        .checked_mul(4)
        .ok_or_else(|| anyhow::anyhow!("RGBA length overflow: {}x{}", width, height))?;
    Ok(rgba_len)
}
