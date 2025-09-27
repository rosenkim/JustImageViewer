use std::path::PathBuf;

use eframe::egui::{self, TextureHandle};

use crate::{
    core::{
        image_loader,
        media::{self, MediaEntry},
    },
    infra::config::{AppConfig, ConfigHandle},
};

pub struct ViewerApp {
    state: AppState,
    config: AppConfig,
}

struct AppState {
    status_message: String,
    config_path: PathBuf,
    current_folder: Option<PathBuf>,
    media_items: Vec<MediaEntry>,
    current_index: Option<usize>,
    current_texture: Option<TextureHandle>,
    current_texture_path: Option<PathBuf>,
    current_image_size: Option<(usize, usize)>,
    needs_texture_reload: bool,
}

impl AppState {
    fn new(config_path: PathBuf, status_message: String) -> Self {
        Self {
            status_message,
            config_path,
            current_folder: None,
            media_items: Vec::new(),
            current_index: None,
            current_texture: None,
            current_texture_path: None,
            current_image_size: None,
            needs_texture_reload: false,
        }
    }

    fn clear_texture(&mut self) {
        self.current_texture = None;
        self.current_texture_path = None;
        self.current_image_size = None;
    }

    fn current_entry(&self) -> Option<&MediaEntry> {
        self.current_index.and_then(|idx| self.media_items.get(idx))
    }
}

impl ViewerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, config_handle: ConfigHandle) -> Self {
        let status_message = format!("Ready — configuration at {}", config_handle.path.display());

