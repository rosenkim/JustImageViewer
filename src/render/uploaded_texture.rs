use std::{path::PathBuf, sync::Arc};

use imgui::TextureId;

pub struct UploadedTexture {
    /// File this texture was decoded from.
    pub path: PathBuf,
    pub id: TextureId,
    pub width: usize,
    pub height: usize,
    /// RGBA pixel data (row-major, 4 bytes per pixel).
    pub pixels: Arc<[u8]>,
}
