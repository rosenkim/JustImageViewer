use std::path::Path;

use anyhow::Result;

use crate::{Camera, OffscreenTarget, RenderedImage, Renderer, Scene};

/// A scene and camera paired with a renderer for simple viewport use.
pub struct Viewport3D {
    renderer: Renderer,
    scene: Scene,
    camera: Camera,
}

impl Viewport3D {
    pub fn new(renderer: Renderer, scene: Scene) -> Result<Self> {
        let camera = Camera::frame(scene.bounds())?;
        Ok(Self {
            renderer,
            scene,
            camera,
        })
    }

    pub fn from_glb(device: &wgpu::Device, queue: &wgpu::Queue, path: &Path) -> Result<Self> {
        let renderer = Renderer::new(device, queue);
        let scene = renderer.load_scene(path)?;
        Self::new(renderer, scene)
    }

    /// Replace the scene and frame it while keeping the GPU pipelines.
    pub fn load_glb(&mut self, path: &Path) -> Result<()> {
        let scene = self.renderer.load_scene(path)?;
        let camera = Camera::frame(scene.bounds())?;
        self.scene = scene;
        self.camera = camera;
        Ok(())
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Create a square target, render, and wait for RGBA pixels.
    pub fn render_offscreen(&self, size: u32) -> Result<RenderedImage> {
        let mut target = self.renderer.create_target(size)?;
        self.render_to_target(&mut target)
    }

    pub fn render_to_target(&self, target: &mut OffscreenTarget) -> Result<RenderedImage> {
        self.renderer
            .render_to_target(&self.scene, &self.camera, target)
    }

    /// Record a pass for displaying the target directly in a GPU-based UI.
    pub fn encode(
        &self,
        target: &OffscreenTarget,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<()> {
        self.renderer
            .encode(&self.scene, &self.camera, target, encoder)
    }
}
