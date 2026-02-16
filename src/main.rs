#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod core;
mod infra;
mod render;
mod ui;

use anyhow::{Context, bail};
use app::{ViewerState};
use imgui::{Context as ImguiContext, FontConfig, FontGlyphRanges, FontSource};
use imgui_wgpu::RendererConfig;
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use wgpu::{CompositeAlphaMode, Device, Queue, Surface, SurfaceConfiguration, SurfaceError};
use winit::{
    dpi::LogicalSize,
    event::{ElementState, Event, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::WindowBuilder,
};

use crate::render::texture_manager::{TextureManager, UploadedTexture};
use crate::ui::render_ui;

#[derive(Debug, Default)]
struct AppArgs {
    reset_config: bool,
}

fn parse_args() -> anyhow::Result<AppArgs> {
    let mut args = AppArgs::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--reset-config" => args.reset_config = true,
            "-h" | "--help" => {
                println!("Usage: image-viewer [--reset-config]");
                println!("  --reset-config  overwrite saved settings with default_settings.toml");
                std::process::exit(0);
            }
            _ => {
                bail!(
                    "unknown argument: {arg}\nUsage: image-viewer [--reset-config]"
                );
            }
        }
    }
    Ok(args)
}

/// App entrypoint: setup systems, run UI loop, then save config.
fn main() -> anyhow::Result<()> {
    let args = parse_args().context("failed to parse command line arguments")?;

    infra::logging::init();

    let config_handle = infra::config::load_or_create(args.reset_config)
        .context("unable to prepare application configuration")?;

    if args.reset_config {
        log::info!("--reset-config was set; configuration reset to bundled defaults");
    }

    log::info!("Loaded configuration from {}", config_handle.path.display());

    let mut app_state = ViewerState::new(config_handle.path, config_handle.settings);
    restore_last_directory_if_needed(&mut app_state);

    let event_loop = EventLoop::new().map_err(anyhow::Error::msg)?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Vibe Image Viewer")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_resizable(true)
            .build(&event_loop)
            .map_err(anyhow::Error::msg)
            .context("failed to create window")?,
    );

    let instance = wgpu::Instance::default();
    let surface = instance
        .create_surface(window.clone())
        .context("failed to create wgpu surface")?;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .context("failed to request wgpu adapter")?;

    // 혹은 성능 위주라면 어댑터의 값을 그대로 사용합니다.
    let adapter_limits = adapter.limits();

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("image-viewer device"),
            required_features: wgpu::Features::empty(),
            // required_limits: wgpu::Limits::default(),
            required_limits: wgpu::Limits {
                max_texture_dimension_2d: adapter_limits.max_texture_dimension_2d,
                ..wgpu::Limits::downlevel_defaults()
            },
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        },
    ))
    .context("failed to request wgpu device")?;

    let mut surface_config = create_surface_config(&surface, &adapter, window.inner_size())
        .context("failed to configure surface")?;
    surface.configure(&device, &surface_config);

    let mut imgui = ImguiContext::create();
    imgui.set_ini_filename(None);
    imgui.style_mut().use_dark_colors();

    let ui_font_filename = app_state.config().ui_font_filename.clone();
    let ui_font_size_pixels = if app_state.config().ui_font_size_pixels > 0.0 {
        app_state.config().ui_font_size_pixels
    } else {
        log::warn!(
            "Invalid ui_font_size_pixels ({}). Falling back to 14.0",
            app_state.config().ui_font_size_pixels
        );
        14.0
    };

    let mut platform = WinitPlatform::init(&mut imgui);
    platform.attach_window(imgui.io_mut(), window.as_ref(), HiDpiMode::Default);

    let hidpi_factor = window.scale_factor() as f32;
    if hidpi_factor > 0.0 && (hidpi_factor - 1.0).abs() > f32::EPSILON {
        imgui.io_mut().font_global_scale = 1.0 / hidpi_factor;
        log::info!("Detected DPI scale: {:.2}", hidpi_factor);
    }

    // Load custom font (from config, under assets/fonts/)
    let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("fonts")
        .join(&ui_font_filename);

    if !ui_font_filename.is_empty() && font_path.exists() {
        let font_data = std::fs::read(&font_path)
            .expect("failed to read custom font file");
        // Leak the data so it lives for the entire program lifetime.
        // imgui requires the font data slice to live as long as the context.
        let font_data: &'static [u8] = Box::leak(font_data.into_boxed_slice());

        imgui.fonts().add_font(&[
            FontSource::TtfData {
                data: font_data,
                size_pixels: ui_font_size_pixels * hidpi_factor.max(1.0),
                config: Some(FontConfig {
                    glyph_ranges: FontGlyphRanges::from_slice(&[
                        // Basic Latin + Latin Supplement
                        0x0020, 0x00FF,
                        // Korean (Hangul Syllables)
                        0xAC00, 0xD7A3,
                        // Korean (Hangul Jamo)
                        0x1100, 0x11FF,
                        // Korean (Hangul Compatibility Jamo)
                        0x3130, 0x318F,
                        // CJK Unified Ideographs (common Hanja)
                        0x4E00, 0x9FFF,
                        // Null terminator
                        0,
                    ]),
                    ..FontConfig::default()
                }),
            },
        ]);
        log::info!(
            "Custom font loaded: {} ({} px, scale: {:.2})",
            font_path.display(),
            ui_font_size_pixels * hidpi_factor.max(1.0),
            hidpi_factor
        );
    } else {
        log::warn!(
            "Custom font not found at {}, using default imgui font",
            font_path.display()
        );
    }

    let renderer_config = RendererConfig {
        texture_format: surface_config.format,
        ..RendererConfig::default()
    };
    let mut renderer = imgui_wgpu::Renderer::new(&mut imgui, &device, &queue, renderer_config);

    let default_max_cache_size: usize = 20;
    let max_texture_size = device.limits().max_texture_dimension_2d;
    let mut texture_manager = TextureManager::new(max_texture_size, default_max_cache_size);
    log::info!(
        "TextureManager created with max_texture_size: {}, default_max_cache_size: {}",
        max_texture_size,
        default_max_cache_size
    );

    let mut current_texture: Option<UploadedTexture> = None;

    let mut last_frame = Instant::now();
    let mut modifiers = ModifiersState::default();

    let _instance = instance;
    event_loop
        .run(move |event, window_target| {
            platform.handle_event(imgui.io_mut(), window.as_ref(), &event);

            match event {
                Event::NewEvents(_) => {
                    let now = Instant::now();
                    imgui.io_mut().update_delta_time(now - last_frame);
                    last_frame = now;
                }
                Event::AboutToWait => {
                    if app_state.take_reload_request() {
                        current_texture = refresh_current_texture(
                            &mut app_state,
                            &device,
                            &queue,
                            &mut renderer,
                            &mut texture_manager,
                        );
                    }

                    if let Err(err) = platform.prepare_frame(imgui.io_mut(), window.as_ref()) {
                        log::error!("prepare_frame failed: {err}");
                        return;
                    }
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    window_id,
                } if window_id == window.id() => {
                    let frame = match surface.get_current_texture() {
                        Ok(frame) => frame,
                        Err(SurfaceError::Lost) | Err(SurfaceError::Outdated) => {
                            surface.configure(&device, &surface_config);
                            return;
                        }
                        Err(SurfaceError::OutOfMemory) => {
                            log::error!("Surface out of memory; exiting");
                            save_config_on_exit(&app_state);
                            texture_manager.clear(&mut renderer);
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

                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    let ui = imgui.frame();
                    let mut running = true;
                    render_ui(ui, &mut app_state, current_texture, &mut running);

                    if !running {
                        save_config_on_exit(&app_state);
                        texture_manager.clear(&mut renderer);
                        window_target.exit();
                        return;
                    }

                    platform.prepare_render(ui, window.as_ref());
                    let draw_data = imgui.render();

                    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("image-viewer encoder"),
                    });

                    {
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("image-viewer render pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
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
                            })],
                            depth_stencil_attachment: None,
                            occlusion_query_set: None,
                            timestamp_writes: None,
                        });

                        if let Err(err) = renderer.render(draw_data, &queue, &device, &mut rpass) {
                            log::error!("imgui render failed: {err}");
                        }
                    }

                    queue.submit(Some(encoder.finish()));
                    frame.present();
                }
                Event::WindowEvent { window_id, event } if window_id == window.id() => {
                    match event {
                        WindowEvent::CloseRequested => {
                            save_config_on_exit(&app_state);
                            texture_manager.clear(&mut renderer);
                            window_target.exit();
                        }
                        WindowEvent::DroppedFile(path) => {
                            app_state.handle_drop_path(path.as_path());
                        }
                        WindowEvent::ModifiersChanged(new_modifiers) => {
                            modifiers = new_modifiers.state();
                        }
                        WindowEvent::KeyboardInput { event, .. }
                            if event.state == ElementState::Pressed && !event.repeat =>
                        {
                            match event.physical_key {
                                PhysicalKey::Code(KeyCode::Escape) => {
                                    save_config_on_exit(&app_state);
                                    texture_manager.clear(&mut renderer);
                                    window_target.exit();
                                }
                                PhysicalKey::Code(KeyCode::KeyO)
                                    if modifiers.control_key() || modifiers.super_key() =>
                                {
                                    app_state.open_directory_dialog();
                                }
                                PhysicalKey::Code(KeyCode::ArrowRight)
                                | PhysicalKey::Code(KeyCode::ArrowDown)
                                | PhysicalKey::Code(KeyCode::PageDown) => {
                                    app_state.advance_selection(1);
                                }
                                PhysicalKey::Code(KeyCode::ArrowLeft)
                                | PhysicalKey::Code(KeyCode::ArrowUp)
                                | PhysicalKey::Code(KeyCode::PageUp) => {
                                    app_state.advance_selection(-1);
                                }
                                _ => {}
                            }
                        }
                        WindowEvent::Resized(new_size) => {
                            if new_size.width > 0 && new_size.height > 0 {
                                surface_config.width = new_size.width;
                                surface_config.height = new_size.height;
                                surface.configure(&device, &surface_config);
                            }
                        }
                        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                            if scale_factor > 0.0 {
                                imgui.io_mut().font_global_scale = 1.0 / scale_factor as f32;
                            }
                            let new_size = window.inner_size();
                            if new_size.width > 0 && new_size.height > 0 {
                                surface_config.width = new_size.width;
                                surface_config.height = new_size.height;
                                surface.configure(&device, &surface_config);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
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

/// Decode selected image and make sure we have a usable GPU texture.
fn refresh_current_texture(
    app_state: &mut ViewerState,
    device: &Device,
    queue: &Queue,
    renderer: &mut imgui_wgpu::Renderer,
    texture_manager: &mut TextureManager,
) -> Option<UploadedTexture> {
    let decoded = match app_state.load_current_image_rgba() {
        Ok(Some(decoded)) => decoded,
        Ok(None) => return None,
        Err(_) => return None,
    };

    let entry = app_state.current_entry()?;
    match texture_manager.get_or_upload(&entry.path, &decoded, device, queue, renderer) {
        Ok(uploaded) => Some(uploaded),
        Err(err) => {
            log::error!(
                "Texture upload failed for {}: {:#}",
                entry.path.display(),
                err
            );
            None
        }
    }
}
