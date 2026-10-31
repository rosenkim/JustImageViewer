/// MainApp - Application lifecycle manager.
///
/// Owns every subsystem (config, window, wgpu, ImGui, web server, image
/// pipeline) and drives the event loop. `main()` only parses arguments,
/// then hands control to `MainApp::initialize` + `MainApp::run`.
use anyhow::Context;
use imgui::{Context as ImguiContext, FontConfig, FontGlyphRanges, FontSource};
use imgui_wgpu::RendererConfig;
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::task::block_in_place;
use tokio_util::sync::CancellationToken;
use wgpu::{Backends, Instance, InstanceDescriptor};
use wgpu::{CompositeAlphaMode, Surface, SurfaceConfiguration, SurfaceError};
use winit::{
    dpi::LogicalSize,
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{Icon, Window, WindowBuilder},
};

use crate::app::ViewerState;
use crate::constants;
use crate::constants::{LOGICAL_DPI, POINTS_PER_INCH};
use crate::core::media::MediaType;
use crate::core::media_manager::MediaManager;
use crate::infra;
use crate::infra::config::{APPLICATION, AppTheme};
use crate::render::app_resources::AppResources;
use crate::render::image_uploader::ImageUploader;
use crate::render::imgui_textures::ImguiTextures;
use crate::render::texture_atlas_manager::TextureAtlasManager;
use crate::render::view_gpu::ViewGpuContext;
use crate::ui::render_ui;
use crate::ui::view_panel::ViewPanel;

struct WebServerState {
    shutdown_token: CancellationToken,
    shared_state: Arc<RwLock<infra::web_server::SharedWebState>>,
    server_handle: tokio::task::JoinHandle<()>,
    listening: Arc<AtomicBool>,
}

pub struct MainApp {
    app_state: ViewerState,
    view_panel: ViewPanel,
    webserver_state: Option<WebServerState>,

    window: Arc<Window>,
    // Kept alive for the lifetime of the surface/device.
    _instance: Instance,
    surface: Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: SurfaceConfiguration,

    imgui: ImguiContext,
    platform: WinitPlatform,
    renderer: imgui_wgpu::Renderer,
    imgui_textures: ImguiTextures,
    texture_atlas: TextureAtlasManager,
    app_resources: AppResources,

    media_manager: MediaManager,
    image_uploader: ImageUploader,

    ui_scale_factor: f32,
    focused_frame_interval: Duration,
    unfocused_frame_interval: Duration,
    next_redraw_at: Instant,
    last_frame: Instant,
    modifiers: ModifiersState,
    is_window_focused: bool,
}

