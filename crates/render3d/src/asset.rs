use std::path::Path;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};

// Field order matches the vertex attributes in renderer.rs and model.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    pub color: [f32; 4],
}

/// One triangle list in world space. Node, morph, and skin transforms are already applied.
pub(crate) struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub material: Option<usize>,
    pub center: Vec3,
}

/// CPU data is kept separate from GPU resources so loading can be tested alone.
pub(crate) struct ModelAsset {
    pub document: gltf::Document,
    pub buffers: Vec<Vec<u8>>,
    pub primitives: Vec<Primitive>,
    pub min: Vec3,
    pub max: Vec3,
}

impl ModelAsset {
    /// Load a self-contained GLB in its initial pose. Animation is not advanced.
    pub fn load(path: &Path) -> Result<Self> {
        let gltf = gltf::Gltf::open(path)
            .with_context(|| format!("failed to open GLB {}", path.display()))?;
        ensure!(gltf.blob.is_some(), "expected a binary GLB file");
        for extension in gltf.document.extensions_required() {
            ensure!(
                extension == "KHR_materials_unlit",
                "unsupported required GLB extension: {extension}"
            );
        }
        let blob = gltf.blob.as_deref().unwrap_or_default();
        let mut buffers = Vec::new();
        for buffer in gltf.document.buffers() {
            ensure!(
                matches!(buffer.source(), gltf::buffer::Source::Bin),
                "GLB buffers must be embedded"
            );
            ensure!(buffer.length() <= blob.len(), "GLB buffer is truncated");
            buffers.push(blob.to_vec());
        }
        for view in gltf.document.views() {
            let end = view
                .offset()
                .checked_add(view.length())
                .context("GLB buffer view overflow")?;
            ensure!(
                end <= buffers[view.buffer().index()].len(),
                "GLB buffer view is out of bounds"
            );
        }
        let document = gltf.document;
        let nodes: Vec<_> = document.nodes().collect();
        let mut parents = vec![None; nodes.len()];
        for node in &nodes {
            for child in node.children() {
                ensure!(
                    parents[child.index()].replace(node.index()).is_none(),
                    "GLB node has multiple parents"
                );
            }
        }
        let mut worlds = vec![Mat4::IDENTITY; nodes.len()];
        let mut states = vec![0; nodes.len()];
        for index in 0..nodes.len() {
            resolve_world(index, &nodes, &parents, &mut worlds, &mut states)?;
        }
        let roots: Vec<_> = if let Some(scene) = document
            .default_scene()
            .or_else(|| document.scenes().next())
        {
            scene.nodes().collect()
        } else {
            nodes
                .iter()
                .filter(|node| parents[node.index()].is_none())
                .cloned()
                .collect()
        };
        let mut primitives = Vec::new();
        let mut pending = roots;
        while let Some(node) = pending.pop() {
            pending.extend(node.children());
            if let Some(mesh) = node.mesh() {
                for primitive in mesh.primitives() {
                    primitives.push(load_primitive(&node, &mesh, primitive, &buffers, &worlds)?);
                }
            }
        }
        ensure!(
            !primitives.is_empty(),
            "GLB scene contains no triangle meshes"
        );
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for primitive in &primitives {
            for vertex in &primitive.vertices {
                let position = Vec3::from_array(vertex.position);
                ensure!(
                    position.is_finite(),
                    "GLB contains a non-finite vertex position"
                );
                min = min.min(position);
                max = max.max(position);
            }
        }
        Ok(Self {
            document,
            buffers,
            primitives,
            min,
            max,
        })
    }

    pub fn image(&self, source: gltf::Image<'_>) -> Result<image::RgbaImage> {
        match source.source() {
            gltf::image::Source::View { view, .. } => {
                let bytes = &self.buffers[view.buffer().index()]
                    [view.offset()..view.offset() + view.length()];
                Ok(image::load_from_memory(bytes)
                    .context("failed to decode GLB texture")?
                    .to_rgba8())
            }
            gltf::image::Source::Uri { .. } => anyhow::bail!("GLB textures must be embedded"),
        }
    }
}

