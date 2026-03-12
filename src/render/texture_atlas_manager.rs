use std::collections::HashMap;
use guillotiere::{AllocId, AtlasAllocator, Size};
use wgpu::{Device, Queue};
use imgui::TextureId;
use imgui_wgpu::Renderer;
use anyhow::Result;
use super::imgui_textures::ImguiTextures;

// ------------------------------------------------------------------
// A region inside an atlas. This is what you get back when you load
// an image. Keep hold of it so you can reference or delete the image
// later.
// ------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    /// A unique number that identifies this image across ALL atlases.
    pub id: u64,
    /// Which atlas texture this image lives in (imgui TextureId).
    pub texture_id: TextureId,
    /// UV coordinates: (u_min, v_min, u_max, v_max), all in 0.0..1.0 range.
    pub uvs: [f32; 4],
    /// The original image size in pixels (width, height).
    pub image_size: (u32, u32),
}

// ------------------------------------------------------------------
// One atlas texture. It owns a GPU texture and a rectangle packer
// that decides where images go inside it.
// ------------------------------------------------------------------
struct AtlasTexture {
    /// The imgui texture ID.
    texture_id: TextureId,
    /// The rectangle packer from guillotiere.
    allocator: AtlasAllocator,
    /// The full size of this atlas in pixels.
    size: u32,
}

impl AtlasTexture {
    /// Make a new, empty atlas texture on the GPU.
    fn new(
        device: &Device,
        queue: &Queue,
        renderer: &mut Renderer,
        imgui_textures: &mut ImguiTextures,
        size: u32,
    ) -> Result<Self> {
        let texture_id = imgui_textures.create_atlas_texture(device, queue, renderer, size)?;
        let allocator = AtlasAllocator::new(Size::new(size as i32, size as i32));

        Ok(Self {
            texture_id,
            allocator,
            size,
        })
    }

    /// Try to fit a rectangle of the given size into this atlas.
    /// Returns the allocation ID and the pixel position if it fits.
    fn try_allocate(&mut self, width: u32, height: u32) -> Option<(AllocId, (u32, u32))> {
        let alloc = self
            .allocator
            .allocate(Size::new(width as i32, height as i32))?;

        let origin = alloc.rectangle.min;
        Some((alloc.id, (origin.x as u32, origin.y as u32)))
    }

    /// Free a previously allocated rectangle.
    fn deallocate(&mut self, alloc_id: AllocId) {
        self.allocator.deallocate(alloc_id);
    }

    /// Returns true when nothing is allocated in this atlas any more.
    fn is_empty(&self) -> bool {
        self.allocator.is_empty()
    }
}

// ------------------------------------------------------------------
// The manager. It keeps a list of atlas textures and hands out unique
// IDs for every image you load.
// ------------------------------------------------------------------
pub struct TextureAtlasManager {
    /// The pixel size used for every new atlas (e.g. 2048 means 2048x2048).
    atlas_size: u32,
    /// All the atlas textures we currently have.
    atlases: Vec<AtlasTexture>,
    /// Counter that goes up by one every time we load an image.
    /// This makes every image ID unique.
    next_id: u64,
    /// Reverse lookup: image ID -> (atlas index, allocation ID).
    /// We need this so we can find and remove an image by its ID.
    id_to_location: HashMap<u64, (usize, AllocId)>,
}

impl TextureAtlasManager {
    /// Create a new manager. `atlas_size` is the width and height in
    /// pixels for every atlas texture that gets created (e.g. 2048).
    pub fn new(atlas_size: u32) -> Self {
        Self {
            atlas_size,
            atlases: Vec::new(),
            next_id: 1,
            id_to_location: HashMap::new(),
        }
    }

    pub fn clear(&mut self, renderer: &mut Renderer, imgui_textures: &mut ImguiTextures) {
        for atlas in self.atlases.drain(..) {
            imgui_textures.remove(renderer, atlas.texture_id);
        }
        self.id_to_location.clear();
    }