impl MainApp {
    /// Setup everything: logging, config, web server, window, wgpu, ImGui.
    pub async fn initialize(
        event_loop: &EventLoop<()>,
        open_path: Option<PathBuf>,
        reset_config: bool,
    ) -> anyhow::Result<Self> {
        infra::logging::init();

        let config_handle = infra::config::load_or_create(reset_config)
            .context("unable to prepare application configuration")?;

        if reset_config {
            log::info!("--reset-config was set; configuration reset to bundled defaults");
        }

        log::info!("Loaded configuration from {}", config_handle.path.display());
        let mut app_state = ViewerState::new(config_handle.path, config_handle.settings);

        let mut webserver_state: Option<WebServerState> = None;
        if app_state.config().http_port > 0 {
            webserver_state = spawn_web_server(&app_state);
            if webserver_state.is_none() {
                log::error!(
                    "Failed to start HTTP server on port {}",
                    app_state.config().http_port
                );
            }
        }

        // Validate FPS settings
        let focused_fps = if app_state.config().focused_fps > 0 {
            app_state.config().focused_fps
        } else {
            log::warn!(
                "Invalid focused_fps ({}). Falling back to {}",
                app_state.config().focused_fps,
                constants::DEFAULT_FOCUSED_FPS
            );
            constants::DEFAULT_FOCUSED_FPS
        };
        let unfocused_fps = if app_state.config().unfocused_fps > 0 {
            app_state.config().unfocused_fps
        } else {
            log::warn!(
                "Invalid unfocused_fps ({}). Falling back to {}",
                app_state.config().unfocused_fps,
                constants::DEFAULT_UNFOCUSED_FPS
            );
            constants::DEFAULT_UNFOCUSED_FPS
        };

        let icon = load_window_icon();
        if icon.is_none() {
            log::warn!("Failed to load window icon");
        }

        let window = Arc::new(
            WindowBuilder::new()
                .with_title(APPLICATION)
                .with_window_icon(icon)
                .with_inner_size(LogicalSize::new(1280.0, 800.0))
                .with_resizable(true)
                .build(event_loop)
                .map_err(anyhow::Error::msg)
                .context("failed to create window")?,
        );

        let instance = make_instance();
        let surface = instance
            .create_surface(window.clone())
            .context("failed to create wgpu surface")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("failed to request wgpu adapter")?;

        let adapter_limits = adapter.limits();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("image-viewer device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits {
                    max_texture_dimension_2d: adapter_limits.max_texture_dimension_2d,
                    ..wgpu::Limits::downlevel_defaults()
                },
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("failed to request wgpu device")?;

        app_state.set_thumbnail_gpu(&device, &queue);
        if let Some(open_path) = open_path {
            app_state
                .open_path_argument(open_path)
                .context("failed to open PATH argument")?;
        } else {
            app_state.restore_saved_directories();
        }

        let surface_config = create_surface_config(&surface, &adapter, window.inner_size())
            .context("failed to configure surface")?;
        surface.configure(&device, &surface_config);

        let mut imgui = ImguiContext::create();
        imgui.set_ini_filename(None);
        let imgui_theme = app_state.config().imgui_theme;
        apply_imgui_theme(imgui.style_mut(), imgui_theme);
        log::info!("Applied ImGui {:?} theme", imgui_theme);

        let ui_scale_factor = if app_state.config().ui_scale_factor > 0.0 {
            app_state.config().ui_scale_factor
        } else {
            log::warn!(
                "Invalid ui_scale_factor ({}). Falling back to {}",
                app_state.config().ui_scale_factor,
                constants::DEFAULT_UI_SCALE_FACTOR
            );
            constants::DEFAULT_UI_SCALE_FACTOR
        };

        let mut platform = WinitPlatform::init(&mut imgui);
        platform.attach_window(imgui.io_mut(), window.as_ref(), HiDpiMode::Default);

        let hidpi_factor = window.scale_factor() as f32;
        setup_fonts(&mut imgui, &app_state, ui_scale_factor, hidpi_factor);

        let renderer_config = RendererConfig {
            texture_format: surface_config.format,
            ..RendererConfig::default()
        };
        let mut renderer = imgui_wgpu::Renderer::new(&mut imgui, &device, &queue, renderer_config);
        let imgui_textures = ImguiTextures::new();
        let texture_atlas = TextureAtlasManager::new(2048);
        let app_resources = AppResources::new(&device, &queue, &mut renderer)
            .context("failed to initialize global app resources")?;

        let mut media_cache_count = app_state.config().media_cache_count;
        if media_cache_count == 0 {
            log::warn!(
                "Invalid media_cache_count ({}). Falling back to {}",
                media_cache_count,
                constants::DEFAULT_MEDIA_CACHE_COUNT
            );
            media_cache_count = constants::DEFAULT_MEDIA_CACHE_COUNT;
        }

        let max_texture_size = device.limits().max_texture_dimension_2d;
        let media_manager = MediaManager::new(media_cache_count);
        let image_uploader = ImageUploader::new(max_texture_size);

        log::info!(
            "TextureManager created with max_texture_size: {}, media_cache_count: {}",
            max_texture_size,
            media_cache_count
        );

        Ok(Self {
            app_state,
            view_panel: ViewPanel::new(),
            webserver_state,
            window,
            _instance: instance,
            surface,
            device,
            queue,
            surface_config,
            imgui,
            platform,
            renderer,
            imgui_textures,
            texture_atlas,
            app_resources,
            media_manager,
            image_uploader,
            ui_scale_factor,
            focused_frame_interval: frame_interval_from_fps(focused_fps),
            unfocused_frame_interval: frame_interval_from_fps(unfocused_fps),
            next_redraw_at: Instant::now(),
            last_frame: Instant::now(),
            modifiers: ModifiersState::default(),
            is_window_focused: true,
        })
    }

