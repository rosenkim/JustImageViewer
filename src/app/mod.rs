use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::{
    core::media::{self, MediaEntry, ThumbnailInfo},
    infra::config::AppConfig,
    render::{
        image_uploader::UploadedTexture,
        imgui_textures::ImguiTextures,
        texture_atlas_manager::TextureAtlasManager,
    },
};

use tokio::sync::mpsc;
use crate::core::image_loader;

pub use crate::math::Rect2D;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ImageViewMode {
    Original,
    #[default]
    FitToWindow,
    FitToWidth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySortField {
    #[default]
    Name,
    Date,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

fn normalize_library_width(width: f32) -> f32 {
    if width.is_finite() && width > 0.0 {
        width
    } else {
        300.0
    }
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
    show_thumbnail: bool,
    show_grid_view: bool,
    pending_library_scroll_to_selection: bool,
    pending_library_scroll_direction: i32,
    library_items_per_row: usize,
    image_selection: Option<Rect2D>,
    image_selection_drag_start: Option<[f32; 2]>,
    image_selection_drag_mode: Option<ImageSelectionDragMode>,

    current_texture: Option<UploadedTexture>,

    worker_handles: Vec<tokio::task::JoinHandle<()>>,
    thumbnail_tx: Option<mpsc::Sender<ThumbnailResult>>,
    thumbnail_rx: Option<mpsc::Receiver<ThumbnailResult>>,
}

/// Decoded thumbnail pixels sent from the worker task back to the main thread.
pub struct ThumbnailResult {
    pub index: usize,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}

impl ViewerState {
    /// Create app state with config and default UI state.
    pub fn new(config_path: PathBuf, mut config: AppConfig) -> Self {
        let status_message = format!("Ready - configuration at {}", config_path.display());
        let show_library = config.show_library;
        let show_info = config.show_info;
        let show_selection_window = config.show_selection_window;
        let library_width = normalize_library_width(config.library_width);
        let image_view_mode = config.image_view_mode;
        let library_sort_field = config.library_sort_field;
        let sort_direction = config.sort_direction;
        let show_thumbnail = config.show_thumbnail;
        let show_grid_view = config.show_grid_view;

        config.library_width = library_width;
        config.image_view_mode = image_view_mode;
        config.library_sort_field = library_sort_field;
        config.sort_direction = sort_direction;
        config.show_thumbnail = show_thumbnail;
        config.show_grid_view = show_grid_view;

        Self {
            config,
            config_path,
            status_message,
            show_library,
            show_info,
            show_keyboard_shortcuts: false,
            show_selection_window,
            current_directory: None,
            media_items: Vec::new(),
            current_index: None,
            current_image_size: None,
            needs_image_reload: false,
            library_width,
            image_view_mode,
            library_sort_field,
            sort_direction,
            show_thumbnail,
            show_grid_view,
            pending_library_scroll_to_selection: false,
            pending_library_scroll_direction: 0,
            library_items_per_row: 1,
            image_selection: None,
            image_selection_drag_start: None,
            image_selection_drag_mode: None,

            current_texture: None,

            worker_handles: Vec::new(),
            thumbnail_tx: None,
            thumbnail_rx: None,
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
        self.config.show_library = show;
    }

    pub fn show_info(&self) -> bool {
        self.show_info
    }

    pub fn set_show_info(&mut self, show: bool) {
        self.show_info = show;
        self.config.show_info = show;
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
        self.config.show_selection_window = show;
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
        self.config.last_open_file.as_deref()
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
        let normalized = normalize_library_width(width);
        self.library_width = normalized;
        self.config.library_width = normalized;
    }

    pub fn image_view_mode(&self) -> ImageViewMode {
        self.image_view_mode
    }

    pub fn set_image_view_mode(&mut self, mode: ImageViewMode) {
        self.image_view_mode = mode;
        self.config.image_view_mode = mode;
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
        self.config.library_sort_field = field;
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
        self.config.sort_direction = direction;
        if self.sort_direction == direction {
            return;
        }
        self.sort_direction = direction;
        self.sort_media_items();
    }

    pub fn show_thumbnail(&self) -> bool {
        self.show_thumbnail
    }

    pub fn set_show_thumbnail(&mut self, show: bool) {
        self.show_thumbnail = show;
        self.config.show_thumbnail = show;
    }

    pub fn show_grid_view(&self) -> bool {
        self.show_grid_view
    }

    pub fn set_show_grid_view(&mut self, show: bool) {
        self.show_grid_view = show;
        self.config.show_grid_view = show;
    }

    pub fn library_items_per_row(&self) -> usize {
        self.library_items_per_row.max(1)
    }

    pub fn set_library_items_per_row(&mut self, items_per_row: usize) {
        // Keep this value always valid so keyboard move is safe.
        self.library_items_per_row = items_per_row.max(1);
    }

    pub fn select_index(&mut self, index: usize) {
        if index < self.media_items.len() {
            let delta = index as i32 - self.current_index.unwrap_or(0) as i32;
            self.current_index = Some(index);
            self.needs_image_reload = true;
            self.pending_library_scroll_to_selection = true;
            self.pending_library_scroll_direction = delta.signum();
            self.clear_image_selection_state();
            self.sync_last_open_file();
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
            self.select_index(next);
        }
    }

    pub fn take_pending_library_scroll_to_selection(&mut self) -> Option<i32> {
        if !self.pending_library_scroll_to_selection {
            return None;
        }

        let pending = self.pending_library_scroll_to_selection;
        let direction = self.pending_library_scroll_direction;
        self.pending_library_scroll_to_selection = false;
        self.pending_library_scroll_direction = 0;

        if pending {
            Some(direction)
        } else {
            None
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

    fn drop_handles(&mut self) {
        for handle in self.worker_handles.iter() {
            if !handle.is_finished() {
                handle.abort();
            }
        }

        self.thumbnail_rx = None;
        self.thumbnail_tx = None;
        self.worker_handles.clear();
    }

    fn spawn_thumbnail_work(&mut self) {
        // Collect (index, path) pairs for entries that don't have a thumbnail yet.
        let paths: Vec<(usize, PathBuf)> = self
            .media_items
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.thumbnail.is_none())
            .map(|(i, entry)| (i, entry.path.clone()))
            .collect();

        if paths.is_empty() {
            return;
        }

        let (tx, rx) = mpsc::channel::<ThumbnailResult>(64);
        self.thumbnail_tx = Some(tx.clone());
        self.thumbnail_rx = Some(rx);

        let handle = tokio::task::spawn(async move {
            for (index, path) in paths {
                // Use spawn_blocking so heavy image decoding doesn't block the async runtime.
                let result = tokio::task::spawn_blocking({
                    let path = path.clone();
                    move || -> anyhow::Result<crate::core::image_loader::DecodedImage> {
                        image_loader::load_thumbnail_rgba(&path, 128)
                    }
                })
                .await;

                match result {
                    Ok(Ok(decoded)) => {
                        let msg = ThumbnailResult {
                            index,
                            path,
                            width: decoded.width as u32,
                            height: decoded.height as u32,
                            pixels: decoded.pixels,
                        };
                        // If the receiver has been dropped (new directory loaded), stop.
                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Ok(Err(err)) => {
                        log::warn!("Failed to load thumbnail for {}: {:#}", path.display(), err);
                    }
                    Err(err) => {
                        log::warn!("Thumbnail task panicked for {}: {:#}", path.display(), err);
                    }
                }

                tokio::task::yield_now().await;
            }
        });

        self.worker_handles.push(handle);
    }

    /// Drain pending thumbnail results from the worker channel.
    /// Returns raw decoded pixel data; the caller is responsible for uploading
    /// to the GPU atlas and then calling `apply_thumbnail_info`.
    pub fn poll_thumbnail_results(&mut self) -> Vec<ThumbnailResult> {
        let Some(rx) = self.thumbnail_rx.as_mut() else {
            return Vec::new();
        };

        let mut results = Vec::new();
        while let Ok(result) = rx.try_recv() {
            // Only keep results that still correspond to the current media list.
            if self
                .media_items
                .get(result.index)
                .is_some_and(|entry| entry.path == result.path)
            {
                results.push(result);
            }
        }
        results
    }

    /// Upload thumbnail pixels to the GPU atlas and write the resulting
    /// `ThumbnailInfo` back into the corresponding `MediaEntry`.
    pub fn apply_thumbnail_info(
        &mut self,
        result: ThumbnailResult,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut imgui_wgpu::Renderer,
        imgui_textures: &mut ImguiTextures,
        texture_atlas: &mut TextureAtlasManager,
    ) {
        // Check index/path are still valid before doing expensive GPU work.
        let entry = match self.media_items.get_mut(result.index) {
            Some(e) if e.path == result.path => e,
            _ => return,
        };

        match texture_atlas.load_image(
            device,
            queue,
            renderer,
            imgui_textures,
            result.width,
            result.height,
            &result.pixels,
        ) {
            Ok(region) => {
                entry.thumbnail = Some(ThumbnailInfo {
                    atlas_image_id: region.id,
                    texture_index: region.texture_id,
                    uvs: region.uvs,
                    image_size: region.image_size,
                });
            }
            Err(err) => {
                log::warn!(
                    "Failed to upload thumbnail for {}: {:#}",
                    result.path.display(),
                    err
                );
            }
        }
    }

    /// Load images from a directory and choose which image to focus first.
    pub fn load_directory(&mut self, directory: PathBuf, focus_file: Option<PathBuf>) {
        self.drop_handles();

        let directory_display = directory.display().to_string();
        match media::scan_directory(&directory) {
            Ok(entries) => {
                let total = entries.len();
                if total == 0 {
                    self.status_message = format!(
                        "No supported images in {} (PNG, JPEG, BMP, GIF, WebP, TIFF, TGA, ICO, PNM, DDS, Farbfeld)",
                        directory_display
                    );
                    self.config.last_open_file = None;
                    self.current_directory = Some(directory);
                    self.media_items.clear();
                    self.current_index = None;
                    self.current_image_size = None;
                    self.needs_image_reload = false;
                    self.clear_image_selection_state();
                    return;
                }

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
                self.config.last_open_file = self
                    .current_index
                    .and_then(|i| self.media_items.get(i))
                    .map(|e| e.path.clone());
                self.current_image_size = None;
                self.status_message = format!("Loaded {} images from {}", total, directory_display);
                self.needs_image_reload = true;
                self.pending_library_scroll_to_selection = true;
                self.pending_library_scroll_direction = 0;
                self.clear_image_selection_state();

                // spawn thumbnails work
                self.spawn_thumbnail_work();
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
        // Read only image header so library list can show resolution quickly.
        let dimensions = image::image_dimensions(&file_path)
            .ok()
            .map(|(width, height)| (width as usize, height as usize));

        self.config.last_open_file = Some(file_path.clone());
        self.current_directory = Some(directory);
        self.media_items = vec![MediaEntry {
            path: file_path.clone(),
            file_name,
            format,
            file_size,
            modified_time,
            dimensions,
            thumbnail: None,
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

    pub fn current_texture(&self) -> Option<&UploadedTexture> {
        self.current_texture.as_ref()
    }

    pub fn set_current_texture(&mut self, texture: Option<UploadedTexture>) {
        self.current_image_size = texture.as_ref().map(|t| (t.width, t.height));
        self.current_texture = texture;
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
        self.sync_last_open_file();
    }

    fn sync_last_open_file(&mut self) {
        self.config.last_open_file = self
            .current_index
            .and_then(|i| self.media_items.get(i))
            .map(|entry| entry.path.clone());
    }

    pub fn copy_region_to_clipboard(&self, selection: Option<Rect2D>) {
        let selection = selection.or_else(|| self.image_selection());
        crate::core::helper::copy_region_to_clipboard(selection, self.current_texture.as_ref());
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
