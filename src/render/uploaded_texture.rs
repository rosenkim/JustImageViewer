use std::sync::Arc;

use imgui::TextureId;

pub struct UploadedTexture {
    pub id: TextureId,
    pub width: usize,
    pub height: usize,
    /// RGBA pixel data (row-major, 4 bytes per pixel).
    pub pixels: Arc<[u8]>,
}

/// A texture that can be displayed in the view panel.
pub enum ViewTexture<'a> {
    Image(&'a UploadedTexture),
    Model(&'a super::model_surface::ModelSurface),
}

impl ViewTexture<'_> {
    pub fn texture_id(&self) -> TextureId {
        match self {
            Self::Image(texture) => texture.id,
            Self::Model(surface) => surface.texture_id,
        }
    }
}