    /// Run the OS event loop until the user quits. Consumes the app.
    pub fn run(mut self, event_loop: EventLoop<()>) -> anyhow::Result<()> {
        block_in_place(|| {
            event_loop.run(move |event, window_target| {
                self.handle_event(event, window_target);
            })
        })
        .map_err(anyhow::Error::msg)
    }

    fn handle_event(&mut self, event: Event<()>, window_target: &EventLoopWindowTarget<()>) {
        // Ignore queued redraws after cleanup has released the textures.
        if window_target.exiting() {
            return;
        }

        // Let ImGui/winit helper see every event (mouse, keyboard, etc.).
        self.platform
            .handle_event(self.imgui.io_mut(), self.window.as_ref(), &event);

        match event {
            // New batch of events from OS just started.
            Event::NewEvents(_) => {
                let now = Instant::now();
                // Update delta time for ImGui (time between frames).
                self.imgui.io_mut().update_delta_time(now - self.last_frame);
                self.last_frame = now;
            }
            // Event loop is about to sleep; good place to decide when to wake up.
            Event::AboutToWait => self.update(window_target),
            // Other window events for our window (close, resize, keyboard, etc.).
            Event::WindowEvent { window_id, event } if window_id == self.window.id() => {
                self.handle_window_event(event, window_target);
            }
            _ => {}
        }
    }

    /// Per-frame update: frame pacing, image decode pipeline, web server state.
    fn update(&mut self, window_target: &EventLoopWindowTarget<()>) {
        let now = Instant::now();
        // If it is not time to redraw yet, sleep until next_redraw_at.
        if now < self.next_redraw_at {
            window_target.set_control_flow(ControlFlow::WaitUntil(self.next_redraw_at));
            return;
        }
        // Choose slower FPS when window is unfocused to save resources.
        let frame_interval = if self.is_window_focused {
            self.focused_frame_interval
        } else {
            self.unfocused_frame_interval
        };
        self.next_redraw_at = now + frame_interval;
        window_target.set_control_flow(ControlFlow::WaitUntil(self.next_redraw_at));

        // Route reloads to the selected media type.
        if self.app_state.take_reload_request() {
            self.view_panel.request_reload();
            if let Some(entry) = self.app_state.current_entry().filter(|entry| {
                entry.format.media_type() == MediaType::Image2D && !self.app_state.is_multi_select()
            }) {
                self.image_uploader
                    .request_decode(&entry.path, &mut self.media_manager);
            } else {
                // Model views and thumbnail grids do not need an image decode.
                self.image_uploader.cancel_pending();
                if let Some(texture_id) = self.app_state.current_texture().map(|texture| texture.id)
                {
                    self.image_uploader.release_texture(
                        &mut self.renderer,
                        &mut self.imgui_textures,
                        texture_id,
                    );
                }
                self.app_state.set_current_texture(None);
            }
        }

        if let Some(webserver) = &self.webserver_state {
            infra::web_server::set_current_directory(
                &webserver.shared_state,
                self.app_state.current_directory().map(PathBuf::from),
            );
            infra::web_server::set_selected_file(
                &webserver.shared_state,
                self.app_state
                    .current_entry()
                    .map(|entry| entry.path.clone()),
            );
        }

        // Poll background decode result and upload to GPU when ready.
        if let Some((decoded_path, uploaded)) = self.image_uploader.poll_decoded(
            &self.device,
            &self.queue,
            &mut self.renderer,
            &mut self.imgui_textures,
            &mut self.media_manager,
        ) {
            // Only apply if the decoded image still matches the current selection.
            let is_current = self.app_state.current_entry().is_some_and(|entry| {
                entry.path == decoded_path
                    && entry.format.media_type() == MediaType::Image2D
                    && !self.app_state.is_multi_select()
            });
            if is_current {
                if let Some(existing_texture) = self.app_state.current_texture() {
                    self.image_uploader.release_texture(
                        &mut self.renderer,
                        &mut self.imgui_textures,
                        existing_texture.id,
                    );
                }
                self.image_uploader.activate_texture(
                    &mut self.renderer,
                    &mut self.imgui_textures,
                    uploaded.id,
                );
                self.app_state.set_current_texture(Some(uploaded));
            } else {
                self.image_uploader.release_texture(
                    &mut self.renderer,
                    &mut self.imgui_textures,
                    uploaded.id,
                );
                log::debug!("Discarding stale decode result: {}", decoded_path.display());
            }
        }

        // Prepare ImGui frame (may fail if window is minimized, etc.).
        if let Err(err) = self
            .platform
            .prepare_frame(self.imgui.io_mut(), self.window.as_ref())
        {
            log::error!("prepare_frame failed: {err}");
            return;
        }
        // Ask OS to trigger a redraw event.
        self.window.request_redraw();

        // Free atlas slots of thumbnails whose files were deleted or modified.
        for id in self.app_state.take_atlas_removals() {
            self.texture_atlas
                .remove_image(&mut self.renderer, &mut self.imgui_textures, id);
        }

        let results = self.app_state.poll_thumbnail_results();
        for result in results {
            self.app_state.apply_thumbnail_info(
                result,
                &self.device,
                &self.queue,
                &mut self.renderer,
                &mut self.imgui_textures,
                &mut self.texture_atlas,
            );
        }
    }

