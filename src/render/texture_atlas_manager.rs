use std::collections::HashMap;
use guillotiere::{AllocId, AtlasAllocator, Size};
use wgpu::{Device, Extent3d, Queue, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

// ------------------------------------------------------------------
// A region inside an atlas. This is what you get back when you load
// an image. Keep hold of it so you can reference or delete the image
// later.
// ------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    /// A unique number that identifies this image across ALL atlases.
    pub id: u64,
    /// Which atlas texture this image lives in (index into the manager's list).
    pub texture_index: u64,
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
    /// The GPU texture we upload pixel data into.
    texture: Texture,
    /// The rectangle packer from guillotiere.
    allocator: AtlasAllocator,
    /// The full size of this atlas in pixels.
    size: u32,
}

impl AtlasTexture {
    /// Make a new, empty atlas texture on the GPU.
    fn new(device: &Device, size: u32, label: &str) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let allocator = AtlasAllocator::new(Size::new(size as i32, size as i32));

        Self {
            texture,
            allocator,
            size,
        }
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
    atlases: HashMap<u64,AtlasTexture>,
    /// The next texture ID to use for a new atlas.
    next_texture_id: u64,
    /// Counter that goes up by one every time we load an image.
    /// This makes every image ID unique.
    next_id: u64,
    /// Reverse lookup: image ID -> (atlas index, allocation ID).
    /// We need this so we can find and remove an image by its ID.
    id_to_location: HashMap<u64, (u64, AllocId)>,
}

impl TextureAtlasManager {
    /// Create a new manager. `atlas_size` is the width and height in
    /// pixels for every atlas texture that gets created (e.g. 2048).
    pub fn new(atlas_size: u32) -> Self {
        Self {
            atlas_size,
            atlases: HashMap::new(),
            next_texture_id: 1,
            next_id: 1, // start at 1 so 0 can mean "no image"
            id_to_location: HashMap::new(),
        }
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
        width: u32,
        height: u32,
        rgba_data: &[u8],
    ) -> AtlasRegion {
        // Sanity check: the image must not be bigger than the atlas itself.
        assert!(
            width <= self.atlas_size && height <= self.atlas_size,
            "Image ({}x{}) is larger than the atlas size ({})",
            width,
            height,
            self.atlas_size,
        );

        // Try every atlas we already have.
        for (&tex_id, atlas) in self.atlases.iter_mut() {
            if let Some((alloc_id, (x, y))) = atlas.try_allocate(width, height) {
                // Found space – upload pixels and return the region.
                let target_tex_id: u64 = tex_id;
                let id = self.next_id;
                self.next_id += 1;

                upload_to_texture(queue, &atlas.texture, x, y, width, height, rgba_data);
                self.id_to_location.insert(id, (target_tex_id, alloc_id));

                return make_region(id, target_tex_id, x, y, width, height, atlas.size);
            }
        }

        // No atlas had room – make a new one.
        let atlas_idx = self.atlases.len();
        let label = format!("atlas_{}", atlas_idx);
        let mut atlas = AtlasTexture::new(device, self.atlas_size, &label);

        let (alloc_id, (x, y)) = atlas
            .try_allocate(width, height)
            .expect("Fresh atlas must have room for the image");

        let id = self.next_id;
        self.next_id += 1;
        let tex_id = self.next_texture_id;
        self.next_texture_id += 1;

        upload_to_texture(queue, &atlas.texture, x, y, width, height, rgba_data);

        self.id_to_location.insert(id, (tex_id, alloc_id));
        self.atlases.insert(tex_id, atlas);

        make_region(id, tex_id, x, y, width, height, self.atlas_size)
    }

    /// Remove an image by its unique ID.
    ///
    /// If this was the last image in its atlas, the atlas is destroyed
    /// to free GPU memory. Returns `true` if the ID was found and
    /// removed, `false` if it didn't exist.
    pub fn remove_image(&mut self, id: u64) -> bool {
        let Some((tex_id, alloc_id)) = self.id_to_location.remove(&id) else {
            return false;
        };

        // Free the rectangle inside the packer.
        let atlas = match self.atlases.get_mut(&tex_id) {
            Some(atlas) => atlas,
            None => return false,
        };
        atlas.deallocate(alloc_id);
        // If nobody is using this atlas any more, drop it entirely.
        if atlas.is_empty() {
            self.atlases.remove(&tex_id);
        }

        true
    }

    /// Get a reference to the raw wgpu Texture for a given atlas index.
    /// Useful when you need to create a TextureView for your shader.
    pub fn get_texture(&self, texture_index: u64) -> Option<&Texture> {
        self.atlases.get(&texture_index).map(|a| &a.texture)
    }

    /// How many atlas textures are currently alive.
    pub fn atlas_count(&self) -> usize {
        self.atlases.len()
    }

}

// ------------------------------------------------------------------
// Helper: upload raw RGBA bytes into a sub-region of a GPU texture.
// ------------------------------------------------------------------
fn upload_to_texture(
    queue: &Queue,
    texture: &Texture,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    rgba_data: &[u8],
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x, y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        rgba_data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            // 4 bytes per pixel (RGBA).
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

// ------------------------------------------------------------------
// Helper: build an AtlasRegion from raw numbers.
// ------------------------------------------------------------------
fn make_region(
    id: u64,
    texture_index: u64,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    atlas_size: u32,
) -> AtlasRegion {
    let size_f = atlas_size as f32;

    AtlasRegion {
        id,
        texture_index,
        uvs: [
            x as f32 / size_f,            // u_min (left)
            y as f32 / size_f,            // v_min (top)
            (x + width) as f32 / size_f,  // u_max (right)
            (y + height) as f32 / size_f, // v_max (bottom)
        ],
        image_size: (width, height),
    }
}