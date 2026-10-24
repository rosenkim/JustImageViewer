//! Small 3D viewport engine using the caller's wgpu device and queue.
//!
//! `Renderer` owns reusable pipelines, `Scene` owns uploaded content, and `Camera`
//! describes a view. `Viewport3D` combines them for simple offscreen rendering.
//! GLB loading uses the initial pose and requires embedded buffers and textures.
//!
//! This crate does not know about the app, a window, or a UI library.

mod asset;
mod camera;
mod material;
mod offscreen;
mod renderer;
mod scene;
mod viewport;

pub use camera::Camera;
pub use offscreen::{OffscreenTarget, RenderedImage};
pub use renderer::Renderer;
pub use scene::{Bounds, Scene};
pub use viewport::Viewport3D;

#[cfg(test)]
mod tests;