    fn handle_window_event(
        &mut self,
        event: WindowEvent,
        window_target: &EventLoopWindowTarget<()>,
    ) {
        match event {
            // User clicked close button or OS asked us to close.
            WindowEvent::CloseRequested => {
                log::info!("CloseRequested");
                self.cleanup();
                window_target.exit();
            }
            // User dropped a file onto the window.
            WindowEvent::DroppedFile(path) => {
                self.app_state.handle_drop_path(path.as_path());
            }
            // Modifier keys (Ctrl, Shift, Alt, Super) state changed.
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();
            }
            // Handle key presses (no auto-repeat, only when main window is focused and no popup is open).
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && self.is_window_focused
                    && !self.app_state.show_keyboard_shortcuts()
                    && !self.app_state.show_bookmark_window()
                    && !self.app_state.show_selection_window() =>
            {
                self.handle_key_press(&event);
            }
            // Window size changed (user resized or system DPI change, etc.).
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    self.surface_config.width = new_size.width;
                    self.surface_config.height = new_size.height;
                    self.surface.configure(&self.device, &self.surface_config);
                }
            }
            // Monitor scale factor (DPI) changed.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if scale_factor > 0.0 {
                    // Keep ImGui UI size visually similar after DPI change.
                    self.imgui.io_mut().font_global_scale =
                        compute_font_global_scale(self.ui_scale_factor, scale_factor as f32);
                }
                let new_size = self.window.inner_size();
                if new_size.width > 0 && new_size.height > 0 {
                    self.surface_config.width = new_size.width;
                    self.surface_config.height = new_size.height;
                    self.surface.configure(&self.device, &self.surface_config);
                }
            }
            // Window focus gained or lost.
            WindowEvent::Focused(focused) => {
                self.is_window_focused = focused;
                // Reset next redraw time so we update immediately.
                self.next_redraw_at = Instant::now();
                if focused {
                    // When refocused, request an immediate redraw.
                    self.window.request_redraw();
                }
            }
            // Handle actual drawing when the window says it needs a redraw.
            WindowEvent::RedrawRequested => {
                self.render(window_target);
            }
            _ => {}
        }
    }

    fn handle_key_press(&mut self, event: &KeyEvent) {
        match event.physical_key {
            // ESC clears current image selection.
            PhysicalKey::Code(KeyCode::Escape) => {
                self.app_state.clear_image_selection_state();
            }
            // Ctrl+O or Cmd+O opens directory dialog.
            PhysicalKey::Code(KeyCode::KeyO)
                if self.modifiers.control_key() || self.modifiers.super_key() =>
            {
                self.app_state.open_directory_dialog();
            }
            // ArrowRight: move by one item.
            PhysicalKey::Code(KeyCode::ArrowRight) => {
                self.navigate(1);
            }
            // ArrowDown: move by one visual row in the library.
            PhysicalKey::Code(KeyCode::ArrowDown) => {
                let step = self.app_state.library_items_per_row() as i32;
                self.navigate(step);
            }
            // PageDown: go to next 10 images.
            PhysicalKey::Code(KeyCode::PageDown) => {
                self.navigate(10);
            }
            // ArrowLeft: move by one item.
            PhysicalKey::Code(KeyCode::ArrowLeft) => {
                self.navigate(-1);
            }
            // ArrowUp: move by one visual row in the library.
            PhysicalKey::Code(KeyCode::ArrowUp) => {
                let step = self.app_state.library_items_per_row() as i32;
                self.navigate(-step);
            }
            // PageUp: go to previous 10 images.
            PhysicalKey::Code(KeyCode::PageUp) => {
                self.navigate(-10);
            }
            // Home: go to first image.
            PhysicalKey::Code(KeyCode::Home) => {
                self.navigate_to(0);
            }
            // End: go to last image.
            PhysicalKey::Code(KeyCode::End) => {
                let total = self.app_state.media_items().len();
                if total > 0 {
                    self.navigate_to(total - 1);
                }
            }
            // Space: collapse a multi-selection down to the focused file.
            PhysicalKey::Code(KeyCode::Space) => {
                self.app_state.collapse_selection_to_cursor();
            }
            _ => {}
        }
    }

    /// Move by `delta`. When several files are selected the keyboard only moves
    /// the cursor; otherwise it changes the single selection.
    fn navigate(&mut self, delta: i32) {
        if self.app_state.is_multi_select() {
            self.app_state.move_cursor(delta);
        } else {
            self.app_state.advance_selection(delta);
        }
    }

    /// Jump to an absolute index, following the same multi-select rule as
    /// [`Self::navigate`].
    fn navigate_to(&mut self, index: usize) {
        if self.app_state.is_multi_select() {
            self.app_state.move_cursor_to(index);
        } else {
            self.app_state.select_index(index);
        }
    }

    /// Render one frame: build the ImGui UI and draw it to the surface.
    fn render(&mut self, window_target: &EventLoopWindowTarget<()>) {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(SurfaceError::Lost) | Err(SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return;
            }
            Err(SurfaceError::OutOfMemory) => {
                log::error!("Surface out of memory; exiting");
                self.cleanup();
                window_target.exit();
                return;
            }
            Err(SurfaceError::Timeout) => {
                return;
            }
            Err(SurfaceError::Other) => {
                log::warn!("Surface returned an unspecified error; retrying next frame");
                return;
            }
        };

        // Create view into the current frame's texture.
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.view_panel.prepare_frame(
            &self.app_state,
            &mut ViewGpuContext {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.renderer,
            },
        );

        // Build ImGui UI for this frame.
        let ui = self.imgui.frame();
        let mut running = true;
        let mut reload_settings = false;
        let is_pending = self.image_uploader.is_pending() || self.view_panel.is_pending();
        let http_port = self.webserver_state.as_ref().and_then(|server| {
            (server.listening.load(Ordering::Acquire) && !server.server_handle.is_finished())
                .then_some(self.app_state.config().http_port)
        });
        render_ui(
            ui,
            &mut self.app_state,
            &mut self.view_panel,
            is_pending,
            http_port,
            &self.app_resources,
            &mut running,
            &mut reload_settings,
        );

        if !running {
            self.cleanup();
            window_target.exit();
            return;
        }

        // Tell ImGui/winit helper we are ready to render.
        self.platform.prepare_render(ui, self.window.as_ref());
        // Get draw lists from ImGui.
        let draw_data = self.imgui.render();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("image-viewer encoder"),
            });

        // Model passes must finish before ImGui samples their textures.
        self.view_panel.encode_frame(&mut encoder);

        let (background_color1, _) = self
            .app_state
            .config()
            .background_style
            .resolved_colors_rgb();

        {
            // Begin a render pass to clear screen and draw ImGui.
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("image-viewer render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(background_color1[0]),
                            g: f64::from(background_color1[1]),
                            b: f64::from(background_color1[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            // Render ImGui draw commands into the current frame.
            if let Err(err) = self
                .renderer
                .render(draw_data, &self.queue, &self.device, &mut rpass)
            {
                log::error!("imgui render failed: {err}");
            }
        }

        // Submit GPU commands and present the frame to the screen.
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        if reload_settings {
            self.reload_settings();
        }
    }

    fn reload_settings(&mut self) {
        let config = match infra::config::load(self.app_state.config_path()) {
            Ok(config) => config,
            Err(err) => {
                log::error!("Failed to reload settings: {err:#}");
                self.app_state.set_status_message(format!("Failed to reload settings: {err:#}"));
                return;
            }
        };
        let old = self.app_state.config();
        let font_changed = old.ui_font_filename != config.ui_font_filename
            || old.ui_font_size_pt != config.ui_font_size_pt;
        let port_changed = old.http_port != config.http_port;
        let cache_changed = old.media_cache_count != config.media_cache_count;
        self.app_state.apply_config(config);
        let config = self.app_state.config();
        apply_imgui_theme(self.imgui.style_mut(), config.imgui_theme);
        self.ui_scale_factor = if config.ui_scale_factor.is_finite() && config.ui_scale_factor > 0.0 {
            config.ui_scale_factor
        } else {
            constants::DEFAULT_UI_SCALE_FACTOR
        };
        self.focused_frame_interval = frame_interval_from_fps(if config.focused_fps > 0 { config.focused_fps } else { constants::DEFAULT_FOCUSED_FPS });
        self.unfocused_frame_interval = frame_interval_from_fps(if config.unfocused_fps > 0 { config.unfocused_fps } else { constants::DEFAULT_UNFOCUSED_FPS });
        if font_changed {
            let font_texture = self.imgui.fonts().tex_id;
            self.imgui.fonts().clear();
            self.imgui.fonts().tex_id = font_texture;
            setup_fonts(&mut self.imgui, &self.app_state, self.ui_scale_factor, self.window.scale_factor() as f32);
            self.renderer.reload_font_texture(&mut self.imgui, &self.device, &self.queue);
        } else {
            self.imgui.io_mut().font_global_scale = compute_font_global_scale(self.ui_scale_factor, self.window.scale_factor() as f32);
        }
        if cache_changed {
            let capacity = if config.media_cache_count > 0 { config.media_cache_count } else { constants::DEFAULT_MEDIA_CACHE_COUNT };
            self.media_manager = MediaManager::new(capacity);
        }
        if port_changed {
            if let Some(server) = self.webserver_state.take() {
                server.shutdown_token.cancel();
                server.server_handle.abort();
            }
            self.webserver_state = spawn_web_server(&self.app_state);
        }
        self.app_state.set_status_message("Settings reloaded".to_owned());
    }

    /// Release all resources and persist state. Called once before exit.
    fn cleanup(&mut self) {
        if let Some(state) = &self.webserver_state {
            log::info!("Shutting down HTTP server...");
            state.shutdown_token.cancel();
            state.server_handle.abort();
        }

        self.app_state.flush_bookmarks_if_dirty();
        save_config_on_exit(&mut self.app_state);
        self.media_manager.clear();
        self.view_panel.release(&mut ViewGpuContext {
            device: &self.device,
            queue: &self.queue,
            renderer: &mut self.renderer,
        });
        self.image_uploader
            .clear(&mut self.renderer, &mut self.imgui_textures);
        self.texture_atlas
            .clear(&mut self.renderer, &mut self.imgui_textures);
        self.app_resources.release(&mut self.renderer);
        self.imgui_textures.clear(&mut self.renderer);
    }
}

fn frame_interval_from_fps(fps: u32) -> Duration {
    debug_assert!(fps > 0);
    Duration::from_secs_f64(1.0 / f64::from(fps))
}

fn points_to_logical_pixels(points: f32) -> f32 {
    points * (LOGICAL_DPI / POINTS_PER_INCH)
}

fn compute_font_global_scale(ui_scale_factor: f32, hidpi_factor: f32) -> f32 {
    if hidpi_factor > 0.0 {
        ui_scale_factor / hidpi_factor
    } else {
        ui_scale_factor
    }
}

fn apply_imgui_theme(style: &mut imgui::Style, theme: AppTheme) {
    match theme {
        AppTheme::Dark => {
            style.use_dark_colors();
        }
        AppTheme::Light => {
            style.use_light_colors();
        }
        AppTheme::Classic => {
            style.use_classic_colors();
        }
    }
}

/// On Windows, prefer DX12 first -> fallback to automatic(ALL) if failed
fn make_instance() -> wgpu::Instance {
    if cfg!(target_os = "windows") {
        // 1) Try to specify the preferred backend first
        let windows_instance = Instance::new(&InstanceDescriptor {
            backends: Backends::DX12,
            ..Default::default()
        });
        // Note: Instance creation itself is not usually a failure, but
        // actual failures often occur when requesting adapters/devices.
        windows_instance
    } else {
        // Non-Windows: default(automatic)
        Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        })
    }
}

