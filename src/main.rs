#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod core;
mod infra;
mod math;
mod render;
mod ui;

use anyhow::{bail, Context};
use app::ViewerState;
use imgui::{Context as ImguiContext, FontConfig, FontGlyphRanges, FontSource};
use imgui_wgpu::RendererConfig;
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::block_in_place;
use wgpu::{Backends, Instance, InstanceDescriptor};
use wgpu::{CompositeAlphaMode, Surface, SurfaceConfiguration, SurfaceError};
use winit::{
    dpi::LogicalSize,
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{Icon, WindowBuilder},
};

use crate::core::image_manager::ImageManager;
use crate::render::app_resources::AppResources;
use crate::render::imgui_textures::ImguiTextures;
use crate::render::texture_atlas_manager::TextureAtlasManager;
use crate::render::image_uploader::ImageUploader;
use crate::ui::render_ui;

const FOCUSED_FPS: u32 = 60;
const UNFOCUSED_FPS: u32 = 5;
const LOGICAL_DPI: f32 = 96.0;
const POINTS_PER_INCH: f32 = 72.0;

#[derive(Debug, Default)]
struct AppArgs {
    reset_config: bool,
    open_path: Option<PathBuf>,
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

fn parse_args() -> anyhow::Result<AppArgs> {
    let mut args = AppArgs::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--reset-config" => args.reset_config = true,
            "-h" | "--help" => {
                println!("Usage: image-viewer [--reset-config] [PATH]");
                println!("  --reset-config  overwrite saved settings with default_settings.toml");
                println!("  PATH            image file path (single-file mode) or directory path");
                std::process::exit(0);
            }
            _ => {
                if arg.starts_with('-') {
                    bail!("unknown argument: {arg}\nUsage: image-viewer [--reset-config] [PATH]");
                }
                if args.open_path.is_some() {
                    bail!(
                        "only one PATH argument is supported\nUsage: image-viewer [--reset-config] [PATH]"
                    );
                }
                args.open_path = Some(PathBuf::from(arg));
            }
        }
    }
    Ok(args)
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

