use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::core::media::MediaFormat;

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Arc<[u8]>,
}

pub fn load_image_rgba(path: &Path) -> Result<DecodedImage> {
    detect_format(path)
        .with_context(|| format!("unsupported image format for {}", path.display()))?;

    let dyn_image = image::io::Reader::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("failed to guess image format for {}", path.display()))?
        .decode()
        .with_context(|| format!("failed to decode image {}", path.display()))?;

    let rgba_image = dyn_image.to_rgba8();
    let (width, height) = rgba_image.dimensions();
    let pixels = Arc::<[u8]>::from(rgba_image.into_raw());

    Ok(DecodedImage {
        width: width as usize,
        height: height as usize,
        pixels,
    })
}

pub fn load_thumbnail_rgba(path: &Path, max_size: u32) -> Result<DecodedImage> {
    detect_format(path)
        .with_context(|| format!("unsupported image format for {}", path.display()))?;

    let dyn_image = image::io::Reader::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("failed to guess image format for {}", path.display()))?
        .decode()
        .with_context(|| format!("failed to decode image {}", path.display()))?;

    // Keep the original ratio so the list item does not look stretched.
    let thumbnail = dyn_image
        .thumbnail(max_size.max(1), max_size.max(1))
        .to_rgba8();
    let (width, height) = thumbnail.dimensions();
    let pixels = Arc::<[u8]>::from(thumbnail.into_raw());

    Ok(DecodedImage {
        width: width as usize,
        height: height as usize,
        pixels,
    })
}

fn detect_format(path: &Path) -> Option<MediaFormat> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(MediaFormat::from_extension)
}