/// Configure ImGui fonts: custom font if found, otherwise the built-in default.
fn setup_fonts(
    imgui: &mut ImguiContext,
    app_state: &ViewerState,
    ui_scale_factor: f32,
    hidpi_factor: f32,
) {
    let ui_font_filename = app_state.config().ui_font_filename.clone();
    let ui_font_size_pt = if app_state.config().ui_font_size_pt > 0.0 {
        app_state.config().ui_font_size_pt
    } else {
        log::warn!(
            "Invalid ui_font_size_pt ({}). Falling back to {}",
            app_state.config().ui_font_size_pt,
            constants::DEFAULT_UI_FONT_SIZE_PT
        );
        constants::DEFAULT_UI_FONT_SIZE_PT
    };

    let font_scale = compute_font_global_scale(ui_scale_factor, hidpi_factor);
    let ui_font_size_logical_px = points_to_logical_pixels(ui_font_size_pt);
    let framebuffer_font_size = ui_font_size_logical_px * hidpi_factor.max(1.0);

    imgui.io_mut().font_global_scale = font_scale;
    log::info!(
        "Detected DPI scale: {:.2}, ui_scale_factor: {:.2}, effective font_global_scale: {:.2}, ui_font_size_pt: {:.2}",
        hidpi_factor,
        ui_scale_factor,
        font_scale,
        ui_font_size_pt
    );

    // Load custom font from:
    // 1) assets/fonts/
    // 2) config directory root
    // 3) config directory fonts/ subdirectory
    let bundled_font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("fonts")
        .join(&ui_font_filename);
    let config_dir = infra::config::config_dir().ok();
    let mut font_candidates = vec![bundled_font_path];
    if let Some(config_dir) = config_dir {
        font_candidates.push(config_dir.join(&ui_font_filename));
        font_candidates.push(config_dir.join("fonts").join(&ui_font_filename));
    }
    let font_path = if ui_font_filename.is_empty() {
        None
    } else {
        font_candidates.iter().find(|path| path.exists()).cloned()
    };

    let mut custom_font_added = false;
    if let Some((font_path, font_data)) = font_path.and_then(|path| match std::fs::read(&path) {
        Ok(data) => Some((path, data)),
        Err(err) => {
            log::warn!("Failed to read font {}: {err}", path.display());
            None
        }
    }) {
        // Leak the data so it lives for the entire program lifetime.
        // imgui requires the font data slice to live as long as the context.
        let font_data: &'static [u8] = Box::leak(font_data.into_boxed_slice());
        // Convert pt -> logical px (96 DPI) -> framebuffer px.
        // This keeps font sizing stable across font changes and DPI.
        imgui.fonts().add_font(&[FontSource::TtfData {
            data: font_data,
            size_pixels: framebuffer_font_size,
            config: Some(FontConfig {
                glyph_ranges: FontGlyphRanges::from_slice(&[
                    // Basic Latin + Latin Supplement
                    0x0020, 0x00FF, // Korean (Hangul Syllables)
                    0xAC00, 0xD7A3, // Korean (Hangul Jamo)
                    0x1100, 0x11FF, // Korean (Hangul Compatibility Jamo)
                    0x3130, 0x318F, // CJK Unified Ideographs (common Hanja and Kanji)
                    0x4E00, 0x9FFF, // Japanese punctuation, Hiragana, and Katakana
                    0x3000, 0x30FF, // Katakana Phonetic Extensions
                    0x31F0, 0x31FF, // Half-width Katakana and full-width forms
                    0xFF00, 0xFFEF, // Null terminator
                    0,
                ]),
                ..FontConfig::default()
            }),
        }]);
        custom_font_added = true;
        log::info!(
            "Custom font loaded: {} ({:.2} pt -> {:.2} logical px -> {:.2} framebuffer px, scale: {:.2})",
            font_path.display(),
            ui_font_size_pt,
            ui_font_size_logical_px,
            framebuffer_font_size,
            hidpi_factor
        );
    } else {
        let checked_paths = font_candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        log::warn!(
            "Custom font not found. checked paths: {}. using default imgui font",
            checked_paths
        );
    }

    if !custom_font_added {
        imgui.fonts().add_font(&[FontSource::DefaultFontData {
            config: Some(FontConfig {
                size_pixels: framebuffer_font_size,
                ..FontConfig::default()
            }),
        }]);
        log::info!(
            "Configured ImGui default font ({:.2} pt -> {:.2} logical px -> {:.2} framebuffer px)",
            ui_font_size_pt,
            ui_font_size_logical_px,
            framebuffer_font_size
        );
    }
}

