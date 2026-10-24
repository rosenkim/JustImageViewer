use std::path::Path;

use anyhow::Result;
use wgpu::util::DeviceExt;

use crate::asset::Vertex;
use crate::camera::Camera;
use crate::material::material_layout;
use crate::offscreen::{COLOR_FORMAT, DEPTH_FORMAT, OffscreenTarget, RenderedImage};
use crate::scene::Scene;

/// Reusable GPU pipelines. Scenes and cameras can change without rebuilding them.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Vec<wgpu::RenderPipeline>,
    camera_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let material_layout = material_layout(device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/model.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model pipeline layout"),
            bind_group_layouts: &[&camera_layout, &material_layout],
            push_constant_ranges: &[],
        });
        // Build every pipeline variant in the order used by `pipeline_index`.
        let mut pipelines = Vec::new();
        for blended in [false, true] {
            for double_sided in [false, true] {
                debug_assert_eq!(pipelines.len(), pipeline_index(blended, double_sided));
                pipelines.push(create_pipeline(
                    device,
                    &shader,
                    &layout,
                    blended,
                    double_sided,
                ));
            }
        }

        Self {
            device: device.clone(),
            queue: queue.clone(),
            pipelines,
            camera_layout,
            material_layout,
        }
    }

    pub fn load_scene(&self, path: &Path) -> Result<Scene> {
        Scene::load(&self.device, &self.queue, &self.material_layout, path)
    }

    pub fn create_target(&self, size: u32) -> Result<OffscreenTarget> {
        OffscreenTarget::new(&self.device, size)
    }

    /// Render and read pixels on the calling thread, using a target from this device.
    pub fn render_to_target(
        &self,
        scene: &Scene,
        camera: &Camera,
        target: &mut OffscreenTarget,
    ) -> Result<RenderedImage> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("3D viewport render"),
            });
        self.encode(scene, camera, target, &mut encoder)?;
        target.read_pixels(&self.device, &self.queue, encoder)
    }

    /// Record a GPU-only pass. The caller submits the encoder and displays the target.
    pub fn encode(
        &self,
        scene: &Scene,
        camera: &Camera,
        target: &OffscreenTarget,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<()> {
        // Each pass owns its uniform snapshot, so concurrent views do not overwrite it.
        let uniform = camera.uniform(1.0)?;
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3D viewport camera"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let camera_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3D viewport camera"),
            layout: &self.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        // Opaque geometry writes depth first. Blend whole primitives from far to near.
        let mut order: Vec<_> = scene.meshes.iter().collect();
        order.sort_by(|a, b| {
            let a_blended = scene.materials[a.material].blended;
            let b_blended = scene.materials[b.material].blended;
            a_blended.cmp(&b_blended).then_with(|| {
                if a_blended {
                    b.center
                        .distance_squared(camera.eye)
                        .total_cmp(&a.center.distance_squared(camera.eye))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("model offscreen pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &camera_bind_group, &[]);
            for mesh in order {
                let material = &scene.materials[mesh.material];
                pass.set_pipeline(&self.pipelines[material.pipeline_index]);
                pass.set_bind_group(1, &material.bind_group, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }
        Ok(())
    }
}

/// Find the pipeline for a material. Blending and culling are fixed per pipeline.
pub(crate) fn pipeline_index(blended: bool, double_sided: bool) -> usize {
    usize::from(blended) * 2 + usize::from(double_sided)
}

fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    blended: bool,
    double_sided: bool,
) -> wgpu::RenderPipeline {
    const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2, 4 => Float32x2, 5 => Float32x4
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("model pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &ATTRIBUTES,
            }],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: if double_sided {
                None
            } else {
                Some(wgpu::Face::Back)
            },
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: !blended,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: blended.then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}
