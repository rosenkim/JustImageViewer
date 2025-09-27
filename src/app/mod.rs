use std::path::PathBuf;

use eframe::egui;

use crate::infra::config::{AppConfig, ConfigHandle};

pub struct ViewerApp {
    state: AppState,
    config: AppConfig,
}

#[derive(Default)]
struct AppState {
    current_folder: Option<PathBuf>,
    status_message: String,
    config_path: PathBuf,
}

impl ViewerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, config_handle: ConfigHandle) -> Self {
        let status_message = format!("Ready — configuration at {}", config_handle.path.display());

        Self {
            state: AppState {
                current_folder: None,
                status_message,
                config_path: config_handle.path,
            },
            config: config_handle.settings,
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open Folder...").clicked() {
                    ui.close_menu();
                    self.state.status_message = "Open folder dialog not implemented yet".to_owned();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.close_menu();
                    self.state.status_message = "Use Cmd/Ctrl+Q to exit".to_owned();
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
        ui.heading("Folders");
        ui.horizontal(|ui| {
            ui.label("Recent:");
            ui.weak("—");
        });
        ui.separator();
        ui.label("Drag & drop a folder to begin.");

        if let Some(folder) = &self.state.current_folder {
            ui.separator();
            ui.label(format!("Active: {}", folder.display()));
        }
    }

    fn viewer_panel(&mut self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Image viewport placeholder");
                ui.label("Configure defaults in settings.toml");
            });
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
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_menu").show(ctx, |ui| {
            self.menu_bar(ui);
        });

        egui::SidePanel::left("folder_panel")
            .min_width(220.0)
            .show(ctx, |ui| {
                self.folder_panel(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.viewer_panel(ui);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            self.status_bar(ui);
        });
    }
}
