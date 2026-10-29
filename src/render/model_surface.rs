use std::sync::Arc;

use anyhow::Result;
use imgui::TextureId;
use imgui_wgpu::{RawTextureConfig, Texture};

use super::view_gpu::ViewGpuContext;

/// ImGui samples the model target directly. No extra color texture or pass is needed.
pub struct ModelSurface {
    pub target: render3d::OffscreenTarget,
    pub texture_id: TextureId,
}

impl ModelSurface {
    pub fn new(ctx: &mut ViewGpuContext<'_>, size: u32) -> Result<Self> {
        let target = render3d::OffscreenTarget::new(ctx.device, size)?;
        let texture = Texture::from_raw_parts(
            ctx.device,
            ctx.renderer,
            Arc::new(target.color_texture().clone()),
            Arc::new(target.color_view().clone()),
            None,
            Some(&RawTextureConfig {
                label: Some("model UI color"),
                sampler_desc: wgpu::SamplerDescriptor {
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                },
            }),
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        let texture_id = ctx.renderer.textures.insert(texture);
        Ok(Self { target, texture_id })
    }

    pub fn release(self, ctx: &mut ViewGpuContext<'_>) {
        ctx.renderer.textures.remove(self.texture_id);
    }
}

fn srgb_to_linear(value: f32) -> f64 {
    let value = f64::from(value.clamp(0.0, 1.0));
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// UI settings contain sRGB colors; wgpu clear colors are linear.
pub fn opaque_background(rgb: [f32; 3]) -> wgpu::Color {
    wgpu::Color {
        r: srgb_to_linear(rgb[0]),
        g: srgb_to_linear(rgb[1]),
        b: srgb_to_linear(rgb[2]),
        a: 1.0,
    }
}

/// OffscreenTarget also allocates a readback buffer, even when the UI never maps it.
pub fn target_size(requested: u32, limits: &wgpu::Limits) -> u32 {
    let mut size = requested
        .clamp(1, 2048)
        .min(limits.max_texture_dimension_2d);
    while u64::from((size * 4).div_ceil(256) * 256) * u64::from(size) > limits.max_buffer_size {
        size /= 2;
        if size == 0 {
            return 1;
        }
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_respects_resolution_and_buffer_limits() {
        let mut limits = wgpu::Limits::downlevel_defaults();
        assert_eq!(target_size(0, &limits), 1);
        assert_eq!(target_size(u32::MAX, &limits), 2048);
        limits.max_texture_dimension_2d = 1024;
        assert_eq!(target_size(3000, &limits), 1024);
        limits.max_buffer_size = 256 * 64;
        let side = target_size(1024, &limits);
        assert!(
            u64::from((side * 4).div_ceil(256) * 256) * u64::from(side) <= limits.max_buffer_size
        );
    }

    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn model_target_displays_opaque_background_and_blended_materials() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("model UI test"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
            trace: wgpu::Trace::Off,
        }))
        .unwrap();
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut imgui = imgui::Context::create();
        imgui.set_ini_filename(None);
        let mut ui_renderer = imgui_wgpu::Renderer::new(
            &mut imgui,
            &device,
            &queue,
            imgui_wgpu::RendererConfig {
                texture_format: wgpu::TextureFormat::Rgba8UnormSrgb,
                ..Default::default()
            },
        );
        let mut ctx = ViewGpuContext {
            device: &device,
            queue: &queue,
            renderer: &mut ui_renderer,
        };
        let renderer = render3d::Renderer::new(&device, &queue);
        for fixture in [
            "cube.glb",
            "textured.glb",
            "transformed.glb",
            "skin.glb",
            "morph.glb",
            "transparent.glb",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("crates/render3d/tests/fixtures")
                .join(fixture);
            let scene = renderer.load_scene(&path).unwrap();
            let camera = render3d::Camera::frame(scene.bounds()).unwrap();
            let mut surface = ModelSurface::new(&mut ctx, 65).unwrap();
            let expected = renderer
                .render_to_target(&scene, &camera, &mut surface.target)
                .unwrap();
            for rgb in [[24u8, 56, 92], [200u8, 180, 160]] {
                let background = opaque_background(rgb.map(|c| f32::from(c) / 255.0));
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer
                    .encode_with_background(
                        &scene,
                        &camera,
                        &surface.target,
                        &mut encoder,
                        background,
                    )
                    .unwrap();
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("model UI test pixels"),
                    size: 512 * 65,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                encoder.copy_texture_to_buffer(
                    ctx.renderer
                        .textures
                        .get(surface.texture_id)
                        .unwrap()
                        .texture()
                        .as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(512),
                            rows_per_image: Some(65),
                        },
                    },
                    wgpu::Extent3d {
                        width: 65,
                        height: 65,
                        depth_or_array_layers: 1,
                    },
                );
                let submission = queue.submit([encoder.finish()]);
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        let _ = tx.send(result);
                    });
                device.poll(wgpu::PollType::wait_for(submission)).unwrap();
                rx.recv().unwrap().unwrap();
                let mapped = readback.slice(..).get_mapped_range();
                let actual: Vec<u8> = mapped
                    .chunks_exact(512)
                    .flat_map(|row| row[..260].iter().copied())
                    .collect();
                assert_eq!(&actual[..4], &[rgb[0], rgb[1], rgb[2], 255], "{fixture}");
                assert!(
                    actual.chunks_exact(4).all(|pixel| pixel[3] == 255),
                    "{fixture}"
                );
                // Compare direct opaque rendering with the existing transparent output
                // composited in linear space, including the half-transparent fixture.
                for (actual, source) in actual.chunks_exact(4).zip(expected.pixels.chunks_exact(4))
                {
                    let alpha = f64::from(source[3]) / 255.0;
                    for channel in 0..3 {
                        let model = srgb_to_linear(f32::from(source[channel]) / 255.0);
                        let bg = srgb_to_linear(f32::from(rgb[channel]) / 255.0);
                        let linear = model * alpha + bg * (1.0 - alpha);
                        let srgb = if linear <= 0.0031308 {
                            linear * 12.92
                        } else {
                            1.055 * linear.powf(1.0 / 2.4) - 0.055
                        };
                        let expected_channel = (srgb * 255.0).round() as i16;
                        assert!(
                            (i16::from(actual[channel]) - expected_channel).abs() <= 3,
                            "{fixture}"
                        );
                    }
                }
                drop(mapped);
                readback.unmap();
            }
            let id = surface.texture_id;
            surface.release(&mut ctx);
            assert!(ctx.renderer.textures.get(id).is_none());
        }
        assert!(pollster::block_on(device.pop_error_scope()).is_none());
    }
}
