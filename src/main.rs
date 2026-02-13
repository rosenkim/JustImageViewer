#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod core;
mod infra;
mod render;

use anyhow::Context;
use app::{ViewerState, format_file_size};
use imgui::{Condition, Context as ImguiContext, FontConfig, FontGlyphRanges, FontSource};
use sdl2::{
    event::Event,
    keyboard::{Keycode, Mod},
    video::GLProfile,
};
use std::path::PathBuf;
use std::time::Instant;

use crate::render::texture_manager::{TextureManager, UploadedTexture};

/// App entrypoint: setup systems, run UI loop, then save config.
fn main() -> anyhow::Result<()> {
    infra::logging::init();

    let config_handle =
        infra::config::load_or_create().context("unable to prepare application configuration")?;

    log::info!("Loaded configuration from {}", config_handle.path.display());

    let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
    let video = sdl.video().map_err(anyhow::Error::msg)?;

    let gl_attr = video.gl_attr();
    gl_attr.set_context_profile(GLProfile::Core);
    gl_attr.set_context_version(3, 3);

    let window = video
        .window("Vibe Image Viewer", 1280, 800)
        .opengl()
        .resizable()
        .allow_highdpi()
        .position_centered()
        .build()
        .map_err(anyhow::Error::msg)
        .context("failed to create SDL2 window")?;

    let gl_context = window
        .gl_create_context()
        .map_err(anyhow::Error::msg)
        .context("failed to create OpenGL context")?;
    window
        .gl_make_current(&gl_context)
        .map_err(anyhow::Error::msg)
        .context("failed to bind OpenGL context")?;

    if let Err(err) = video.gl_set_swap_interval(1) {
        log::warn!("Failed to enable vsync: {}", err);
    }

    gl::load_with(|symbol| video.gl_get_proc_address(symbol) as *const _);

    let max_texture_size = unsafe {
        let mut size: i32 = 0;
        gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut size);
        size
    };
    log::info!("OpenGL max texture size: {}x{}", max_texture_size, max_texture_size);

    let mut app_state = ViewerState::new(config_handle.path, config_handle.settings);
    let mut texture_manager = TextureManager::new(max_texture_size);
    let mut current_texture: Option<UploadedTexture> = None;

    restore_last_folder_if_needed(&mut app_state);

    let mut imgui = ImguiContext::create();
    imgui.set_ini_filename(None);
    imgui.style_mut().use_dark_colors();

    // Load custom font (place your .ttf file at assets/fonts/)
    let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("fonts")
        .join("NanumGothic.ttf");

    if font_path.exists() {
        let font_data = std::fs::read(&font_path)
            .expect("failed to read custom font file");
        // Leak the data so it lives for the entire program lifetime.
        // imgui requires the font data slice to live as long as the context.
        let font_data: &'static [u8] = Box::leak(font_data.into_boxed_slice());

        imgui.fonts().add_font(&[
            FontSource::TtfData {
                data: font_data,
                size_pixels: 14.0,
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
        log::info!("Custom font loaded: {}", font_path.display());
    } else {
        log::warn!(
            "Custom font not found at {}, using default imgui font",
            font_path.display()
        );
    }

    let mut imgui_sdl2 = imgui_sdl2::ImguiSdl2::new(&mut imgui, &window);
    let renderer =
        imgui_opengl_renderer::Renderer::new(&mut imgui, |s| video.gl_get_proc_address(s) as _);

    let mut event_pump = sdl.event_pump().map_err(anyhow::Error::msg)?;
    let mut last_frame = Instant::now();
    let mut running = true;

    while running {
        for event in event_pump.poll_iter() {
            imgui_sdl2.handle_event(&mut imgui, &event);

            match event {
                Event::Quit { .. } => running = false,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => running = false,
                Event::DropFile { filename, .. } => {
                    app_state.handle_drop_path(PathBuf::from(filename).as_path());
                }
                Event::DropText { filename, .. } => {
                    let path = PathBuf::from(filename);
                    if path.exists() {
                        app_state.handle_drop_path(path.as_path());
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::O),
                    keymod,
                    ..
                } if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD | Mod::LGUIMOD | Mod::RGUIMOD) =>
                {
                    app_state.open_folder_dialog();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::PageDown),
                    ..
                } => app_state.advance_selection(1),
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::PageUp),
                    ..
                } => app_state.advance_selection(-1),
                _ => {}
            }
        }

        let now = Instant::now();
        imgui.io_mut().update_delta_time(now - last_frame);
        last_frame = now;

        imgui_sdl2.prepare_frame(imgui.io_mut(), &window, &event_pump.mouse_state());

        if app_state.take_reload_request() {
            current_texture = refresh_current_texture(&mut app_state, &mut texture_manager);
        }

        let ui = imgui.frame();

        ui.main_menu_bar(|| {
            ui.menu("File", || {
                if ui.menu_item("Open Folder...") {
                    app_state.open_folder_dialog();
                }
                if ui.menu_item("Quit") {
                    running = false;
                }
            });
            ui.menu("View", || {
                ui.text("Zoom/Fit toggles coming soon");
            });
            ui.menu("Help", || {
                if ui.menu_item("Keyboard Shortcuts") {
                    log::info!("Keyboard shortcuts overlay not implemented yet");
                }
            });
        });

        let display = ui.io().display_size;
        let menu_height = 24.0;
        let status_height = 34.0;
        let left_width = 300.0;
        let content_height = (display[1] - menu_height - status_height).max(120.0);
        let viewer_width = (display[0] - left_width).max(220.0);
        let window_flags = imgui::WindowFlags::NO_MOVE
            | imgui::WindowFlags::NO_RESIZE
            | imgui::WindowFlags::NO_COLLAPSE;

        let mut clicked_index: Option<usize> = None;

        ui.window("Library")
            .position([0.0, menu_height], Condition::Always)
            .size([left_width, content_height], Condition::Always)
            .flags(window_flags)
            .build(|| {
                if let Some(folder) = app_state.current_folder() {
                    ui.text(format!("Folder: {}", folder.display()));
                    ui.text(format!("Items: {}", app_state.media_items().len()));
                } else {
                    ui.text("Drag a folder/file or use File > Open Folder");
                }
                ui.separator();
                ui.child_window("library_scroll")
                    .size([0.0, -36.0])
                    .build(|| {
                        for (index, entry) in app_state.media_items().iter().enumerate() {
                            if ui
                                .selectable_config(&entry.file_name)
                                .selected(app_state.current_index() == Some(index))
                                .build()
                            {
                                clicked_index = Some(index);
                            }
                        }
                    });
                if ui.button("Open Folder...") {
                    app_state.open_folder_dialog();
                }
            });

        ui.window("Viewer")
            .position([left_width, menu_height], Condition::Always)
            .size([viewer_width, content_height], Condition::Always)
            .flags(window_flags)
            .build(|| {
                let metadata_height = 86.0;
                ui.child_window("image_region")
                    .size([0.0, -metadata_height])
                    .build(|| {
                        if let Some(texture) = current_texture {
                            let avail = ui.content_region_avail();
                            let width_scale = avail[0] / texture.width as f32;
                            let height_scale = avail[1] / texture.height as f32;
                            let scale = width_scale.min(height_scale).min(1.0).max(0.01);
                            let display_size =
                                [texture.width as f32 * scale, texture.height as f32 * scale];
                            let cursor = ui.cursor_pos();
                            let centered = [
                                (avail[0] - display_size[0]).max(0.0) * 0.5,
                                (avail[1] - display_size[1]).max(0.0) * 0.5,
                            ];
                            ui.set_cursor_pos([cursor[0] + centered[0], cursor[1] + centered[1]]);
                            imgui::Image::new(texture.id, display_size).build(ui);
                        } else if app_state.current_folder().is_some() {
                            ui.text("No image selected or decode failed.");
                        } else {
                            ui.text("Welcome to Vibe Image Viewer");
                            ui.text("Open a PNG/JPEG folder to begin.");
                        }
                    });

                ui.separator();
                if let Some(entry) = app_state.current_entry() {
                    ui.text(format!("File: {}", entry.file_name));
                    ui.text(format!(
                        "Format: {}  Size: {}",
                        entry.format.as_str(),
                        format_file_size(entry.file_size)
                    ));
                    if let Some((w, h)) = app_state.current_image_size() {
                        ui.text(format!("Resolution: {} x {}", w, h));
                    }
                } else {
                    ui.text("No file selected");
                }
            });

        ui.window("Status")
            .position([0.0, menu_height + content_height], Condition::Always)
            .size([display[0], status_height], Condition::Always)
            .flags(window_flags | imgui::WindowFlags::NO_TITLE_BAR)
            .build(|| {
                ui.text(format!("Status: {}", app_state.status_message()));
                ui.same_line();
                ui.text("|");
                ui.same_line();
                ui.text(format!("Config: {}", app_state.config_path().display()));
                ui.same_line();
                ui.text("|");
                ui.same_line();
                ui.text(format!(
                    "Restore last folder: {}",
                    if app_state.restore_last_folder() {
                        "on"
                    } else {
                        "off"
                    }
                ));
            });

        if let Some(index) = clicked_index {
            app_state.select_index(index);
        }

        imgui_sdl2.prepare_render(ui, &window);
        unsafe {
            gl::Viewport(0, 0, window.drawable_size().0 as i32, window.drawable_size().1 as i32);
            gl::ClearColor(0.08, 0.09, 0.11, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        renderer.render(&mut imgui);
        window.gl_swap_window();
    }

    infra::config::save(app_state.config_path(), app_state.config())
        .context("failed to persist application configuration")?;

    Ok(())
}

/// Try to restore the last folder from config.
fn restore_last_folder_if_needed(app_state: &mut ViewerState) {
    if let Some(folder) = app_state.restore_candidate().map(PathBuf::from) {
        if folder.is_dir() {
            app_state.load_folder(folder, None);
        } else {
            log::warn!(
                "Configured last_open_folder is not a directory: {}",
                folder.display()
            );
        }
    }
}

/// Decode selected image and make sure we have a usable OpenGL texture.
fn refresh_current_texture(
    app_state: &mut ViewerState,
    texture_manager: &mut TextureManager,
) -> Option<UploadedTexture> {
    let decoded = match app_state.load_current_image_rgba() {
        Ok(Some(decoded)) => decoded,
        Ok(None) => return None,
        Err(_) => return None,
    };

    let entry = app_state.current_entry()?;
    match texture_manager.get_or_upload(&entry.path, &decoded) {
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
