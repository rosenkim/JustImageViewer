use std::sync::Arc;

use anyhow::{Context, Result, ensure};

use crate::core::image_encoder::SharedRgbaImage;
use crate::core::view_capture::{ViewCaptureError, ViewCaptureReply, respond_all};

/// Copies a whole RGBA texture to memory that the CPU can read.
pub struct TextureReadback {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row_bytes: u32,
}

impl TextureReadback {
    /// Record the copy. Call this after the commands that draw the texture.
    pub fn new(
        device: &wgpu::Device,
        texture: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<Self> {
        ensure!(
            matches!(
                texture.format(),
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
            ),
            "unsupported texture format {:?}",
            texture.format()
        );
        let size = texture.size();
        let padded_row_bytes = padded_row_bytes(size.width);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("texture readback"),
            size: u64::from(padded_row_bytes) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(size.height),
                },
            },
            size,
        );
        Ok(Self {
            buffer,
            width: size.width,
            height: size.height,
            padded_row_bytes,
        })
    }

    /// Wait for the GPU on a worker thread so the UI does not stop, then reply.
    pub fn finish(
        self,
        device: wgpu::Device,
        submission: wgpu::SubmissionIndex,
        replies: Vec<ViewCaptureReply>,
    ) {
        tokio::task::spawn_blocking(move || {
            let result = self
                .read_image(&device, submission)
                .map_err(|err| ViewCaptureError::Failed(format!("{err:#}")));
            respond_all(replies, result);
        });
    }

    fn read_image(
        &self,
        device: &wgpu::Device,
        submission: wgpu::SubmissionIndex,
    ) -> Result<SharedRgbaImage> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let slice = self.buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_for(submission))
            .context("failed to wait for the GPU")?;
        rx.recv()
            .context("GPU readback was dropped")?
            .context("failed to map GPU readback")?;

        let pixels = remove_row_padding(
            &slice.get_mapped_range(),
            self.width,
            self.padded_row_bytes,
        );
        self.buffer.unmap();
        SharedRgbaImage::from_raw(self.width, self.height, Arc::from(pixels))
            .context("read pixels do not match the texture size")
    }
}

/// GPU buffer copies need each row to start at an aligned offset.
fn padded_row_bytes(width: u32) -> u32 {
    (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
}

fn remove_row_padding(mapped: &[u8], width: u32, padded_row_bytes: u32) -> Vec<u8> {
    let row_bytes = width as usize * 4;
    mapped
        .chunks_exact(padded_row_bytes as usize)
        .flat_map(|row| row[..row_bytes].iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_padding_is_removed() {
        let padded = padded_row_bytes(1);
        assert_eq!(padded, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let mut mapped = vec![0xEE; padded as usize * 2];
        mapped[..4].copy_from_slice(&[1, 2, 3, 4]);
        mapped[padded as usize..padded as usize + 4].copy_from_slice(&[5, 6, 7, 8]);
        assert_eq!(remove_row_padding(&mapped, 1, padded), [1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
