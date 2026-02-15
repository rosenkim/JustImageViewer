use std::path::{Path, PathBuf};

use crate::{
    core::{
        image_loader::{self, DecodedImage},
        media::{self, MediaEntry},
    },
    infra::config::AppConfig,
};

pub struct ViewerState {
    config: AppConfig,
    config_path: PathBuf,
    status_message: String,
    show_library: bool,
    show_info: bool,
    current_directory: Option<PathBuf>,
    media_items: Vec<MediaEntry>,
    current_index: Option<usize>,
    current_image_size: Option<(usize, usize)>,
    needs_image_reload: bool,
}

impl ViewerState {
    /// Create app state with config and default UI state.
    pub fn new(config_path: PathBuf, config: AppConfig) -> Self {
        let status_message = format!("Ready - configuration at {}", config_path.display());
        Self {
            config,
            config_path,
            status_message,
            show_library: true,
            show_info: true,
            current_directory: None,
            media_items: Vec::new(),
            current_index: None,
            current_image_size: None,
            needs_image_reload: false,
        }
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn show_library(&self) -> bool {
        self.show_library
    }

    pub fn set_show_library(&mut self, show: bool) {
        self.show_library = show;
    }

    pub fn show_info(&self) -> bool {
        self.show_info
    }

    pub fn set_show_info(&mut self, show: bool) {
        self.show_info = show;
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn restore_last_directory(&self) -> bool {
        self.config.restore_last_directory
    }

    pub fn restore_candidate(&self) -> Option<&Path> {
        if !self.config.restore_last_directory {
            return None;
        }
        self.config.last_open_directory.as_deref()
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn current_directory(&self) -> Option<&Path> {
        self.current_directory.as_deref()
    }

    pub fn media_items(&self) -> &[MediaEntry] {
        &self.media_items
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current_entry(&self) -> Option<&MediaEntry> {
        self.current_index.and_then(|idx| self.media_items.get(idx))
    }

    pub fn current_image_size(&self) -> Option<(usize, usize)> {
        self.current_image_size
    }

    pub fn select_index(&mut self, index: usize) {
        if index < self.media_items.len() {
            self.current_index = Some(index);
            self.needs_image_reload = true;
        }
    }

    pub fn advance_selection(&mut self, delta: i32) {
        let Some(current) = self.current_index else {
            return;
        };
        let total = self.media_items.len();
        if total == 0 {
            return;
        }

        let next = ((current as i32 + delta).rem_euclid(total as i32)) as usize;
        if next != current {
            self.current_index = Some(next);
            self.needs_image_reload = true;
        }
    }

    pub fn open_directory_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.load_directory(path, None);
        } else {
            self.status_message = "Directory selection cancelled".to_owned();
        }
    }

    pub fn handle_drop_path(&mut self, path: &Path) {
        if path.is_dir() {
            self.load_directory(path.to_path_buf(), None);
        } else if path.is_file() && let Some(parent) = path.parent() {
            self.load_directory(parent.to_path_buf(), Some(path.to_path_buf()));
        }
    }

    /// Load images from a directory and choose which image to focus first.
    pub fn load_directory(&mut self, directory: PathBuf, focus_file: Option<PathBuf>) {
        let directory_display = directory.display().to_string();
        match media::scan_directory(&directory) {
            Ok(entries) => {
                let total = entries.len();
                if total == 0 {
                    self.status_message = format!(
                        "No supported images in {} (PNG, JPEG, BMP, GIF, WebP, TIFF, TGA, ICO, PNM, HDR, DDS, Farbfeld)",
                        directory_display
                    );
                    self.config.last_open_directory = Some(directory.clone());
                    self.current_directory = Some(directory);
                    self.media_items.clear();
                    self.current_index = None;
                    self.current_image_size = None;
                    self.needs_image_reload = false;
                    return;
                }

                let focus_index = focus_file
                    .as_ref()
                    .and_then(|target| entries.iter().position(|entry| entry.path == *target))
                    .or(Some(0));

                self.config.last_open_directory = Some(directory.clone());
                self.current_directory = Some(directory);
                self.media_items = entries;
                self.current_index = focus_index;
                self.current_image_size = None;
                self.status_message = format!("Loaded {} images from {}", total, directory_display);
                self.needs_image_reload = true;
            }
            Err(err) => {
                self.status_message = format!("Failed to read {}: {:#}", directory_display, err);
                log::error!("Failed to load directory {}: {:#}", directory_display, err);
            }
        }
    }

    pub fn take_reload_request(&mut self) -> bool {
        let requested = self.needs_image_reload;
        self.needs_image_reload = false;
        requested
    }

    /// Decode the selected image into RGBA bytes for texture upload.
    pub fn load_current_image_rgba(&mut self) -> anyhow::Result<Option<DecodedImage>> {
        let Some(entry) = self.current_entry() else {
            self.current_image_size = None;
            return Ok(None);
        };
        let path = entry.path.clone();
        let file_name = entry.file_name.clone();
        let file_size = entry.file_size;

        match image_loader::load_image_rgba(&path) {
            Ok(decoded) => {
                self.current_image_size = Some((decoded.width, decoded.height));
                self.status_message = format!(
                    "Viewing {} ({})",
                    file_name,
                    format_file_size(file_size)
                );
                Ok(Some(decoded))
            }
            Err(err) => {
                self.current_image_size = None;
                self.status_message = format!("Failed to decode {}: {:#}", file_name, err);
                log::error!("Image decode error for {}: {:#}", path.display(), err);
                Err(err)
            }
        }
    }
}

pub fn format_file_size(bytes: u64) -> String {
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
