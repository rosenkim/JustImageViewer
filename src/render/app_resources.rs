use std::path::Path;

use anyhow::{Context, Result};
use imgui_wgpu::Renderer;
use wgpu::{Device, Queue};

use crate::core::image_loader;
use super::texture_atlas_manager::{TextureAtlasManager, AtlasRegion};
use super::imgui_textures::ImguiTextures;

pub struct AppResources {
    pub loading_icon_region: AtlasRegion,
    pub empty_icon_region: AtlasRegion,
    pub open_dir_icon_region: AtlasRegion,
    pub close_dir_icon_region: AtlasRegion,
    texture_atlas_manager: TextureAtlasManager,
    imgui_textures: ImguiTextures,
}

impl AppResources {
    /// Initialize global UI resources shared across screens.
    pub fn new(device: &Device, queue: &Queue, renderer: &mut Renderer) -> Result<Self> {
        let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");

        let mut texture_atlas_manager = TextureAtlasManager::new(2048);
        let mut imgui_textures = ImguiTextures::new();

        // A 4x4 opaque white texture used as a fallback when an icon file is missing.
        const WHITE_TEXTURE_WIDTH: u32 = 4;
        const WHITE_TEXTURE_HEIGHT: u32 = 4;
        const WHITE_PIXEL: [u8; WHITE_TEXTURE_WIDTH as usize * WHITE_TEXTURE_HEIGHT as usize * 4] =
            [255u8; WHITE_TEXTURE_WIDTH as usize * WHITE_TEXTURE_HEIGHT as usize * 4];

        let mut load_icon = |name: &str| -> Result<AtlasRegion> {
            let path = assets_dir.join(name);
            let (width, height, pixels): (u32, u32, Vec<u8>) =
                match image_loader::load_image_rgba(&path) {
                    Ok(decoded) => (decoded.width as u32, decoded.height as u32, decoded.pixels.to_vec()),
                    Err(_) => {
                        log::warn!("Failed to load icon '{}', using white fallback", name);
                        (WHITE_TEXTURE_WIDTH, WHITE_TEXTURE_HEIGHT, WHITE_PIXEL.to_vec())
                    }
                };
            texture_atlas_manager.load_image(
                device,
                queue,
                renderer,
                &mut imgui_textures,
                width,
                height,
                &pixels,
            )
        };

        let loading_icon_region = load_icon("loading_icon.png")?;
        let empty_icon_region = load_icon("empty_image_icon.png")?;
        let open_dir_icon_region = load_icon("open_dir_icon.png")?;
        let close_dir_icon_region = load_icon("close_dir_icon.png")?;

        Ok(Self {
            loading_icon_region,
            empty_icon_region,
            open_dir_icon_region,
            close_dir_icon_region,
            texture_atlas_manager,
            imgui_textures,
        })
    }

    /// Release all global resources allocated by this object.
    pub fn release(&mut self, renderer: &mut Renderer) {
        self.texture_atlas_manager.clear(renderer, &mut self.imgui_textures);
    }
}
