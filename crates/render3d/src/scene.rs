use std::path::Path;

use anyhow::{Result, ensure};
use glam::Vec3;
use wgpu::util::DeviceExt;

use crate::asset::ModelAsset;
use crate::material::{Material, upload_material};

/// World-space bounds of the loaded initial pose.
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

/// Owns the uploaded meshes and materials, independent of the camera.
pub struct Scene {
    pub(crate) materials: Vec<Material>,
    pub(crate) meshes: Vec<Mesh>,
    bounds: Bounds,
}

/// GPU buffers for one primitive. `center` is used to sort transparent meshes.
pub(crate) struct Mesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_count: u32,
    pub material: usize,
    pub center: Vec3,
}

impl Scene {
    pub(crate) fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material_layout: &wgpu::BindGroupLayout,
        path: &Path,
    ) -> Result<Self> {
        let asset = ModelAsset::load(path)?;
        let bounds = Bounds {
            min: asset.min,
            max: asset.max,
        };
        let mut materials = Vec::new();
        for material in asset.document.materials() {
            materials.push(upload_material(
                device,
                queue,
                material_layout,
                &asset,
                material,
            )?);
        }
        let default_material = materials.len();
        if let Some(material) = asset
            .document
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .map(|primitive| primitive.material())
            .find(|material| material.index().is_none())
        {
            materials.push(upload_material(
                device,
                queue,
                material_layout,
                &asset,
                material,
            )?);
        }
        let mut meshes = Vec::new();
        for primitive in asset.primitives {
            ensure!(
                primitive.indices.len() <= u32::MAX as usize,
                "GLB has too many indices"
            );
            let vertex_bytes = bytemuck::cast_slice(&primitive.vertices);
            let index_bytes = bytemuck::cast_slice(&primitive.indices);
            ensure!(
                vertex_bytes.len() as u64 <= device.limits().max_buffer_size
                    && index_bytes.len() as u64 <= device.limits().max_buffer_size,
                "GLB geometry exceeds the device buffer limit"
            );
            meshes.push(Mesh {
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("model vertices"),
                    contents: vertex_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("model indices"),
                    contents: index_bytes,
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: primitive.indices.len() as u32,
                material: primitive.material.unwrap_or(default_material),
                center: primitive.center,
            });
        }
        Ok(Self {
            materials,
            meshes,
            bounds,
        })
    }

    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
}
