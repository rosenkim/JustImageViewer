use anyhow::{Result, ensure};
use wgpu::util::DeviceExt;

use crate::asset::ModelAsset;
use crate::renderer::pipeline_index;

// Field order and alignment match Material in model.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaterialUniform {
    base_color: [f32; 4],
    emissive_metallic: [f32; 4],
    factors: [f32; 4],
    flags: [u32; 4],
    uv_sets: [u32; 4],
    uv_extra: [u32; 4],
}

/// GPU bindings for one glTF material, plus how the renderer should draw it.
pub(crate) struct Material {
    pub bind_group: wgpu::BindGroup,
    pub pipeline_index: usize,
    pub blended: bool,
}

pub(crate) fn material_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let mut entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }];
    for slot in 0..5 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: slot * 2 + 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: slot * 2 + 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("model material layout"),
        entries: &entries,
    })
}

pub(crate) fn upload_material(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    asset: &ModelAsset,
    material: gltf::Material<'_>,
) -> Result<Material> {
    let pbr = material.pbr_metallic_roughness();
    let base = pbr.base_color_texture();
    let mr = pbr.metallic_roughness_texture();
    let normal = material.normal_texture();
    let occlusion = material.occlusion_texture();
    let emissive = material.emissive_texture();
    let uv_sets = [
        base.as_ref().map_or(0, |t| t.tex_coord()),
        mr.as_ref().map_or(0, |t| t.tex_coord()),
        normal.as_ref().map_or(0, |t| t.tex_coord()),
        occlusion.as_ref().map_or(0, |t| t.tex_coord()),
    ];
    let emissive_uv = emissive.as_ref().map_or(0, |t| t.tex_coord());
    ensure!(
        uv_sets
            .iter()
            .chain(std::iter::once(&emissive_uv))
            .all(|&set| set <= 1),
        "GLB material requires UV sets beyond TEXCOORD_1"
    );
    let alpha_mode = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => 0,
        gltf::material::AlphaMode::Mask => 1,
        gltf::material::AlphaMode::Blend => 2,
    };
    let emissive_factor = material.emissive_factor();
    let uniform = MaterialUniform {
        base_color: pbr.base_color_factor(),
        emissive_metallic: [
            emissive_factor[0],
            emissive_factor[1],
            emissive_factor[2],
            pbr.metallic_factor(),
        ],
        factors: [
            pbr.roughness_factor(),
            normal.as_ref().map_or(1.0, |t| t.scale()),
            occlusion.as_ref().map_or(1.0, |t| t.strength()),
            material.alpha_cutoff().unwrap_or(0.5),
        ],
        flags: [
            alpha_mode,
            u32::from(material.unlit()),
            u32::from(normal.is_some()),
            u32::from(material.double_sided()),
        ],
        uv_sets,
        uv_extra: [emissive_uv, 0, 0, 0],
    };
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("model material"),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let sources = [
        base.map(|t| t.texture()),
        mr.map(|t| t.texture()),
        normal.map(|t| t.texture()),
        occlusion.map(|t| t.texture()),
        emissive.map(|t| t.texture()),
    ];
    let mut textures = Vec::new();
    for (slot, source) in sources.into_iter().enumerate() {
        let srgb = slot == 0 || slot == 4;
        let fallback = if slot == 2 {
            [128, 128, 255, 255]
        } else {
            [255; 4]
        };
        let image = source
            .as_ref()
            .map(|t| asset.image(t.source()))
            .transpose()?
            .unwrap_or_else(|| image::RgbaImage::from_pixel(1, 1, image::Rgba(fallback)));
        let (width, height) = image.dimensions();
        ensure!(
            width > 0
                && height > 0
                && width <= device.limits().max_texture_dimension_2d
                && height <= device.limits().max_texture_dimension_2d,
            "GLB texture dimensions exceed the device limit"
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model material texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            image.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            texture.size(),
        );
        let source_sampler = source.as_ref().map(|t| t.sampler());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model material sampler"),
            address_mode_u: source_sampler
                .as_ref()
                .map_or(wgpu::AddressMode::Repeat, |s| wrap_mode(s.wrap_s())),
            address_mode_v: source_sampler
                .as_ref()
                .map_or(wgpu::AddressMode::Repeat, |s| wrap_mode(s.wrap_t())),
            mag_filter: if source_sampler.as_ref().and_then(|s| s.mag_filter())
                == Some(gltf::texture::MagFilter::Nearest)
            {
                wgpu::FilterMode::Nearest
            } else {
                wgpu::FilterMode::Linear
            },
            min_filter: match source_sampler.as_ref().and_then(|s| s.min_filter()) {
                Some(
                    gltf::texture::MinFilter::Nearest
                    | gltf::texture::MinFilter::NearestMipmapNearest
                    | gltf::texture::MinFilter::NearestMipmapLinear,
                ) => wgpu::FilterMode::Nearest,
                _ => wgpu::FilterMode::Linear,
            },
            ..Default::default()
        });
        textures.push((texture.create_view(&Default::default()), sampler));
    }
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: buffer.as_entire_binding(),
    }];
    for (slot, (view, sampler)) in textures.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry {
            binding: slot as u32 * 2 + 1,
            resource: wgpu::BindingResource::TextureView(view),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: slot as u32 * 2 + 2,
            resource: wgpu::BindingResource::Sampler(sampler),
        });
    }
    Ok(Material {
        bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model material"),
            layout,
            entries: &entries,
        }),
        pipeline_index: pipeline_index(alpha_mode == 2, material.double_sided()),
        blended: alpha_mode == 2,
    })
}

fn wrap_mode(mode: gltf::texture::WrappingMode) -> wgpu::AddressMode {
    match mode {
        gltf::texture::WrappingMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
        gltf::texture::WrappingMode::MirroredRepeat => wgpu::AddressMode::MirrorRepeat,
        gltf::texture::WrappingMode::Repeat => wgpu::AddressMode::Repeat,
    }
}
