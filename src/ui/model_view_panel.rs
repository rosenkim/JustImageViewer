use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};

use anyhow::Result;
use imgui::{MouseButton, Ui};
use render3d::{Renderer, Scene};

use crate::app::ViewerState;
use crate::core::media::MediaType;
use crate::render::{
    app_resources::AppResources,
    model_camera::ModelCamera,
    model_surface::{ModelSurface, opaque_background, target_size},
    view_gpu::ViewGpuContext,
};

use super::view_panel::ViewContentPanel;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LoadRequest {
    path: PathBuf,
    generation: u64,
}

#[derive(Default)]
struct ModelRequestState {
    current: Option<LoadRequest>,
    generation: u64,
}

impl ModelRequestState {
    fn select(&mut self, path: Option<PathBuf>, reload: bool) -> bool {
        if !reload && self.current.as_ref().map(|r| &r.path) == path.as_ref() {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.current = path.map(|path| LoadRequest {
            path,
            generation: self.generation,
        });
        true
    }

    fn accepts(&self, request: &LoadRequest) -> bool {
        self.current.as_ref() == Some(request)
    }
}

struct LoadedModel {
    scene: Scene,
    camera: ModelCamera,
}

struct PendingModel {
    request: LoadRequest,
    receiver: mpsc::Receiver<Result<LoadedModel>>,
}

pub struct ModelViewPanel {
    requests: ModelRequestState,
    renderer: Option<Arc<Renderer>>,
    pending: Option<PendingModel>,
    loaded: Option<LoadedModel>,
    surface: Option<ModelSurface>,
    error: Option<String>,
    reload: bool,
    requested_size: u32,
    dragging: Option<MouseButton>,
    drawn: bool,
    dirty: bool,
    background: [f32; 3],
}

impl ModelViewPanel {
    pub fn new() -> Self {
        Self {
            requests: ModelRequestState::default(),
            renderer: None,
            pending: None,
            loaded: None,
            surface: None,
            error: None,
            reload: false,
            requested_size: 512,
            dragging: None,
            drawn: false,
            dirty: false,
            background: [0.0; 3],
        }
    }

    fn clear_model(&mut self, ctx: &mut ViewGpuContext<'_>) {
        if let Some(surface) = self.surface.take() {
            surface.release(ctx);
        }
        self.loaded = None;
        self.error = None;
        self.dragging = None;
    }

    fn poll_load(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(anyhow::anyhow!("model loading worker stopped"))
            }
        };
        let pending = self.pending.take().unwrap();
        // A refresh can replace the file without changing its path.
        if !self.requests.accepts(&pending.request) {
            return;
        }
        match result {
            Ok(model) => {
                self.loaded = Some(model);
                self.dirty = true;
            }
            Err(err) => {
                log::warn!("Failed to load {}: {err:#}", pending.request.path.display());
                self.error = Some(format!("Failed to load GLB: {err:#}"));
            }
        }
    }
}

fn selected_model_path(state: &ViewerState) -> Option<PathBuf> {
    if state.is_multi_select() {
        return None;
    }
    state
        .current_entry()
        .filter(|entry| entry.format.media_type() == MediaType::Model3D)
        .map(|entry| entry.path.clone())
}

impl ViewContentPanel for ModelViewPanel {
    fn request_reload(&mut self) {
        self.reload = true;
    }

    fn prepare_frame(&mut self, state: &ViewerState, ctx: &mut ViewGpuContext<'_>) {
        self.drawn = false;
        let reload = std::mem::take(&mut self.reload);
        if self.requests.select(selected_model_path(state), reload) {
            self.clear_model(ctx);
        }
        self.poll_load();
        let Some(request) = self.requests.current.clone() else {
            return;
        };

        // Finish one worker before starting the latest request. Rapid navigation
        // must not create an unbounded queue of expensive GLB loads.
        if self.pending.is_none() && self.loaded.is_none() && self.error.is_none() {
            let renderer = self
                .renderer
                .get_or_insert_with(|| Arc::new(Renderer::new(ctx.device, ctx.queue)))
                .clone();
            let (tx, receiver) = mpsc::sync_channel(1);
            let path = request.path.clone();
            tokio::task::spawn_blocking(move || {
                let result = (|| {
                    let scene = renderer.load_scene(&path)?;
                    let camera = ModelCamera::new(scene.bounds())?;
                    Ok(LoadedModel { scene, camera })
                })();
                let _ = tx.send(result);
            });
            self.pending = Some(PendingModel { request, receiver });
        }

        if self.loaded.is_some() && self.error.is_none() {
            let size = target_size(self.requested_size, &ctx.device.limits());
            if self
                .surface
                .as_ref()
                .is_none_or(|s| s.target.size() != size)
            {
                match ModelSurface::new(ctx, size) {
                    Ok(surface) => {
                        if let Some(old) = self.surface.replace(surface) {
                            old.release(ctx);
                        }
                        self.dirty = true;
                    }
                    Err(err) => self.error = Some(format!("Failed to create model view: {err:#}")),
                }
            }
        }
    }

