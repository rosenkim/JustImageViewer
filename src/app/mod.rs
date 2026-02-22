use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, bail};

use crate::{
    core::{
        image_loader::{self, DecodedImage},
        media::{self, MediaEntry},
    },
    infra::config::AppConfig,
};

pub use crate::math::Rect2D;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageViewMode {
    Original,
    FitToWindow,
    FitToWidth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySortField {
    Name,
    Date,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSelectionResizeHandle {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageSelectionDragMode {
    Create,
    Move {
        original: Rect2D,
    },
    Resize {
        handle: ImageSelectionResizeHandle,
        original: Rect2D,
    },
}

pub struct ViewerState {
    config: AppConfig,
    config_path: PathBuf,
    status_message: String,
    show_library: bool,
    show_info: bool,
    show_keyboard_shortcuts: bool,
    show_selection_window: bool,
    current_directory: Option<PathBuf>,
    media_items: Vec<MediaEntry>,
    current_index: Option<usize>,
    current_image_size: Option<(usize, usize)>,
    needs_image_reload: bool,
    library_width: f32,
    image_view_mode: ImageViewMode,
    library_sort_field: LibrarySortField,
    sort_direction: SortDirection,
    image_selection: Option<Rect2D>,
    image_selection_drag_start: Option<[f32; 2]>,
    image_selection_drag_mode: Option<ImageSelectionDragMode>,
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
            show_keyboard_shortcuts: false,
            show_selection_window: false,
            current_directory: None,
            media_items: Vec::new(),
            current_index: None,
            current_image_size: None,
            needs_image_reload: false,
            library_width: 300.0,
            image_view_mode: ImageViewMode::FitToWindow,
            library_sort_field: LibrarySortField::Name,
            sort_direction: SortDirection::Ascending,
            image_selection: None,
            image_selection_drag_start: None,
            image_selection_drag_mode: None,
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

    pub fn show_keyboard_shortcuts(&self) -> bool {
        self.show_keyboard_shortcuts
    }

    pub fn set_show_keyboard_shortcuts(&mut self, show: bool) {
        self.show_keyboard_shortcuts = show;
    }

    pub fn show_selection_window(&self) -> bool {
        self.show_selection_window
    }

    pub fn set_show_selection_window(&mut self, show: bool) {
        self.show_selection_window = show;
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

    pub fn library_width(&self) -> f32 {
        self.library_width
    }

    pub fn set_library_width(&mut self, width: f32) {
        self.library_width = width;
    }

    pub fn image_view_mode(&self) -> ImageViewMode {
        self.image_view_mode
    }

    pub fn set_image_view_mode(&mut self, mode: ImageViewMode) {
        self.image_view_mode = mode;
    }

    pub fn image_selection(&self) -> Option<Rect2D> {
        self.image_selection
    }

    pub fn set_image_selection(&mut self, selection: Option<Rect2D>) {
        self.image_selection = selection;
    }

    pub fn image_selection_drag_start(&self) -> Option<[f32; 2]> {
        self.image_selection_drag_start
    }

    pub fn image_selection_drag_mode(&self) -> Option<ImageSelectionDragMode> {
        self.image_selection_drag_mode
    }

    pub fn begin_image_selection_drag(&mut self, start: [f32; 2], mode: ImageSelectionDragMode) {
        self.image_selection_drag_start = Some(start);
        self.image_selection_drag_mode = Some(mode);
    }

    pub fn clear_image_selection_drag(&mut self) {
        self.image_selection_drag_start = None;
        self.image_selection_drag_mode = None;
    }

    pub fn clear_image_selection_state(&mut self) {
        self.image_selection = None;
        self.image_selection_drag_start = None;
        self.image_selection_drag_mode = None;
    }

    pub fn library_sort_field(&self) -> LibrarySortField {
        self.library_sort_field
    }

    pub fn set_library_sort_field(&mut self, field: LibrarySortField) {
        if self.library_sort_field == field {
            return;
        }
        self.library_sort_field = field;
        self.sort_media_items();
    }

    pub fn sort_direction(&self) -> SortDirection {
        self.sort_direction
    }

    pub fn set_sort_direction(&mut self, direction: SortDirection) {
        if self.sort_direction == direction {
            return;
        }
        self.sort_direction = direction;
        self.sort_media_items();
    }

    pub fn select_index(&mut self, index: usize) {
        if index < self.media_items.len() {
            self.current_index = Some(index);
            self.needs_image_reload = true;
            self.clear_image_selection_state();
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
            self.clear_image_selection_state();
        }
    }

    pub fn open_directory_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.load_directory(path, None);
        } else {
            self.status_message = "Directory selection cancelled".to_owned();
        }
    }

    pub fn open_path_argument(&mut self, path: PathBuf) -> anyhow::Result<()> {
        if path.is_dir() {
            self.load_directory(path, None);
            return Ok(());
        }

        if path.is_file() {
            self.load_single_file(path)?;
            self.show_library = false;
            return Ok(());
        }

        bail!(
            "path does not exist or is not accessible: {}",
            path.display()
        );
    }

    pub fn refresh_current_directory(&mut self) {
        let Some(directory) = self.current_directory.clone() else {
            self.status_message = "No directory is currently open".to_owned();
            return;
        };

        let focus_file = self.current_entry().map(|entry| entry.path.clone());
        self.load_directory(directory, focus_file);
    }

    pub fn handle_drop_path(&mut self, path: &Path) {
        if path.is_dir() {
            self.load_directory(path.to_path_buf(), None);
        } else if path.is_file()
            && let Some(parent) = path.parent()
        {
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
                        "No supported images in {} (PNG, JPEG, BMP, GIF, WebP, TIFF, TGA, ICO, PNM, DDS, Farbfeld)",
                        directory_display
                    );
                    self.config.last_open_directory = Some(directory.clone());
                    self.current_directory = Some(directory);
                    self.media_items.clear();
                    self.current_index = None;
                    self.current_image_size = None;
                    self.needs_image_reload = false;
                    self.clear_image_selection_state();
                    return;
                }

                self.config.last_open_directory = Some(directory.clone());
                self.current_directory = Some(directory);
                self.media_items = entries;
                self.sort_media_items();
                let focus_index = focus_file
                    .as_ref()
                    .and_then(|target| {
                        self.media_items
                            .iter()
                            .position(|entry| entry.path == *target)
                    })
                    .or(Some(0));
                self.current_index = focus_index;
                self.current_image_size = None;
                self.status_message = format!("Loaded {} images from {}", total, directory_display);
                self.needs_image_reload = true;
                self.clear_image_selection_state();
            }
            Err(err) => {
                self.status_message = format!("Failed to read {}: {:#}", directory_display, err);
                log::error!("Failed to load directory {}: {:#}", directory_display, err);
            }
        }
    }

    fn load_single_file(&mut self, file_path: PathBuf) -> anyhow::Result<()> {
        let directory = file_path
            .parent()
            .map(Path::to_path_buf)
            .context("file path has no parent directory")?;

        let extension = file_path
            .extension()
            .and_then(OsStr::to_str)
            .context("file has no extension")?;
        let format = media::MediaFormat::from_extension(extension)
            .with_context(|| format!("unsupported image file extension: {}", extension))?;

        let metadata = std::fs::metadata(&file_path)
            .with_context(|| format!("failed to read metadata for {}", file_path.display()))?;
        let file_size = metadata.len();
        let modified_time = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();
        let file_name = file_path
            .file_name()
            .and_then(OsStr::to_str)
            .map(str::to_owned)
            .unwrap_or_else(|| file_path.display().to_string());

        self.config.last_open_directory = Some(directory.clone());
        self.current_directory = Some(directory);
        self.media_items = vec![MediaEntry {
            path: file_path.clone(),
            file_name,
            format,
            file_size,
            modified_time,
        }];
        self.current_index = Some(0);
        self.current_image_size = None;
        self.needs_image_reload = true;
        self.status_message = format!("Loaded 1 image: {}", file_path.display());
        self.clear_image_selection_state();

        Ok(())
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
            self.clear_image_selection_state();
            return Ok(None);
        };
        let path = entry.path.clone();
        let file_name = entry.file_name.clone();
        let file_size = entry.file_size;

        match image_loader::load_image_rgba(&path) {
            Ok(decoded) => {
                self.current_image_size = Some((decoded.width, decoded.height));
                self.status_message =
                    format!("Viewing {} ({})", file_name, format_file_size(file_size));
                Ok(Some(decoded))
            }
            Err(err) => {
                self.current_image_size = None;
                self.clear_image_selection_state();
                self.status_message = format!("Failed to decode {}: {:#}", file_name, err);
                log::error!("Image decode error for {}: {:#}", path.display(), err);
                Err(err)
            }
        }
    }

    fn sort_media_items(&mut self) {
        let selected_path = self.current_entry().map(|entry| entry.path.clone());
        let sort_direction = self.sort_direction;
        let sort_field = self.library_sort_field;

        self.media_items.sort_by(|a, b| {
            let primary = match sort_field {
                LibrarySortField::Name => {
                    a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase())
                }
                LibrarySortField::Date => a.modified_time.cmp(&b.modified_time),
                LibrarySortField::Size => a.file_size.cmp(&b.file_size),
            };
            let name_tiebreaker = a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase());
            let path_tiebreaker = a.path.cmp(&b.path);

            if sort_direction == SortDirection::Ascending {
                primary.then(name_tiebreaker).then(path_tiebreaker)
            } else {
                primary
                    .reverse()
                    .then(name_tiebreaker.reverse())
                    .then(path_tiebreaker.reverse())
            }
        });

        self.current_index = selected_path.as_ref().and_then(|target| {
            self.media_items
                .iter()
                .position(|entry| &entry.path == target)
        });
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
