use std::path::Path;

use anyhow::{Context, Result};
use imgui::TextureId;
use imgui_wgpu::Renderer;
use wgpu::{Device, Queue};

use crate::core::image_loader;
use super::texture_atlas_manager::{TextureAtlasManager, AtlasRegion};
use super::imgui_textures::ImguiTextures;

pub struct AppResources {
    pub empty_icon_texture_id: TextureId,
    pub empty_icon_region: AtlasRegion,
    texture_atlas_manager: TextureAtlasManager,
    imgui_textures: ImguiTextures,
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

        let mut texture_atlas_manager = TextureAtlasManager::new(2048);
        let mut imgui_textures = ImguiTextures::new();

        let empty_icon_region = texture_atlas_manager.load_image(
            device,
            queue,
            renderer,
            &mut imgui_textures,
            decoded.width as u32,
            decoded.height as u32,
            &decoded.pixels,
        )?;

        let empty_icon_texture_id = empty_icon_region.texture_id;

        Ok(Self {
            empty_icon_texture_id,
            empty_icon_region,
            texture_atlas_manager,
            imgui_textures,
        })
    }

    /// Release all global resources allocated by this object.
    pub fn release(&mut self, renderer: &mut Renderer) {
        self.texture_atlas_manager.clear(renderer, &mut self.imgui_textures);
    }
}
