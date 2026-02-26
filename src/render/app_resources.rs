use std::path::Path;

use anyhow::{Context, Result, bail};
use imgui::TextureId;
use imgui_wgpu::{Renderer, Texture, TextureConfig};
use wgpu::{Device, Extent3d, Queue, TextureFormat};

use crate::core::image_loader;

pub struct AppResources {
    pub empty_icon_texture_id: TextureId,
    texture_ids: Vec<TextureId>,
}

impl AppResources {
    /// Initialize global UI resources shared across screens.
    pub fn new(device: &Device, queue: &Queue, renderer: &mut Renderer) -> Result<Self> {
        let empty_icon_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("empty_image_icon.png");

        let decoded = image_loader::load_image_rgba(&empty_icon_path).with_context(|| {
            format!(
                "failed to load empty image icon from {}",
                empty_icon_path.display()
            )
        })?;

        if decoded.width as u32 > device.limits().max_texture_dimension_2d
            || decoded.height as u32 > device.limits().max_texture_dimension_2d
        {
            bail!(
                "empty image icon {}x{} exceeds GPU max texture size {}",
                decoded.width,
                decoded.height,
                device.limits().max_texture_dimension_2d
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
                label: Some("image-viewer empty icon texture"),
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

        let empty_icon_texture_id = renderer.textures.insert(texture);

        Ok(Self {
            empty_icon_texture_id,
            texture_ids: vec![empty_icon_texture_id],
        })
    }

    /// Release all global resources allocated by this object.
    pub fn release(&mut self, renderer: &mut Renderer) {
        for texture_id in self.texture_ids.drain(..) {
            renderer.textures.remove(texture_id);
        }
    }
}
