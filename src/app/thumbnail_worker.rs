use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use super::{DirectoryId, ThumbnailResult};
use crate::core::image_loader::{self, DecodedImage};
use crate::core::media::{MediaFormat, MediaType};

struct ModelThumbnailRenderer {
    renderer: render3d::Renderer,
    target: render3d::OffscreenTarget,
}

impl ModelThumbnailRenderer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, size: u32) -> anyhow::Result<Self> {
        let renderer = render3d::Renderer::new(device, queue);
        let target = renderer.create_target(size.max(1))?;
        Ok(Self { renderer, target })
    }

    fn render(&mut self, path: &Path) -> anyhow::Result<DecodedImage> {
        let scene = self.renderer.load_scene(path)?;
        let camera = render3d::Camera::frame(scene.bounds())?;
        let image = self
            .renderer
            .render_to_target(&scene, &camera, &mut self.target)?;
        Ok(DecodedImage {
            width: image.width,
            height: image.height,
            pixels: image.pixels,
        })
    }
}

/// Decode or render thumbnails in the background and send each result to the viewer.
pub(super) async fn generate_thumbnails(
    directory_id: DirectoryId,
    paths: Vec<PathBuf>,
    thumbnail_size: u32,
    tx: mpsc::Sender<ThumbnailResult>,
    gpu: Option<(wgpu::Device, wgpu::Queue)>,
) {
    let mut model_renderer: Option<ModelThumbnailRenderer> = None;
    for path in paths {
        let media_type = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(MediaFormat::from_extension)
            .map(|format| format.media_type());
        let task_path = path.clone();
        let result = match media_type {
            Some(MediaType::Image2D) => {
                tokio::task::spawn_blocking(move || {
                    image_loader::load_thumbnail_rgba(&task_path, thumbnail_size)
                })
                .await
            }
            Some(MediaType::Model3D) => {
                let Some((device, queue)) = gpu.clone() else {
                    log::warn!(
                        "Cannot render model thumbnail without a GPU: {}",
                        path.display()
                    );
                    continue;
                };
                // The blocking task owns the renderer until it finishes, even if cancelled.
                let task = tokio::task::spawn_blocking(move || {
                    let mut renderer = match model_renderer {
                        Some(renderer) => renderer,
                        None => {
                            match ModelThumbnailRenderer::new(&device, &queue, thumbnail_size) {
                                Ok(renderer) => renderer,
                                Err(err) => return (None, Err(err)),
                            }
                        }
                    };
                    let result = renderer.render(&task_path);
                    (Some(renderer), result)
                })
                .await;
                match task {
                    Ok((renderer, result)) => {
                        model_renderer = renderer;
                        Ok(result)
                    }
                    Err(err) => {
                        model_renderer = None;
                        Err(err)
                    }
                }
            }
            _ => continue,
        };

        match result {
            Ok(Ok(decoded)) => {
                let msg = ThumbnailResult {
                    directory_id,
                    path,
                    width: decoded.width as u32,
                    height: decoded.height as u32,
                    pixels: decoded.pixels,
                };
                if tx.send(msg).await.is_err() {
                    break;
                }
            }
            Ok(Err(err)) => {
                log::warn!("Failed to load thumbnail for {}: {:#}", path.display(), err)
            }
            Err(err) => log::warn!("Thumbnail task panicked for {}: {:#}", path.display(), err),
        }
        tokio::task::yield_now().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a native GPU adapter"]
    async fn renders_multiple_model_thumbnails() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("thumbnail test"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/render3d/tests/fixtures");
        let paths = vec![fixtures.join("cube.glb"), fixtures.join("textured.glb")];
        let (tx, mut rx) = mpsc::channel(4);
        generate_thumbnails(
            DirectoryId(1),
            paths.clone(),
            64,
            tx,
            Some((device.clone(), queue)),
        )
        .await;
        for path in paths {
            let result = rx.recv().await.unwrap();
            assert_eq!(result.path, path);
            assert_eq!((result.width, result.height), (64, 64));
            assert!(result.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        }
        assert!(rx.recv().await.is_none());
        assert!(device.pop_error_scope().await.is_none());
    }
}