    fn render(
        &mut self,
        ui: &Ui,
        state: &mut ViewerState,
        _resources: &AppResources,
        _is_pending: bool,
    ) {
        // The Library may change the selection earlier in this same UI frame.
        if self.requests.current.as_ref().map(|r| &r.path) != selected_model_path(state).as_ref() {
            ui.text("Loading GLB...");
            return;
        }
        if let Some(error) = &self.error {
            ui.text_wrapped(error);
            return;
        }
        let (Some(model), Some(surface)) = (&mut self.loaded, &self.surface) else {
            ui.text("Loading GLB...");
            return;
        };
        if ui.button("Reset View") {
            model.camera.reset();
            self.dirty = true;
        }
        ui.same_line();
        ui.text_disabled("Controls");
        if ui.is_item_hovered() {
            ui.tooltip_text("Left drag: rotate\nRight drag: pan\nMouse wheel: zoom");
        }
        let available = ui.content_region_avail();
        let side = available[0].min(available[1]);
        if side < 1.0 {
            return;
        }
        let cursor = ui.cursor_pos();
        ui.set_cursor_pos([
            cursor[0] + (available[0] - side) * 0.5,
            cursor[1] + (available[1] - side) * 0.5,
        ]);
        let background = state.config().background_style.resolved_colors_rgb().0;
        if self.background != background {
            self.background = background;
            self.dirty = true;
        }
        imgui::Image::new(surface.texture_id, [side, side]).build(ui);
        self.drawn = true;
        let scale = ui.io().display_framebuffer_scale;
        self.requested_size = (side * scale[0].max(scale[1])).ceil().max(1.0) as u32;

        let hovered = ui.is_item_hovered();
        if hovered {
            for button in [MouseButton::Left, MouseButton::Right] {
                if ui.is_mouse_clicked(button) {
                    self.dragging = Some(button);
                }
            }
            if ui.io().mouse_wheel != 0.0 {
                model.camera.zoom(ui.io().mouse_wheel);
                self.dirty = true;
            }
        }
        if let Some(button) = self.dragging {
            if !ui.is_mouse_down(button) || ui.io().app_focus_lost {
                self.dragging = None;
            } else {
                let delta = ui.io().mouse_delta;
                if delta != [0.0, 0.0] {
                    match button {
                        MouseButton::Left => model.camera.orbit(delta),
                        MouseButton::Right => model.camera.pan(delta, side),
                        _ => {}
                    }
                    self.dirty = true;
                }
            }
        }
    }

    fn encode_frame(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !self.drawn || !self.dirty {
            return;
        }
        if let (Some(renderer), Some(model), Some(surface)) =
            (&self.renderer, &self.loaded, &self.surface)
        {
            if let Err(err) = renderer.encode_with_background(
                &model.scene,
                &model.camera.camera,
                &surface.target,
                encoder,
                opaque_background(self.background),
            ) {
                self.error = Some(format!("Failed to render GLB: {err:#}"));
            }
            self.dirty = false;
        }
    }

    fn is_pending(&self) -> bool {
        self.requests.current.is_some() && self.loaded.is_none() && self.error.is_none()
    }

    fn release(&mut self, ctx: &mut ViewGpuContext<'_>) {
        self.requests.select(None, true);
        self.clear_model(ctx);
        self.pending = None;
        self.renderer = None;
        self.drawn = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_selection_and_refresh_reject_stale_loads() {
        let mut state = ModelRequestState::default();
        assert!(state.select(Some("a.glb".into()), false));
        let first = state.current.clone().unwrap();
        assert!(!state.select(Some("a.glb".into()), false));
        assert!(state.select(Some("b.glb".into()), false));
        assert!(!state.accepts(&first));
        state.select(Some("a.glb".into()), false);
        assert!(!state.accepts(&first));
        let before_refresh = state.current.clone().unwrap();
        state.select(Some("a.glb".into()), true);
        assert!(!state.accepts(&before_refresh));
        assert!(state.accepts(state.current.as_ref().unwrap()));
        let before_clear = state.current.clone().unwrap();
        state.select(None, false);
        assert!(!state.accepts(&before_clear));
    }

    #[test]
    fn stale_worker_failure_does_not_replace_the_current_request() {
        let mut panel = ModelViewPanel::new();
        panel.requests.select(Some("old.glb".into()), false);
        let request = panel.requests.current.clone().unwrap();
        let (tx, receiver) = mpsc::sync_channel(1);
        panel.pending = Some(PendingModel { request, receiver });
        panel.requests.select(Some("new.glb".into()), false);
        assert!(tx.send(Err(anyhow::anyhow!("old file is invalid"))).is_ok());
        panel.poll_load();
        assert!(panel.pending.is_none());
        assert!(panel.error.is_none());
        assert!(panel.is_pending());

        let request = panel.requests.current.clone().unwrap();
        let (tx, receiver) = mpsc::sync_channel(1);
        panel.pending = Some(PendingModel { request, receiver });
        drop(tx);
        panel.poll_load();
        assert!(panel.error.as_ref().unwrap().contains("worker stopped"));
        assert!(!panel.is_pending());
    }
}