fn resolve_world(
    index: usize,
    nodes: &[gltf::Node<'_>],
    parents: &[Option<usize>],
    worlds: &mut [Mat4],
    states: &mut [u8],
) -> Result<()> {
    if states[index] == 2 {
        return Ok(());
    }
    ensure!(states[index] == 0, "GLB node hierarchy contains a cycle");
    states[index] = 1;
    let parent = if let Some(parent) = parents[index] {
        resolve_world(parent, nodes, parents, worlds, states)?;
        worlds[parent]
    } else {
        Mat4::IDENTITY
    };
    worlds[index] = parent * Mat4::from_cols_array_2d(&nodes[index].transform().matrix());
    states[index] = 2;
    Ok(())
}

fn load_primitive(
    node: &gltf::Node<'_>,
    mesh: &gltf::Mesh<'_>,
    primitive: gltf::Primitive<'_>,
    buffers: &[Vec<u8>],
    worlds: &[Mat4],
) -> Result<Primitive> {
    let reader = primitive.reader(|buffer| Some(buffers[buffer.index()].as_slice()));
    let positions: Vec<_> = reader
        .read_positions()
        .context("GLB primitive has no positions")?
        .collect();
    ensure!(
        !positions.is_empty() && positions.len() <= u32::MAX as usize,
        "invalid GLB vertex count"
    );
    let raw_indices: Vec<_> = reader
        .read_indices()
        .map(|indices| indices.into_u32().collect())
        .unwrap_or_else(|| (0..positions.len() as u32).collect());
    ensure!(
        raw_indices
            .iter()
            .all(|&index| (index as usize) < positions.len()),
        "GLB index is out of bounds"
    );
    let mut indices = Vec::new();
    match primitive.mode() {
        gltf::mesh::Mode::Triangles => {
            ensure!(raw_indices.len() % 3 == 0, "incomplete GLB triangle");
            indices = raw_indices;
        }
        gltf::mesh::Mode::TriangleStrip => {
            for i in 2..raw_indices.len() {
                let mut triangle = [raw_indices[i - 2], raw_indices[i - 1], raw_indices[i]];
                if i % 2 != 0 {
                    triangle.swap(0, 1);
                }
                indices.extend(triangle);
            }
        }
        gltf::mesh::Mode::TriangleFan => {
            for i in 2..raw_indices.len() {
                indices.extend([raw_indices[0], raw_indices[i - 1], raw_indices[i]]);
            }
        }
        mode => anyhow::bail!("unsupported GLB primitive mode: {mode:?}"),
    }
    ensure!(!indices.is_empty(), "GLB primitive contains no triangles");
    let mut vertices: Vec<_> = positions
        .into_iter()
        .map(|position| Vertex {
            position,
            normal: [0.0; 3],
            tangent: [0.0; 4],
            uv0: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        })
        .collect();
    let has_normals = reader.read_normals().is_some();
    if let Some(normals) = reader.read_normals() {
        for (vertex, normal) in vertices.iter_mut().zip(normals) {
            vertex.normal = normal;
        }
    }
    if let Some(tangents) = reader.read_tangents() {
        for (vertex, tangent) in vertices.iter_mut().zip(tangents) {
            vertex.tangent = tangent;
        }
    }
    for set in 0..2 {
        if let Some(coords) = reader.read_tex_coords(set) {
            for (vertex, coord) in vertices.iter_mut().zip(coords.into_f32()) {
                if set == 0 {
                    vertex.uv0 = coord;
                } else {
                    vertex.uv1 = coord;
                }
            }
        }
    }
    if let Some(colors) = reader.read_colors(0) {
        for (vertex, color) in vertices.iter_mut().zip(colors.into_rgba_f32()) {
            vertex.color = color;
        }
    }
    // Apply the default morph weights before skinning or node transforms.
    let weights = node.weights().or_else(|| mesh.weights()).unwrap_or(&[]);
    for (index, (positions, normals, tangents)) in reader.read_morph_targets().enumerate() {
        let weight = weights.get(index).copied().unwrap_or(0.0);
        if let Some(values) = positions {
            for (vertex, delta) in vertices.iter_mut().zip(values) {
                vertex.position = (Vec3::from_array(vertex.position)
                    + Vec3::from_array(delta) * weight)
                    .to_array();
            }
        }
        if let Some(values) = normals {
            for (vertex, delta) in vertices.iter_mut().zip(values) {
                vertex.normal =
                    (Vec3::from_array(vertex.normal) + Vec3::from_array(delta) * weight).to_array();
            }
        }
        if let Some(values) = tangents {
            for (vertex, delta) in vertices.iter_mut().zip(values) {
                for axis in 0..3 {
                    vertex.tangent[axis] += delta[axis] * weight;
                }
            }
        }
    }
    if !has_normals {
        for triangle in indices.chunks_exact(3) {
            let a = Vec3::from_array(vertices[triangle[0] as usize].position);
            let b = Vec3::from_array(vertices[triangle[1] as usize].position);
            let c = Vec3::from_array(vertices[triangle[2] as usize].position);
            let normal = (b - a).cross(c - a);
            for &index in triangle {
                let vertex = &mut vertices[index as usize];
                vertex.normal = (Vec3::from_array(vertex.normal) + normal).to_array();
            }
        }
    }
    let mut transforms = vec![worlds[node.index()]; vertices.len()];
    if let Some(skin) = node.skin() {
        let inverse_binds: Vec<_> = skin
            .reader(|buffer| Some(buffers[buffer.index()].as_slice()))
            .read_inverse_bind_matrices()
            .map(|values| values.map(|m| Mat4::from_cols_array_2d(&m)).collect())
            .unwrap_or_else(|| vec![Mat4::IDENTITY; skin.joints().len()]);
        let joints: Vec<_> = skin
            .joints()
            .zip(inverse_binds)
            .map(|(joint, bind)| worlds[joint.index()] * bind)
            .collect();
        let joint_indices = reader
            .read_joints(0)
            .context("skinned GLB mesh has no joint indices")?
            .into_u16();
        let joint_weights = reader
            .read_weights(0)
            .context("skinned GLB mesh has no joint weights")?
            .into_f32();
        ensure!(
            reader.read_joints(1).is_none(),
            "more than four skin influences per vertex are unsupported"
        );
        for ((transform, indices), weights) in
            transforms.iter_mut().zip(joint_indices).zip(joint_weights)
        {
            let sum: f32 = weights.iter().sum();
            ensure!(sum.is_finite() && sum > 0.0, "invalid GLB skin weights");
            *transform = Mat4::ZERO;
            for (index, weight) in indices.into_iter().zip(weights) {
                if weight > 0.0 {
                    *transform += *joints
                        .get(index as usize)
                        .context("GLB joint index is out of bounds")?
                        * (weight / sum);
                }
            }
        }
    }
    let mut center = Vec3::ZERO;
    for (vertex, transform) in vertices.iter_mut().zip(transforms) {
        ensure!(
            transform.is_finite() && transform.determinant().abs() > 1e-12,
            "GLB has a singular vertex transform"
        );
        let position = transform.transform_point3(Vec3::from_array(vertex.position));
        vertex.position = position.to_array();
        vertex.normal = transform
            .inverse()
            .transpose()
            .transform_vector3(Vec3::from_array(vertex.normal))
            .normalize_or_zero()
            .to_array();
        let tangent = transform
            .transform_vector3(Vec3::new(
                vertex.tangent[0],
                vertex.tangent[1],
                vertex.tangent[2],
            ))
            .normalize_or_zero();
        vertex.tangent = [
            tangent.x,
            tangent.y,
            tangent.z,
            vertex.tangent[3] * transform.determinant().signum(),
        ];
        center += position;
    }
    if worlds[node.index()].determinant() < 0.0 {
        for triangle in indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }
    center /= vertices.len() as f32;
    Ok(Primitive {
        vertices,
        indices,
        material: primitive.material().index(),
        center,
    })
}
