use std::sync::{Arc, mpsc};

use anyhow::{Context, Result, ensure};

/// A tightly packed, straight-alpha sRGB image returned by GPU readback.
#[derive(Debug, Clone)]
pub struct RenderedImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Arc<[u8]>,
}

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Owns a square render target and a reusable CPU readback buffer.
pub struct OffscreenTarget {
    size: u32,
    color: wgpu::Texture,
    pub(crate) color_view: wgpu::TextureView,
    pub(crate) depth_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row_bytes: u32,
}

impl OffscreenTarget {
    /// `size` is the side length in pixels; it does not need to be a power of two.
    pub fn new(device: &wgpu::Device, size: u32) -> Result<Self> {
        ensure!(size > 0, "offscreen size must be greater than zero");
        ensure!(
            size <= device.limits().max_texture_dimension_2d,
            "offscreen size exceeds the device limit"
        );
        let row_bytes = size.checked_mul(4).context("offscreen row size overflow")?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = row_bytes.div_ceil(alignment) * alignment;
        let buffer_size = u64::from(padded_row_bytes) * u64::from(size);
        ensure!(
            buffer_size <= device.limits().max_buffer_size,
            "offscreen readback exceeds the device buffer limit"
        );

        let descriptor = wgpu::TextureDescriptor {
            label: Some("model offscreen color"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let color = device.create_texture(&descriptor);
        let color_view = color.create_view(&Default::default());
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model offscreen depth"),
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            ..descriptor
        });
        let depth_view = depth.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model offscreen readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Self {
            size,
            color,
            color_view,
            depth_view,
            readback,
            padded_row_bytes,
        })
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// The color texture can be sampled by a UI after the render pass is submitted.
    pub fn color_view(&self) -> &wgpu::TextureView {
        &self.color_view
    }

    /// Submit after the render pass, then wait for the pixels on the calling thread.
    pub(crate) fn read_pixels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
    ) -> Result<RenderedImage> {
        encoder.copy_texture_to_buffer(
            self.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row_bytes),
                    rows_per_image: Some(self.size),
                },
            },
            self.color.size(),
        );
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::sync_channel(1);
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        if let Err(err) = device.poll(wgpu::PollType::wait_for(submission)) {
            self.readback.unmap();
            return Err(err).context("failed to wait for offscreen rendering");
        }
        rx.recv()
            .context("readback callback was dropped")?
            .context("failed to map offscreen pixels")?;
        let mapped = self.readback.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity(self.size as usize * self.size as usize * 4);
        // GPU copy rows have padding; the returned image is tightly packed RGBA.
        for row in mapped.chunks_exact(self.padded_row_bytes as usize) {
            pixels.extend_from_slice(&row[..self.size as usize * 4]);
        }
        drop(mapped);
        self.readback.unmap();
        // Blending stores premultiplied linear color. Return straight-alpha sRGB.
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = f32::from(pixel[3]) / 255.0;
            if alpha > 0.0 && alpha < 1.0 {
                for channel in &mut pixel[..3] {
                    let srgb = f32::from(*channel) / 255.0;
                    let linear = if srgb <= 0.04045 {
                        srgb / 12.92
                    } else {
                        ((srgb + 0.055) / 1.055).powf(2.4)
                    };
                    let straight = (linear / alpha).min(1.0);
                    let srgb = if straight <= 0.0031308 {
                        straight * 12.92
                    } else {
                        1.055 * straight.powf(1.0 / 2.4) - 0.055
                    };
                    *channel = (srgb * 255.0).round() as u8;
                }
            }
        }
        Ok(RenderedImage {
            width: self.size as usize,
            height: self.size as usize,
            pixels: Arc::from(pixels),
        })
    }
}