/// App entrypoint: setup systems, run UI loop, then save config.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args().context("failed to parse command line arguments")?;

    infra::logging::init();

    let config_handle = infra::config::load_or_create(args.reset_config)
        .context("unable to prepare application configuration")?;

    if args.reset_config {
        log::info!("--reset-config was set; configuration reset to bundled defaults");
    }

    log::info!("Loaded configuration from {}", config_handle.path.display());

    let mut app_state = ViewerState::new(config_handle.path, config_handle.settings);
    if let Some(open_path) = args.open_path {
        app_state
            .open_path_argument(open_path)
            .context("failed to open PATH argument")?;
    } else {
        restore_last_directory_if_needed(&mut app_state);
    }

    let event_loop = EventLoop::new().map_err(anyhow::Error::msg)?;

    let icon = load_window_icon();
    if icon.is_none() {
        log::warn!("Failed to load window icon");
    }

    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Vibe Image Viewer")
            .with_window_icon(icon)
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_resizable(true)
            .build(&event_loop)
            .map_err(anyhow::Error::msg)
            .context("failed to create window")?,
    );

    let instance = make_instance();
    // let instance = wgpu::Instance::default();
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
            // required_limits: wgpu::Limits::default(),
            required_limits: wgpu::Limits {
                max_texture_dimension_2d: adapter_limits.max_texture_dimension_2d,
                ..wgpu::Limits::downlevel_defaults()
            },
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })
        .await
        .context("failed to request wgpu device")?;

    let mut surface_config = create_surface_config(&surface, &adapter, window.inner_size())
        .context("failed to configure surface")?;
    surface.configure(&device, &surface_config);

    let mut imgui = ImguiContext::create();
    imgui.set_ini_filename(None);
    imgui.style_mut().use_dark_colors();

    let ui_font_filename = app_state.config().ui_font_filename.clone();
    let ui_font_size_pt = if app_state.config().ui_font_size_pt > 0.0 {
        app_state.config().ui_font_size_pt
    } else {
        log::warn!(
            "Invalid ui_font_size_pt ({}). Falling back to 10.5",
            app_state.config().ui_font_size_pt
        );
        10.5
    };

    let ui_scale_factor = if app_state.config().ui_scale_factor > 0.0 {
        app_state.config().ui_scale_factor
    } else {
        log::warn!(
            "Invalid ui_scale_factor ({}). Falling back to 1.0",
            app_state.config().ui_scale_factor
        );
        1.0
    };

    let mut platform = WinitPlatform::init(&mut imgui);
    platform.attach_window(imgui.io_mut(), window.as_ref(), HiDpiMode::Default);

    let hidpi_factor = window.scale_factor() as f32;
    let font_scale = compute_font_global_scale(ui_scale_factor, hidpi_factor);
    let ui_font_size_logical_px = points_to_logical_pixels(ui_font_size_pt);

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

    if let Some(font_path) = font_path {
        let font_data = std::fs::read(&font_path).expect("failed to read custom font file");
        // Leak the data so it lives for the entire program lifetime.
        // imgui requires the font data slice to live as long as the context.
        let font_data: &'static [u8] = Box::leak(font_data.into_boxed_slice());
        // Convert pt -> logical px (96 DPI) -> framebuffer px.
        // This keeps font sizing stable across font changes and DPI.
        let font_size = ui_font_size_logical_px * hidpi_factor.max(1.0);

        imgui.fonts().add_font(&[FontSource::TtfData {
            data: font_data,
            size_pixels: font_size,
            config: Some(FontConfig {
                glyph_ranges: FontGlyphRanges::from_slice(&[
                    // Basic Latin + Latin Supplement
                    0x0020, 0x00FF, // Korean (Hangul Syllables)
                    0xAC00, 0xD7A3, // Korean (Hangul Jamo)
                    0x1100, 0x11FF, // Korean (Hangul Compatibility Jamo)
                    0x3130, 0x318F, // CJK Unified Ideographs (common Hanja)
                    0x4E00, 0x9FFF, // Null terminator
                    0,
                ]),
                ..FontConfig::default()
            }),
        }]);
        log::info!(
            "Custom font loaded: {} ({:.2} pt -> {:.2} logical px -> {:.2} framebuffer px, scale: {:.2})",
            font_path.display(),
            ui_font_size_pt,
            ui_font_size_logical_px,
            font_size,
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

    let renderer_config = RendererConfig {
        texture_format: surface_config.format,
        ..RendererConfig::default()
    };
    let mut renderer = imgui_wgpu::Renderer::new(&mut imgui, &device, &queue, renderer_config);
    let mut imgui_textures = ImguiTextures::new();
    let mut texture_atlas = TextureAtlasManager::new(2048);
    let mut app_resources = AppResources::new(&device, &queue, &mut renderer)
        .context("failed to initialize global app resources")?;

    let mut max_cache_size = app_state.config().texture_cache_max_entries;
    if max_cache_size == 0 {
        log::warn!(
            "Invalid texture_cache_max_entries ({}). Falling back to 16",
            max_cache_size
        );
        max_cache_size = 16;
    }

    let mut image_cache_count = app_state.config().image_cache_count;
    if image_cache_count == 0 {
        log::warn!(
            "Invalid image_cache_count ({}). Falling back to 32",
            image_cache_count
        );
        image_cache_count = 32;
    }

    let max_texture_size = device.limits().max_texture_dimension_2d;
    let mut image_manager = ImageManager::new(image_cache_count);
    let mut image_uploader = ImageUploader::new(max_texture_size, max_cache_size);

    log::info!(
        "TextureManager created with max_texture_size: {}, max_texture_cache_size: {}, image_cache_count: {}",
        max_texture_size,
        max_cache_size,
        image_cache_count
    );

    let mut last_frame = Instant::now();
    let mut modifiers = ModifiersState::default();
    let mut is_window_focused = true;
    let focused_frame_interval = frame_interval_from_fps(FOCUSED_FPS);
    let unfocused_frame_interval = frame_interval_from_fps(UNFOCUSED_FPS);
    let mut next_redraw_at = Instant::now();

    let _instance = instance;
    // Main event loop: handle OS/window events and drive rendering.
    block_in_place(|| {
        event_loop.run(move |event, window_target| {
            //
            // Let ImGui/winit helper see every event (mouse, keyboard, etc.).
            platform.handle_event(imgui.io_mut(), window.as_ref(), &event);

            match event {
                // New batch of events from OS just started.
                Event::NewEvents(_) => {
                    let now = Instant::now();
                    // Update delta time for ImGui (time between frames).
                    imgui.io_mut().update_delta_time(now - last_frame);
                    last_frame = now;
                }
                // Event loop is about to sleep; good place to decide when to wake up.
                Event::AboutToWait => {
                    let now = Instant::now();
                    // If it is not time to redraw yet, sleep until next_redraw_at.
                    if now < next_redraw_at {
                        window_target.set_control_flow(ControlFlow::WaitUntil(next_redraw_at));
                        return;
                    }
                    // Choose slower FPS when window is unfocused to save resources.
                    let frame_interval = if is_window_focused {
                        focused_frame_interval
                    } else {
                        unfocused_frame_interval
                    };
                    next_redraw_at = now + frame_interval;
                    window_target.set_control_flow(ControlFlow::WaitUntil(next_redraw_at));

                    // Reload current image/texture if someone requested it.
                    if app_state.take_reload_request() {
                        if let Some(entry) = app_state.current_entry() {
                            // Fast path: texture already cached in GPU.
                            if let Some(cached) = image_uploader.get_cached(&entry.path) {
                                app_state.set_current_texture(Some(cached));
                            } else {
                                // Slow path: kick off background decode.
                                image_uploader.request_decode(&entry.path, &mut image_manager);
                            }
                        } else {
                            // No image selected (e.g. directory changed). Cancel any stale decode.
                            image_uploader.cancel_pending();
                            app_state.set_current_texture(None);
                        }
                    }

                    // Poll background decode result and upload to GPU when ready.
                    if let Some((decoded_path, uploaded)) = image_uploader.poll_decoded(
                        &device,
                        &queue,
                        &mut renderer,
                        &mut imgui_textures,
                        &mut image_manager,
                    ) {
                        // Only apply if the decoded image still matches the current selection.
                        let is_current = app_state
                            .current_entry()
                            .map_or(false, |e| e.path == decoded_path);
                        if is_current {
                            app_state.set_current_texture(Some(uploaded));
                        } else {
                            log::debug!(
                                "Discarding stale decode result: {}",
                                decoded_path.display()
                            );
                        }
                    }

                    // Prepare ImGui frame (may fail if window is minimized, etc.).
                    if let Err(err) = platform.prepare_frame(imgui.io_mut(), window.as_ref()) {
                        log::error!("prepare_frame failed: {err}");
                        return;
                    }
                    // Ask OS to trigger a redraw event.
                    window.request_redraw();
                    //
                    let results = app_state.poll_thumbnail_results();
                    for result in results {
                        app_state.apply_thumbnail_info(
                            result,
                            &device,
                            &queue,
                            &mut renderer,
                            &mut imgui_textures,
                            &mut texture_atlas,
                        );
                    }
                }
                // Other window events for our window (close, resize, keyboard, etc.).
                Event::WindowEvent { window_id, event } if window_id == window.id() => {
                    match event {
                        // User clicked close button or OS asked us to close.
                        WindowEvent::CloseRequested => {
                            log::info!("CloseRequested");
                            // exit
                            cleanup_on_exit(
                                &app_state,
                                &mut image_manager,
                                &mut image_uploader,
                                &mut texture_atlas,
                                &mut imgui_textures,
                                &mut app_resources,
                                &mut renderer,
                            );
                            window_target.exit();
                        }
                        // User dropped a file onto the window.
                        WindowEvent::DroppedFile(path) => {
                            app_state.handle_drop_path(path.as_path());
                        }
                        // Modifier keys (Ctrl, Shift, Alt, Super) state changed.
                        WindowEvent::ModifiersChanged(new_modifiers) => {
                            modifiers = new_modifiers.state();
                        }
                        // Handle key presses (no auto-repeat, only when main window is focused and no popup is open).
                        WindowEvent::KeyboardInput { event, .. }
                            if event.state == ElementState::Pressed
                                && !event.repeat
                                && is_window_focused
                                && !app_state.show_keyboard_shortcuts()
                                && !app_state.show_selection_window() =>
                        {
                            match event.physical_key {
                                // ESC clears current image selection.
                                PhysicalKey::Code(KeyCode::Escape) => {
                                    app_state.clear_image_selection_state();
                                }
                                // Ctrl+O or Cmd+O opens directory dialog.
                                PhysicalKey::Code(KeyCode::KeyO)
                                    if modifiers.control_key() || modifiers.super_key() =>
                                {
                                    app_state.open_directory_dialog();
                                }
                                // ArrowRight: move by one item.
                                PhysicalKey::Code(KeyCode::ArrowRight) => {
                                    app_state.advance_selection(1);
                                }
                                // ArrowDown: move by one visual row in the library.
                                PhysicalKey::Code(KeyCode::ArrowDown) => {
                                    let step = app_state.library_items_per_row() as i32;
                                    app_state.advance_selection(step);
                                }
                                // PageDown: go to next 10 images.
                                PhysicalKey::Code(KeyCode::PageDown) => {
                                    app_state.advance_selection(10);
                                }
                                // ArrowLeft: move by one item.
                                PhysicalKey::Code(KeyCode::ArrowLeft) => {
                                    app_state.advance_selection(-1);
                                }
                                // ArrowUp: move by one visual row in the library.
                                PhysicalKey::Code(KeyCode::ArrowUp) => {
                                    let step = app_state.library_items_per_row() as i32;
                                    app_state.advance_selection(-step);
                                }
                                // PageUp: go to previous 10 images.
                                PhysicalKey::Code(KeyCode::PageUp) => {
                                    app_state.advance_selection(-10);
                                }
                                // Home: go to first image.
                                PhysicalKey::Code(KeyCode::Home) => {
                                    app_state.select_index(0);
                                }
                                // End: go to last image.
                                PhysicalKey::Code(KeyCode::End) => {
                                    let total = app_state.media_items().len();
                                    if total > 0 {
                                        app_state.select_index(total - 1);
                                    }
                                }
                                _ => {}
                            }
                        }
                        // Window size changed (user resized or system DPI change, etc.).
                        WindowEvent::Resized(new_size) => {
                            if new_size.width > 0 && new_size.height > 0 {
                                surface_config.width = new_size.width;
                                surface_config.height = new_size.height;
                                surface.configure(&device, &surface_config);
                            }
                        }
                        // Monitor scale factor (DPI) changed.
                        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                            if scale_factor > 0.0 {
                                // Keep ImGui UI size visually similar after DPI change.
                                imgui.io_mut().font_global_scale =
                                    compute_font_global_scale(ui_scale_factor, scale_factor as f32);
                            }
                            let new_size = window.inner_size();
                            if new_size.width > 0 && new_size.height > 0 {
                                surface_config.width = new_size.width;
                                surface_config.height = new_size.height;
                                surface.configure(&device, &surface_config);
                            }
                        }
                        // Window focus gained or lost.
                        WindowEvent::Focused(focused) => {
                            is_window_focused = focused;
                            // Reset next redraw time so we update immediately.
                            next_redraw_at = Instant::now();
                            if focused {
                                // When refocused, request an immediate redraw.
                                window.request_redraw();
                            }
                        }
                        // Handle actual drawing when the window says it needs a redraw.
                        WindowEvent::RedrawRequested => {
                            let frame = match surface.get_current_texture() {
                                Ok(frame) => frame,
                                Err(SurfaceError::Lost) | Err(SurfaceError::Outdated) => {
                                    surface.configure(&device, &surface_config);
                                    return;
                                }
                                Err(SurfaceError::OutOfMemory) => {
                                    log::error!("Surface out of memory; exiting");
                                    cleanup_on_exit(
                                        &app_state,
                                        &mut image_manager,
                                        &mut image_uploader,
                                        &mut texture_atlas,
                                        &mut imgui_textures,
                                        &mut app_resources,
                                        &mut renderer,
                                    );
                                    window_target.exit();
                                    return;
                                }
                                Err(SurfaceError::Timeout) => {
                                    return;
                                }
                                Err(SurfaceError::Other) => {
                                    log::warn!(
                                        "Surface returned an unspecified error; retrying next frame"
                                    );
                                    return;
                                }
                            };

                            // Create view into the current frame's texture.
                            let view = frame
                                .texture
                                .create_view(&wgpu::TextureViewDescriptor::default());

                            // Build ImGui UI for this frame.
                            let ui = imgui.frame();
                            let mut running = true;
                            // Clone to avoid moving out of the closure-captured option.
                            render_ui(
                                ui,
                                &mut app_state,
                                image_uploader.is_pending(),
                                &app_resources,
                                &mut running,
                            );

                            if !running {
                                // exit
                                cleanup_on_exit(
                                    &app_state,
                                    &mut image_manager,
                                    &mut image_uploader,
                                    &mut texture_atlas,
                                    &mut imgui_textures,
                                    &mut app_resources,
                                    &mut renderer,
                                );
                                window_target.exit();
                                return;
                            }

                            // Tell ImGui/winit helper we are ready to render.
                            platform.prepare_render(ui, window.as_ref());
                            // Get draw lists from ImGui.
                            let draw_data = imgui.render();

                            let mut encoder =
                                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("image-viewer encoder"),
                                });

                            {
                                // Begin a render pass to clear screen and draw ImGui.
                                let mut rpass =
                                    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                        label: Some("image-viewer render pass"),
                                        color_attachments: &[Some(
                                            wgpu::RenderPassColorAttachment {
                                                view: &view,
                                                resolve_target: None,
                                                ops: wgpu::Operations {
                                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                                        r: 0.08,
                                                        g: 0.09,
                                                        b: 0.11,
                                                        a: 1.0,
                                                    }),
                                                    store: wgpu::StoreOp::Store,
                                                },
                                            },
                                        )],
                                        depth_stencil_attachment: None,
                                        occlusion_query_set: None,
                                        timestamp_writes: None,
                                    });

                                // Render ImGui draw commands into the current frame.
                                if let Err(err) =
                                    renderer.render(draw_data, &queue, &device, &mut rpass)
                                {
                                    log::error!("imgui render failed: {err}");
                                }
                            }

                            // Submit GPU commands and present the frame to the screen.
                            queue.submit(Some(encoder.finish()));
                            frame.present();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        })
    })
    .map_err(anyhow::Error::msg)
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

fn cleanup_on_exit(
    app_state: &ViewerState,
    image_manager: &mut ImageManager,
    image_uploader: &mut ImageUploader,
    texture_atlas: &mut TextureAtlasManager,
    imgui_textures: &mut ImguiTextures,
    app_resources: &mut AppResources,
    renderer: &mut imgui_wgpu::Renderer,
) {
    save_config_on_exit(app_state);
    image_manager.clear();
    image_uploader.clear(renderer, imgui_textures);
    texture_atlas.clear(renderer, imgui_textures);
    app_resources.release(renderer);
}

/// Try to restore the last directory from config.
fn restore_last_directory_if_needed(app_state: &mut ViewerState) {
    if let Some(directory) = app_state.restore_candidate().map(PathBuf::from) {
        if directory.is_dir() {
            app_state.load_directory(directory, None);
        } else {
            log::warn!(
                "Configured last_open_directory is not a directory: {}",
                directory.display()
            );
        }
    }
}

fn load_window_icon() -> Option<Icon> {
    // 런타임 파일로 로드해도 되고, 배포 편하게 include_bytes!로 박아도 됨.
    let bytes = include_bytes!("../assets/icon.png");

    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}
