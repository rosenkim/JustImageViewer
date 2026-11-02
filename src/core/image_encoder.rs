use std::io::Cursor;
use std::sync::Arc;

use anyhow::{Context, Result};
use image::{ImageBuffer, ImageFormat, Rgba};

/// RGBA image whose pixels can be shared without copying them.
pub type SharedRgbaImage = ImageBuffer<Rgba<u8>, Arc<[u8]>>;

/// Image bytes together with the MIME type that describes them.
pub struct EncodedImage {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

/// Turns raw pixels into a file format. Callers do not need to know which one.
pub trait ImageEncoder: Send + Sync {
    fn encode(&self, image: &SharedRgbaImage) -> Result<EncodedImage>;
}

/// Encodes with any output format supported by the `image` crate.
pub struct FormatImageEncoder {
    format: ImageFormat,
}

impl FormatImageEncoder {
    pub fn new(format: ImageFormat) -> Self {
        Self { format }
    }
}

impl ImageEncoder for FormatImageEncoder {
    fn encode(&self, image: &SharedRgbaImage) -> Result<EncodedImage> {
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, self.format)
            .with_context(|| format!("failed to encode image as {:?}", self.format))?;
        Ok(EncodedImage {
            bytes: bytes.into_inner(),
            content_type: self.format.to_mime_type(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_matches_the_chosen_format() {
        let pixels: Arc<[u8]> = [10, 20, 30, 255].repeat(6).into();
        let image = SharedRgbaImage::from_raw(2, 3, pixels).unwrap();
        for (format, content_type) in [
            (ImageFormat::Png, "image/png"),
            (ImageFormat::Jpeg, "image/jpeg"),
        ] {
            let encoded = FormatImageEncoder::new(format).encode(&image).unwrap();
            assert_eq!(encoded.content_type, content_type);
            assert_eq!(image::guess_format(&encoded.bytes).unwrap(), format);
        }
    }
}