        Self {
            state: AppState::new(config_handle.path, status_message),
            config: config_handle.settings,
        }
    }

    fn menu_bar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open Folder...").clicked() {
                    ui.close_menu();
                    self.prompt_folder_dialog();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.close_menu();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button("View", |ui| {
                ui.label("Zoom/Fit toggles coming soon");
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard Shortcuts").clicked() {
                    ui.close_menu();
                    self.state.status_message = "Shortcut overlay placeholder".to_owned();
                }
            });
        });
    }

    fn folder_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Library");
        ui.separator();
        if let Some(folder) = &self.state.current_folder {
            ui.label(format!("Folder: {}", folder.display()));
            ui.label(format!("Items: {}", self.state.media_items.len()));
        } else {
            ui.label("Drag & drop a folder or use File → Open Folder");
        }

        ui.add_space(8.0);

        if self.state.media_items.is_empty() {
            ui.weak("No supported images yet.");
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (index, entry) in self.state.media_items.iter().enumerate() {
                    let selected = self.state.current_index == Some(index);
                    let response = ui.selectable_label(selected, &entry.file_name);
                    if response.clicked() {
                        self.state.current_index = Some(index);
                        self.state.needs_texture_reload = true;
                    }
                    response.on_hover_text(entry.path.display().to_string());
                }
            });
    }

    fn viewer_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        self.handle_drop_events(ctx);
        self.handle_navigation_keys(ctx);
        self.ensure_current_texture(ctx);

        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            if let Some(texture) = self.state.current_texture.as_ref() {
                let available = ui.available_size();
                let size = texture.size_vec2();
                let mut scale = (available.x / size.x).min(available.y / size.y);
                if !scale.is_finite() || scale <= 0.0 {
                    scale = 1.0;
                }
                scale = scale.min(1.0);
                let display_size = size * scale.max(0.01);
                ui.add(egui::Image::new((texture.id(), display_size)));
            } else if self.state.current_folder.is_some() {
                ui.label("No image selected or failed to decode.");
            } else {
                ui.heading("Welcome to Vibe Image Viewer");
                ui.label("Open a folder with PNG/JPEG images to begin.");
            }

            ui.add_space(12.0);
            if let Some(entry) = self.state.current_entry() {
                ui.separator();
                ui.label(format!("File: {}", entry.file_name));
                ui.label(format!(
                    "Format: {} · Size: {}",
                    entry.format.as_str(),
                    format_file_size(entry.file_size)
                ));

                if let Some((w, h)) = self.state.current_image_size {
                    ui.label(format!("Resolution: {} × {} px", w, h));
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Status:");
            ui.label(&self.state.status_message);
            ui.separator();
            ui.label("Config");
            ui.weak(self.state.config_path.display().to_string());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "Restore last folder: {}",
                    if self.config.restore_last_folder {
                        "on"
                    } else {
                        "off"
                    }
                ));
            });
        });
    }

    fn prompt_folder_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.load_folder(path, None);
        } else {
            self.state.status_message = "Folder selection cancelled".to_owned();
        }
    }

    fn load_folder(&mut self, folder: PathBuf, focus_file: Option<PathBuf>) {
        let folder_display = folder.display().to_string();
        match media::scan_directory(&folder) {
            Ok(entries) => {
                let total = entries.len();
                if total == 0 {
                    self.state.status_message =
                        format!("No supported PNG/JPEG images in {}", folder_display);
                    self.state.current_folder = Some(folder);
                    self.state.media_items.clear();
                    self.state.current_index = None;
                    self.state.clear_texture();
                    self.state.needs_texture_reload = false;
                    return;
                }

                let focus_index = focus_file
                    .as_ref()
                    .and_then(|target| entries.iter().position(|entry| entry.path == *target))
                    .or(Some(0));

                self.state.current_folder = Some(folder);
                self.state.media_items = entries;
                self.state.current_index = focus_index;
                self.state.status_message =
                    format!("Loaded {} images from {}", total, folder_display);
                self.state.clear_texture();
                self.state.needs_texture_reload = true;
            }
            Err(err) => {
                self.state.status_message = format!("Failed to read {}: {:#}", folder_display, err);
                log::error!("Failed to load folder {}: {:#}", folder_display, err);
            }
        }
    }

    fn handle_drop_events(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }

        for file in dropped {
            if let Some(path) = file.path {
                if path.is_dir() {
                    self.load_folder(path, None);
                    break;
                } else if path.is_file() {
                    if let Some(parent) = path.parent() {
                        self.load_folder(parent.to_path_buf(), Some(path.clone()));
                    }
                    break;
                }
            }
        }
    }

    fn handle_navigation_keys(&mut self, ctx: &egui::Context) {
        let mut delta: i32 = 0;
        ctx.input(|input| {
            if input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::PageDown) {
                delta += 1;
            }
            if input.key_pressed(egui::Key::ArrowLeft) || input.key_pressed(egui::Key::PageUp) {
                delta -= 1;
            }
        });

        if delta != 0 {
            self.advance_selection(delta);
        }
    }

    fn advance_selection(&mut self, delta: i32) {
        let Some(current) = self.state.current_index else {
            return;
        };

        let total = self.state.media_items.len();
        if total == 0 {
            return;
        }

        let next = ((current as i32 + delta).rem_euclid(total as i32)) as usize;
        if next != current {
            self.state.current_index = Some(next);
            self.state.needs_texture_reload = true;
        }
    }

    fn ensure_current_texture(&mut self, ctx: &egui::Context) {
        let Some(index) = self.state.current_index else {
            self.state.clear_texture();
            return;
        };

        let Some(entry) = self.state.media_items.get(index) else {
            self.state.clear_texture();
            return;
        };

        let needs_reload = self.state.needs_texture_reload
            || self
                .state
                .current_texture_path
                .as_ref()
                .map(|path| path != &entry.path)
                .unwrap_or(true);

        if !needs_reload {
            return;
        }

        match image_loader::load_image_rgba(&entry.path) {
            Ok(decoded) => {
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [decoded.width, decoded.height],
                    &decoded.pixels,
                );
                if let Some(texture) = self.state.current_texture.as_mut() {
                    texture.set(color_image, egui::TextureOptions::default());
                } else {
                    let texture = ctx.load_texture(
                        "viewer/current_image",
                        color_image,
                        egui::TextureOptions::default(),
                    );
                    self.state.current_texture = Some(texture);
                }

                self.state.current_texture_path = Some(entry.path.clone());
                self.state.current_image_size = Some((decoded.width, decoded.height));
                self.state.status_message = format!("Viewing {}", entry.file_name);
                self.state.needs_texture_reload = false;
            }
            Err(err) => {
                self.state.status_message =
                    format!("Failed to decode {}: {:#}", entry.file_name, err);
                log::error!("Image decode error for {}: {:#}", entry.path.display(), err);
                self.state.clear_texture();
                self.state.needs_texture_reload = false;
            }
        }
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_menu").show(ctx, |ui| {
            self.menu_bar(ctx, ui);
        });

        egui::SidePanel::left("folder_panel")
            .min_width(240.0)
            .show(ctx, |ui| {
                self.folder_panel(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.viewer_panel(ctx, ui);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            self.status_bar(ui);
        });
    }
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;

    while value >= 1024.0 && unit_index + 1 < UNITS.len() {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{value:.1} {}", UNITS[unit_index])
    }
}