fn spawn_web_server(app_state: &ViewerState) -> Option<WebServerState> {
    if app_state.config().http_port > 0 {
        let port = app_state.config().http_port;
        let shutdown_token = CancellationToken::new();
        let shared_state = infra::web_server::new_shared_state();
        let listening = Arc::new(AtomicBool::new(false));

        infra::web_server::set_current_directory(
            &shared_state,
            app_state.current_directory().map(PathBuf::from),
        );
        infra::web_server::set_selected_file(
            &shared_state,
            app_state.current_entry().map(|entry| entry.path.clone()),
        );

        let server_handle = infra::web_server::start(
            port,
            shutdown_token.clone(),
            shared_state.clone(),
            listening.clone(),
        );
        log::info!("HTTP server requested on 127.0.0.1:{port}");

        Some(WebServerState {
            shutdown_token,
            shared_state,
            server_handle,
            listening,
        })
    } else {
        None
    }
}

fn create_surface_config(
    surface: &Surface<'_>,
    adapter: &wgpu::Adapter,
    size: winit::dpi::PhysicalSize<u32>,
) -> anyhow::Result<SurfaceConfiguration> {
    let capabilities = surface.get_capabilities(adapter);
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .or_else(|| capabilities.formats.first().copied())
        .context("surface supports no texture formats")?;

    let present_mode = if capabilities
        .present_modes
        .contains(&wgpu::PresentMode::Fifo)
    {
        wgpu::PresentMode::Fifo
    } else {
        *capabilities
            .present_modes
            .first()
            .context("surface supports no present modes")?
    };

    let alpha_mode = capabilities
        .alpha_modes
        .iter()
        .copied()
        .find(|mode| *mode == CompositeAlphaMode::Auto)
        .unwrap_or_else(|| capabilities.alpha_modes[0]);

    Ok(SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width.max(1),
        height: size.height.max(1),
        desired_maximum_frame_latency: 2,
        present_mode,
        alpha_mode,
        view_formats: vec![],
    })
}

fn save_config_on_exit(app_state: &ViewerState) {
    if let Err(err) = infra::config::save(app_state.config_path(), app_state.config()) {
        log::error!("failed to persist application configuration: {err:#}");
    }
}

fn load_window_icon() -> Option<Icon> {
    // 런타임 파일로 로드해도 되고, 배포 편하게 include_bytes!로 박아도 됨.
    let bytes = include_bytes!("../assets/icon.png");

    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}