    /// Load RGBA pixel data into the atlas and get back an AtlasRegion.
    ///
    /// * `device` / `queue` – your wgpu device and queue.
    /// * `width`, `height` – image dimensions in pixels.
    /// * `rgba_data` – raw pixels, 4 bytes per pixel, row-major.
    ///
    /// If no existing atlas has room, a brand-new one is created
    /// automatically.
    pub fn load_image(
        &mut self,
        device: &Device,
        queue: &Queue,
        renderer: &mut Renderer,
        imgui_textures: &mut ImguiTextures,
        width: u32,
        height: u32,
        rgba_data: &[u8],
    ) -> Result<AtlasRegion> {
        // Sanity check: the image must not be bigger than the atlas itself.
        assert!(
            width <= self.atlas_size && height <= self.atlas_size,
            "Image ({}x{}) is larger than the atlas size ({})",
            width,
            height,
            self.atlas_size,
        );

        // Try every atlas we already have.
        for (atlas_idx, atlas) in self.atlases.iter_mut().enumerate() {
            if let Some((alloc_id, (x, y))) = atlas.try_allocate(width, height) {
                // Found space – upload pixels and return the region.
                let id = self.next_id;
                self.next_id += 1;

                imgui_textures.update_sub_region(
                    queue,
                    renderer,
                    atlas.texture_id,
                    x,
                    y,
                    width,
                    height,
                    rgba_data,
                )?;
                self.id_to_location.insert(id, (atlas_idx, alloc_id));

                return Ok(make_region(id, atlas.texture_id, x, y, width, height, atlas.size));
            }
        }

        // No atlas had room – make a new one.
        let atlas_idx = self.atlases.len();
        let mut atlas = AtlasTexture::new(device, queue, renderer, imgui_textures, self.atlas_size)?;

        let (alloc_id, (x, y)) = atlas
            .try_allocate(width, height)
            .expect("Fresh atlas must have room for the image");

        let id = self.next_id;
        self.next_id += 1;

        imgui_textures.update_sub_region(
            queue,
            renderer,
            atlas.texture_id,
            x,
            y,
            width,
            height,
            rgba_data,
        )?;

        let texture_id = atlas.texture_id;
        self.id_to_location.insert(id, (atlas_idx, alloc_id));
        self.atlases.push(atlas);

        Ok(make_region(id, texture_id, x, y, width, height, self.atlas_size))
    }

    /// Remove an image by its unique ID.
    ///
    /// If this was the last image in its atlas, the atlas is destroyed
    /// to free GPU memory. Returns `true` if the ID was found and
    /// removed, `false` if it didn't exist.
    pub fn remove_image(
        &mut self,
        renderer: &mut Renderer,
        imgui_textures: &mut ImguiTextures,
        id: u64,
    ) -> bool {
        let Some((atlas_idx, alloc_id)) = self.id_to_location.remove(&id) else {
            return false;
        };

        // Free the rectangle inside the packer.
        let atlas = match self.atlases.get_mut(atlas_idx) {
            Some(atlas) => atlas,
            None => return false,
        };
        atlas.deallocate(alloc_id);

        // If nobody is using this atlas any more, drop it entirely.
        if atlas.is_empty() {
            let removed_atlas = self.atlases.remove(atlas_idx);
            imgui_textures.remove(renderer, removed_atlas.texture_id);

            // Update indices in id_to_location for all images in atlases after the removed one.
            for (_, (idx, _)) in self.id_to_location.iter_mut() {
                if *idx > atlas_idx {
                    *idx -= 1;
                }
            }
        }

        true
    }

    /// How many atlas textures are currently alive.
    pub fn atlas_count(&self) -> usize {
        self.atlases.len()
    }
}

// ------------------------------------------------------------------
// Helper: build an AtlasRegion from raw numbers.
// ------------------------------------------------------------------
fn make_region(
    id: u64,
    texture_id: TextureId,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    atlas_size: u32,
) -> AtlasRegion {
    let size_f = atlas_size as f32;

    AtlasRegion {
        id,
        texture_id,
        uvs: [
            x as f32 / size_f,
            y as f32 / size_f,
            (x + width) as f32 / size_f,
            (y + height) as f32 / size_f,
        ],
        image_size: (width, height),
    }
}