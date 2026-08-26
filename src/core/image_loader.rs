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

    let box_size = max_size.max(1);
    // Only shrink. `thumbnail()` would also enlarge a small image, so skip it here.
    let thumbnail = if dyn_image.width() > box_size || dyn_image.height() > box_size {
        // Keep the original ratio so the list item does not look stretched.
        dyn_image.thumbnail(box_size, box_size).to_rgba8()
    } else {
        dyn_image.to_rgba8()
    };
    let thumbnail = pad_to_box(thumbnail, box_size);
    let (width, height) = thumbnail.dimensions();
    let pixels = Arc::<[u8]>::from(thumbnail.into_raw());

    Ok(DecodedImage {
        width: width as usize,
        height: height as usize,
        pixels,
    })
}

/// Put a small image in the middle of a transparent square box.
/// Images that already fill the box are returned unchanged (no upscaling).
fn pad_to_box(image: image::RgbaImage, box_size: u32) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    if width >= box_size || height >= box_size {
        return image;
    }

    // Transparent background, so only the original pixels are visible.
    let mut canvas = image::RgbaImage::from_pixel(box_size, box_size, image::Rgba([0, 0, 0, 0]));
    let offset_x = (box_size.saturating_sub(width)) / 2;
    let offset_y = (box_size.saturating_sub(height)) / 2;
    image::imageops::replace(&mut canvas, &image, offset_x as i64, offset_y as i64);
    canvas
}

fn detect_format(path: &Path) -> Option<MediaFormat> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(MediaFormat::from_extension)
}
