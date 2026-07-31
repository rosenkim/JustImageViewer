use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

use anyhow::{Context, bail};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    core::media::{self, MediaEntry, ThumbnailInfo},
    infra::{
        bookmark::{BookmarkEntry, BookmarkStore, sort_bookmarks},
        config::{AppConfig, OpenDirectoryConfig},
    },
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

pub const MAX_OPEN_DIRECTORIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DirectoryId(u64);

/// State owned by one row in the Library panel.
pub struct DirectorySession {
    id: DirectoryId,
    directory: PathBuf,
    media_items: Vec<MediaEntry>,
    current_index: Option<usize>,
    selected_paths: HashSet<PathBuf>,
    selection_anchor: Option<usize>,
    pending_scroll_to_selection: bool,
    pending_scroll_direction: i32,
    items_per_row: usize,
}

impl DirectorySession {
    fn new(id: DirectoryId, directory: PathBuf, media_items: Vec<MediaEntry>) -> Self {
        Self {
            id,
            directory,
            media_items,
            current_index: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            pending_scroll_to_selection: false,
            pending_scroll_direction: 0,
            items_per_row: 1,
        }
    }

    pub fn id(&self) -> DirectoryId {
        self.id
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn media_items(&self) -> &[MediaEntry] {
        &self.media_items
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current_entry(&self) -> Option<&MediaEntry> {
        self.current_index
            .and_then(|index| self.media_items.get(index))
    }

    pub fn is_multi_select(&self) -> bool {
        self.selected_paths.len() > 1
    }

    pub fn selected_count(&self) -> usize {
        self.selected_paths.len()
    }

    pub fn is_path_selected(&self, path: &Path) -> bool {
        self.selected_paths.contains(path)
    }
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
    bookmark_store: BookmarkStore,
    status_message: String,
    show_library: bool,
    show_info: bool,
    show_keyboard_shortcuts: bool,
    show_bookmark_window: bool,
    show_selection_window: bool,
    directories: Vec<DirectorySession>,
    active_directory_id: Option<DirectoryId>,
    next_directory_id: u64,
    // Atlas image ids whose files were deleted or modified. The render loop
    // drains this list and frees the atlas slots (needs GPU objects).
    pending_atlas_removals: Vec<u64>,
    current_image_size: Option<(usize, usize)>,
    needs_image_reload: bool,
    library_width: f32,
    image_view_mode: ImageViewMode,
    library_sort_field: LibrarySortField,
    sort_direction: SortDirection,
    show_thumbnail: bool,
    show_grid_view: bool,
    image_selection: Option<Rect2D>,
    image_selection_drag_start: Option<[f32; 2]>,
    image_selection_drag_mode: Option<ImageSelectionDragMode>,
    bookmarks: Vec<BookmarkEntry>,
    bookmarks_dirty: bool,
    pending_delete_bookmark_path: Option<PathBuf>,

    current_texture: Option<UploadedTexture>,

    worker_handles: HashMap<DirectoryId, tokio::task::JoinHandle<()>>,
    thumbnail_tx: mpsc::Sender<ThumbnailResult>,
    thumbnail_rx: mpsc::Receiver<ThumbnailResult>,
}

/// Decoded thumbnail pixels sent from the worker task back to the main thread.
pub struct ThumbnailResult {
    pub directory_id: DirectoryId,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}

impl ViewerState {
    /// Create app state with config and default UI state.
    pub fn new(config_path: PathBuf, mut config: AppConfig) -> Self {
        let bookmark_store = BookmarkStore::new(&config_path);
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

        let (bookmarks, status_message) = match bookmark_store.load_all() {
            Ok(bookmarks) => (bookmarks, status_message),
            Err(err) => {
                log::error!("Failed to load bookmarks: {err:#}");
                (
                    Vec::new(),
                    format!("Ready - failed to load bookmarks: {err:#}"),
                )
            }
        };

        let (thumbnail_tx, thumbnail_rx) = mpsc::channel::<ThumbnailResult>(64);

        Self {
            config,
            config_path,
            bookmark_store,
            status_message,
            show_library,
            show_info,
            show_keyboard_shortcuts: false,
            show_bookmark_window: false,
            show_selection_window,
            directories: Vec::new(),
            active_directory_id: None,
            next_directory_id: 1,
            pending_atlas_removals: Vec::new(),
            current_image_size: None,
            needs_image_reload: false,
            library_width,
            image_view_mode,
            library_sort_field,
            sort_direction,
            show_thumbnail,
            show_grid_view,
            image_selection: None,
            image_selection_drag_start: None,
            image_selection_drag_mode: None,
            bookmarks,
            bookmarks_dirty: false,
            pending_delete_bookmark_path: None,

            current_texture: None,

            worker_handles: HashMap::new(),
            thumbnail_tx,
            thumbnail_rx,
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

    pub fn show_bookmark_window(&self) -> bool {
        self.show_bookmark_window
    }

    pub fn set_show_bookmark_window(&mut self, show: bool) {
        if self.show_bookmark_window && !show {
            self.flush_bookmarks_if_dirty();
            self.pending_delete_bookmark_path = None;
        }
        self.show_bookmark_window = show;
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn directory_sessions(&self) -> &[DirectorySession] {
        &self.directories
    }

    pub fn active_directory_id(&self) -> Option<DirectoryId> {
        self.active_directory_id
    }

    pub fn directory_session(&self, id: DirectoryId) -> Option<&DirectorySession> {
        self.directories.iter().find(|session| session.id == id)
    }

    fn directory_session_mut(&mut self, id: DirectoryId) -> Option<&mut DirectorySession> {
        self.directories.iter_mut().find(|session| session.id == id)
    }

    fn active_session(&self) -> Option<&DirectorySession> {
        self.active_directory_id.and_then(|id| self.directory_session(id))
    }

    fn active_session_mut(&mut self) -> Option<&mut DirectorySession> {
        let id = self.active_directory_id?;
        self.directory_session_mut(id)
    }

    pub fn current_directory(&self) -> Option<&Path> {
        self.active_session().map(DirectorySession::directory)
    }

    pub fn media_items(&self) -> &[MediaEntry] {
        self.active_session().map_or(&[], DirectorySession::media_items)
    }

    pub fn current_index(&self) -> Option<usize> {
        self.active_session().and_then(DirectorySession::current_index)
    }

    pub fn current_entry(&self) -> Option<&MediaEntry> {
        self.active_session().and_then(DirectorySession::current_entry)
    }

    pub fn bookmarks(&self) -> &[BookmarkEntry] {
        &self.bookmarks
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
        self.sort_all_media_items();
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
        self.sort_all_media_items();
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
        self.active_session().map_or(1, |session| session.items_per_row.max(1))
    }

    pub fn set_library_items_per_row(&mut self, id: DirectoryId, items_per_row: usize) {
        // Keep this value always valid so keyboard move is safe.
        if let Some(session) = self.directory_session_mut(id) {
            session.items_per_row = items_per_row.max(1);
        }
    }

    pub fn activate_directory(&mut self, id: DirectoryId) {
        if self.active_directory_id == Some(id) || self.directory_session(id).is_none() {
            return;
        }
        self.active_directory_id = Some(id);
        self.needs_image_reload = true;
        self.current_image_size = None;
        self.clear_image_selection_state();
        self.sync_restore_config();
    }

    /// Select exactly one item (single-select). Moves the cursor, resets the
    /// selection set to this item, and asks the viewer to decode its image.
    pub fn select_index(&mut self, index: usize) {
        let changed = if let Some(session) = self.active_session_mut() {
            if index >= session.media_items.len() {
                false
            } else {
                let delta = index as i32 - session.current_index.unwrap_or(0) as i32;
                session.current_index = Some(index);
                Self::set_single_selection(session, Some(index));
                session.pending_scroll_to_selection = true;
                session.pending_scroll_direction = delta.signum();
                true
            }
        } else {
            false
        };
        if changed {
            self.needs_image_reload = true;
            self.clear_image_selection_state();
            self.sync_restore_config();
        }
    }

    pub fn advance_selection(&mut self, delta: i32) {
        let Some(current) = self.current_index() else {
            return;
        };
        let total = self.media_items().len();
        if total == 0 {
            return;
        }

        let next = ((current as i32 + delta).rem_euclid(total as i32)) as usize;
        if next != current {
            self.select_index(next);
        }
    }

    /// Reset the selection set so it holds only the file at `index` (or nothing
    /// when `index` is `None`). Also records that index as the range anchor.
    fn set_single_selection(session: &mut DirectorySession, index: Option<usize>) {
        session.selected_paths.clear();
        session.selection_anchor = index;
        if let Some(path) = index.and_then(|i| session.media_items.get(i)).map(|e| e.path.clone()) {
            session.selected_paths.insert(path);
        }
    }

    /// Toggle a single file in or out of the selection (Shift+click). Clicking an
    /// unselected file adds it; clicking a selected file removes it.
    pub fn toggle_selection_at(&mut self, index: usize) {
        let mut reload = false;
        let Some(session) = self.active_session_mut() else {
            return;
        };
        let Some(path) = session
            .media_items
            .get(index)
            .map(|entry| entry.path.clone())
        else {
            return;
        };

        if session.selected_paths.contains(&path) {
            session.selected_paths.remove(&path);
        } else {
            session.selected_paths.insert(path);
        }
        session.selection_anchor = Some(index);
        session.current_index = Some(index);
        session.pending_scroll_to_selection = true;
        session.pending_scroll_direction = 0;

        match session.selected_paths.len() {
            // Nothing selected: clear the cursor and the viewer.
            0 => {
                session.current_index = None;
                reload = true;
            }
            // Collapsed to one file: show it as a single image and move the
            // cursor onto it (it may differ from the file just clicked).
            1 => {
                if let Some(only) = session.selected_paths.iter().next().cloned()
                    && let Some(i) = session.media_items.iter().position(|e| e.path == only)
                {
                    session.current_index = Some(i);
                }
                reload = true;
            }
            // Still multiple: the viewer keeps showing the thumbnail grid.
            _ => {}
        }
        self.clear_image_selection_state();
        if reload {
            self.current_image_size = None;
            self.needs_image_reload = true;
        }
        self.sync_restore_config();
    }

    /// True when more than one file is selected (multi-select mode).
    pub fn is_multi_select(&self) -> bool {
        self.active_session().is_some_and(DirectorySession::is_multi_select)
    }

    /// Number of files currently selected.
    pub fn selected_count(&self) -> usize {
        self.active_session().map_or(0, DirectorySession::selected_count)
    }

    /// True when `path` is part of the current selection.
    pub fn is_path_selected(&self, path: &Path) -> bool {
        self.active_session().is_some_and(|session| session.is_path_selected(path))
    }

    /// Selected entries, yielded in library (display) order.
    pub fn selected_entries(&self) -> impl Iterator<Item = &MediaEntry> {
        self.media_items().iter().filter(|entry| self.is_path_selected(&entry.path))
    }

    /// Move only the keyboard cursor by `delta` without touching the selection.
    /// Used while multiple files are selected.
    pub fn move_cursor(&mut self, delta: i32) {
        let Some(session) = self.active_session_mut() else {
            return;
        };
        let Some(current) = session.current_index else {
            return;
        };
        let total = session.media_items.len();
        if total == 0 {
            return;
        }
        let next = ((current as i32 + delta).rem_euclid(total as i32)) as usize;
        if next != current {
            session.current_index = Some(next);
            session.pending_scroll_to_selection = true;
            session.pending_scroll_direction = (next as i32 - current as i32).signum();
        }
        self.sync_restore_config();
    }

    /// Move only the keyboard cursor to an absolute index (e.g. Home/End) while
    /// multiple files are selected.
    pub fn move_cursor_to(&mut self, index: usize) {
        let Some(session) = self.active_session_mut() else {
            return;
        };
        if index >= session.media_items.len() {
            return;
        }
        let delta = index as i32 - session.current_index.unwrap_or(0) as i32;
        session.current_index = Some(index);
        session.pending_scroll_to_selection = true;
        session.pending_scroll_direction = delta.signum();
        self.sync_restore_config();
    }

    /// Collapse a multi-selection down to the single file under the cursor
    /// (triggered by Spacebar).
    pub fn collapse_selection_to_cursor(&mut self) {
        if let Some(index) = self.current_index() {
            self.select_index(index);
        }
    }

    pub fn take_pending_library_scroll_to_selection(&mut self, id: DirectoryId) -> Option<i32> {
        let session = self.directory_session_mut(id)?;
        if !session.pending_scroll_to_selection {
            return None;
        }
        let direction = session.pending_scroll_direction;
        session.pending_scroll_to_selection = false;
        session.pending_scroll_direction = 0;
        Some(direction)
    }

    pub fn open_directory_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.load_directory(path, None);
        } else {
            self.status_message = "Directory selection cancelled".to_owned();
        }
    }

    /// Reveal the directory that is currently open in the system file manager
    /// (Finder on macOS, Explorer on Windows, the default handler on Linux).
    pub fn open_current_directory_in_file_manager(&mut self) {
        let Some(directory) = self.current_directory().map(Path::to_path_buf) else {
            self.status_message = "No directory is currently open".to_owned();
            return;
        };

        // Pick the right command for each operating system.
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("open").arg(&directory).spawn();
        #[cfg(target_os = "windows")]
        let result = std::process::Command::new("explorer").arg(&directory).spawn();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let result = std::process::Command::new("xdg-open").arg(&directory).spawn();

        match result {
            Ok(_) => {
                self.status_message = format!("Opened {}", directory.display());
            }
            Err(error) => {
                self.status_message = format!("Failed to open directory: {error}");
            }
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

    pub fn bookmark_current_directory(&mut self) {
        let Some(directory) = self.current_directory().map(Path::to_path_buf) else {
            self.status_message = "No directory is currently open".to_owned();
            return;
        };

        self.add_bookmark(directory);
    }

    pub fn bookmark_current_file(&mut self) {
        let Some(path) = self.current_entry().map(|entry| entry.path.clone()) else {
            self.status_message = "No file is currently selected".to_owned();
            return;
        };

        self.add_bookmark(path);
    }

    pub fn open_bookmark_path(&mut self, path: &Path) {
        if path.is_dir() {
            if self.load_directory(path.to_path_buf(), None) {
                self.status_message = format!("Opened bookmarked directory: {}", path.display());
            }
            return;
        }

        if path.is_file() {
            if let Some(parent) = path.parent() {
                self.set_show_library(true);
                if self.load_directory(parent.to_path_buf(), Some(path.to_path_buf())) {
                    self.status_message = format!("Opened bookmarked file: {}", path.display());
                }
            } else {
                self.status_message = format!(
                    "Bookmarked file has no parent directory: {}",
                    path.display()
                );
            }
            return;
        }

        self.status_message = format!("Bookmarked path was not found: {}", path.display());
    }

    pub fn request_delete_bookmark(&mut self, path: PathBuf) {
        self.pending_delete_bookmark_path = Some(path);
    }

    pub fn confirm_delete_bookmark(&mut self) {
        let Some(path) = self.pending_delete_bookmark_path.take() else {
            return;
        };

        let old_len = self.bookmarks.len();
        self.bookmarks.retain(|entry| entry.path != path);
        if self.bookmarks.len() != old_len {
            self.bookmarks_dirty = true;
            self.status_message = format!("Bookmark deleted: {}", path.display());
        }
    }

    pub fn cancel_delete_bookmark(&mut self) {
        self.pending_delete_bookmark_path = None;
    }

    pub fn flush_bookmarks_if_dirty(&mut self) {
        if !self.bookmarks_dirty {
            return;
        }

        match self.bookmark_store.replace_all(&self.bookmarks) {
            Ok(()) => {
                self.bookmarks_dirty = false;
            }
            Err(err) => {
                self.status_message = format!("Failed to save bookmarks: {err:#}");
                log::error!("Failed to save bookmarks: {err:#}");
            }
        }
    }

    pub fn refresh_current_directory(&mut self) {
        let Some(id) = self.active_directory_id else {
            self.status_message = "No directory is currently open".to_owned();
            return;
        };
        let Some(directory) = self
            .directory_session(id)
            .map(|session| session.directory.clone())
        else {
            return;
        };
        let focus_path = self
            .directory_session(id)
            .and_then(DirectorySession::current_entry)
            .map(|entry| entry.path.clone());

        let mut fresh = match media::scan_directory(&directory) {
            Ok(entries) => entries,
            Err(err) => {
                self.status_message =
                    format!("Failed to refresh {}: {:#}", directory.display(), err);
                log::error!("Failed to refresh {}: {:#}", directory.display(), err);
                return;
            }
        };

        self.cancel_thumbnail_work(id);

        let old_items = self
            .directory_session_mut(id)
            .map(|session| std::mem::take(&mut session.media_items))
            .unwrap_or_default();

        // Move old entries into a map so we can reuse their thumbnails.
        let mut old_by_path: HashMap<PathBuf, MediaEntry> = old_items
            .into_iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect();

        let mut added = 0usize;
        let mut updated = 0usize;
        for entry in fresh.iter_mut() {
            match old_by_path.remove(&entry.path) {
                Some(old) => {
                    if old.file_size == entry.file_size && old.modified_time == entry.modified_time
                    {
                        // Unchanged file: keep the existing thumbnail.
                        entry.thumbnail = old.thumbnail;
                    } else {
                        // Modified file: free the stale thumbnail, regenerate later.
                        updated += 1;
                        if let Some(thumb) = old.thumbnail {
                            self.pending_atlas_removals.push(thumb.atlas_image_id);
                        }
                    }
                }
                None => added += 1,
            }
        }

        // Whatever is left in the map belongs to deleted files.
        let removed = old_by_path.len();
        for (_, old) in old_by_path {
            if let Some(thumb) = old.thumbnail {
                self.pending_atlas_removals.push(thumb.atlas_image_id);
            }
        }

        let sort_field = self.library_sort_field;
        let sort_direction = self.sort_direction;
        if let Some(session) = self.directory_session_mut(id) {
            session.media_items = fresh;
            let existing: HashSet<PathBuf> = session
                .media_items
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            session.selected_paths.retain(|path| existing.contains(path));
            session.current_index = focus_path.as_ref().and_then(|target| {
                session
                    .media_items
                    .iter()
                    .position(|entry| &entry.path == target)
            });
            Self::sort_session(session, sort_field, sort_direction);

            if session.current_index.is_none() && !session.media_items.is_empty() {
                Self::set_single_selection(session, Some(0));
            } else if session.media_items.is_empty() {
                Self::set_single_selection(session, None);
            } else if !session.is_multi_select()
                && session
                    .current_entry()
                    .is_some_and(|entry| !session.selected_paths.contains(&entry.path))
            {
                Self::set_single_selection(session, session.current_index);
            }
        }

        self.current_image_size = None;
        self.needs_image_reload = true;
        self.clear_image_selection_state();

        self.status_message = format!(
            "Refreshed: {} added, {} updated, {} removed",
            added, updated, removed
        );

        // Only entries with `thumbnail: None` (new/modified) get regenerated.
        self.sync_restore_config();
        self.spawn_thumbnail_work(id);
    }

    /// Take the atlas ids of stale thumbnails so the render loop can free them.
    pub fn take_atlas_removals(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.pending_atlas_removals)
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

    fn cancel_thumbnail_work(&mut self, id: DirectoryId) {
        if let Some(handle) = self.worker_handles.remove(&id) {
            handle.abort();
        }
    }

    fn spawn_thumbnail_work(&mut self, id: DirectoryId) {
        self.cancel_thumbnail_work(id);
        let paths: Vec<PathBuf> = self
            .directory_session(id)
            .map(|session| {
                session
                    .media_items
                    .iter()
                    .filter(|entry| entry.thumbnail.is_none())
                    .map(|entry| entry.path.clone())
                    .collect()
            })
            .unwrap_or_default();

        if paths.is_empty() {
            return;
        }

        let tx = self.thumbnail_tx.clone();
        let handle = tokio::task::spawn(async move {
            for path in paths {
                // Decode off the async runtime because image codecs are blocking.
                let result = tokio::task::spawn_blocking({
                    let path = path.clone();
                    move || {
                        image_loader::load_thumbnail_rgba(
                            &path,
                            crate::constants::THUMBNAIL_IMAGE_SIZE,
                        )
                    }
                })
                .await;

                match result {
                    Ok(Ok(decoded)) => {
                        let msg = ThumbnailResult {
                            directory_id: id,
                            path,
                            width: decoded.width as u32,
                            height: decoded.height as u32,
                            pixels: decoded.pixels,
                        };
                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Ok(Err(err)) => {
                        log::warn!("Failed to load thumbnail for {}: {:#}", path.display(), err)
                    }
                    Err(err) => {
                        log::warn!("Thumbnail task panicked for {}: {:#}", path.display(), err)
                    }
                }
                tokio::task::yield_now().await;
            }
        });
        self.worker_handles.insert(id, handle);
    }

    /// Drain valid results for every open directory.
    pub fn poll_thumbnail_results(&mut self) -> Vec<ThumbnailResult> {
        let mut pending = Vec::new();
        while let Ok(result) = self.thumbnail_rx.try_recv() {
            pending.push(result);
        }
        pending
            .into_iter()
            .filter(|result| {
                self.directory_session(result.directory_id)
                    .is_some_and(|session| {
                        session
                            .media_items
                            .iter()
                            .any(|entry| entry.path == result.path)
                    })
            })
            .collect()
    }

    /// Upload thumbnail pixels and attach them to the matching directory entry.
    pub fn apply_thumbnail_info(
        &mut self,
        result: ThumbnailResult,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut imgui_wgpu::Renderer,
        imgui_textures: &mut ImguiTextures,
        texture_atlas: &mut TextureAtlasManager,
    ) {
        let Some(session) = self.directory_session(result.directory_id) else {
            return;
        };
        if !session
            .media_items
            .iter()
            .any(|entry| entry.path == result.path)
        {
            return;
        }

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
                if let Some(entry) = self.directory_session_mut(result.directory_id)
                    .and_then(|session| session.media_items.iter_mut().find(|entry| entry.path == result.path))
                {
                    entry.thumbnail = Some(ThumbnailInfo {
                        atlas_image_id: region.id,
                        texture_index: region.texture_id,
                        uvs: region.uvs,
                        image_size: region.image_size,
                    });
                } else {
                    texture_atlas.remove_image(renderer, imgui_textures, region.id);
                }
            }
            Err(err) => log::warn!(
                "Failed to upload thumbnail for {}: {:#}",
                result.path.display(),
                err
            ),
        }
    }

    fn queue_session_thumbnail_removals(&mut self, id: DirectoryId) {
        let ids: Vec<u64> = self
            .directory_session(id)
            .into_iter()
            .flat_map(|session| session.media_items.iter())
            .filter_map(|entry| entry.thumbnail.map(|thumbnail| thumbnail.atlas_image_id))
            .collect();
        self.pending_atlas_removals.extend(ids);
    }

    pub fn close_directory(&mut self, id: DirectoryId) {
        let Some(index) = self
            .directories
            .iter()
            .position(|session| session.id == id)
        else {
            return;
        };
        let was_active = self.active_directory_id == Some(id);
        let directory = self.directories[index].directory.clone();
        self.cancel_thumbnail_work(id);
        self.queue_session_thumbnail_removals(id);
        self.directories.remove(index);

        if was_active {
            self.active_directory_id = self
                .directories
                .get(index)
                .or_else(|| self.directories.last())
                .map(|session| session.id);
            self.current_image_size = None;
            self.needs_image_reload = true;
            self.clear_image_selection_state();
        }
        self.status_message = format!("Closed directory: {}", directory.display());
        self.sync_restore_config();
    }

    fn evict_oldest_if_full(&mut self) {
        if self.directories.len() == MAX_OPEN_DIRECTORIES {
            let oldest = self.directories[0].id;
            self.close_directory(oldest);
        }
    }

    /// Load images from a directory and choose which image to focus first.
    pub fn load_directory(&mut self, directory: PathBuf, focus_file: Option<PathBuf>) -> bool {
        let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
        let focus_file = focus_file.map(|path| std::fs::canonicalize(&path).unwrap_or(path));
        let directory_display = directory.display().to_string();
        if let Some(id) = self
            .directories
            .iter()
            .find(|session| session.directory == directory)
            .map(|session| session.id)
        {
            self.activate_directory(id);
            if let Some(target) = focus_file.as_ref()
                && let Some(index) = self.directory_session(id).and_then(|session| {
                    session
                        .media_items
                        .iter()
                        .position(|entry| &entry.path == target)
                })
            {
                self.select_index(index);
            }
            self.status_message = format!("Focused open directory: {}", directory_display);
            return true;
        }

        let entries = match media::scan_directory(&directory) {
            Ok(entries) => entries,
            Err(err) => {
                self.status_message = format!("Failed to read {}: {:#}", directory_display, err);
                log::error!("Failed to load directory {}: {:#}", directory_display, err);
                return false;
            }
        };

        self.evict_oldest_if_full();

        let id = DirectoryId(self.next_directory_id);
        self.next_directory_id += 1;
        let total = entries.len();
        let mut session = DirectorySession::new(id, directory, entries);
        Self::sort_session(&mut session, self.library_sort_field, self.sort_direction);
        let focus_index = focus_file.as_ref()
            .and_then(|target| session.media_items.iter().position(|entry| &entry.path == target))
            .or_else(|| (!session.media_items.is_empty()).then_some(0));
        session.current_index = focus_index;
        Self::set_single_selection(&mut session, focus_index);
        session.pending_scroll_to_selection = true;
        self.directories.push(session);
        self.active_directory_id = Some(id);
        self.current_image_size = None;
        self.needs_image_reload = true;
        self.clear_image_selection_state();
        self.status_message = if total == 0 {
            format!("No supported images in {}", directory_display)
        } else {
            format!("Loaded {} images from {}", total, directory_display)
        };
        self.sync_restore_config();
        self.spawn_thumbnail_work(id);
        true
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

        let entry = MediaEntry {
            path: file_path.clone(),
            file_name,
            format,
            file_size,
            modified_time,
            dimensions,
            thumbnail: None,
        };
        let id = DirectoryId(self.next_directory_id);
        self.next_directory_id += 1;
        let mut session = DirectorySession::new(id, directory, vec![entry]);
        session.current_index = Some(0);
        Self::set_single_selection(&mut session, Some(0));
        self.directories.push(session);
        self.active_directory_id = Some(id);
        self.current_image_size = None;
        self.needs_image_reload = true;
        self.status_message = format!("Loaded 1 image: {}", file_path.display());
        self.clear_image_selection_state();
        self.sync_restore_config();

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

    fn sort_all_media_items(&mut self) {
        let sort_field = self.library_sort_field;
        let sort_direction = self.sort_direction;
        for session in &mut self.directories {
            Self::sort_session(session, sort_field, sort_direction);
        }
        self.sync_restore_config();
    }

    fn sort_session(
        session: &mut DirectorySession,
        sort_field: LibrarySortField,
        sort_direction: SortDirection,
    ) {
        let selected_path = session.current_entry().map(|entry| entry.path.clone());
        session.media_items.sort_by(|a, b| {
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

        session.current_index = selected_path.as_ref().and_then(|target| {
            session.media_items
                .iter()
                .position(|entry| &entry.path == target)
        });
        session.selection_anchor = session.current_index;
    }

    fn sync_restore_config(&mut self) {
        self.config.open_directories = self
            .directories
            .iter()
            .map(|session| OpenDirectoryConfig {
                path: session.directory.clone(),
                focused_file: session.current_entry().map(|entry| entry.path.clone()),
            })
            .collect();
        self.config.active_directory = self.current_directory().map(Path::to_path_buf);
        self.config.last_open_file = self.current_entry().map(|entry| entry.path.clone());
    }

    /// Restore all saved directory rows. Old settings fall back to last_open_file.
    pub fn restore_saved_directories(&mut self) {
        if !self.config.restore_last_directory {
            return;
        }

        let saved = self.config.open_directories.clone();
        let active = self.config.active_directory.clone();
        let legacy_file = self.config.last_open_file.clone();

        for entry in saved.into_iter().take(MAX_OPEN_DIRECTORIES) {
            if entry.path.is_dir() {
                self.load_directory(entry.path, entry.focused_file);
            } else {
                log::warn!("Configured open directory is unavailable: {}", entry.path.display());
            }
        }

        if self.directories.is_empty() {
            if let Some(file) = legacy_file
                && file.is_file()
                && let Some(parent) = file.parent().map(Path::to_path_buf)
            {
                self.load_directory(parent, Some(file));
            }
            return;
        }

        if let Some(active_path) = active {
            let normalized = std::fs::canonicalize(&active_path).unwrap_or(active_path);
            if let Some(id) = self
                .directories
                .iter()
                .find(|session| session.directory == normalized)
                .map(|session| session.id)
            {
                self.activate_directory(id);
            }
        }
        self.sync_restore_config();
    }

    pub fn copy_region_to_clipboard(&self, selection: Option<Rect2D>) {
        let selection = selection.or_else(|| self.image_selection());
        crate::core::helper::copy_region_to_clipboard(selection, self.current_texture.as_ref());
    }

    fn add_bookmark(&mut self, path: PathBuf) {
        let had_pending_delete = self.bookmarks_dirty;
        let bookmarked_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let entry = BookmarkEntry::new(path.clone(), bookmarked_at);

        self.bookmarks.retain(|saved| saved.path != path);
        self.bookmarks.push(entry.clone());
        sort_bookmarks(&mut self.bookmarks);

        match self.bookmark_store.save_entry(&entry) {
            Ok(()) => {
                self.bookmarks_dirty = had_pending_delete;
                self.status_message = format!("Bookmark saved: {}", path.display());
            }
            Err(err) => {
                self.bookmarks_dirty = true;
                self.status_message = format!("Failed to save bookmark: {err:#}");
                log::error!("Failed to save bookmark {}: {err:#}", path.display());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_state() -> ViewerState {
        ViewerState::new(
            PathBuf::from("/tmp/just-image-viewer-tests/settings.toml"),
            AppConfig::default(),
        )
    }

    fn media_entry(directory: &str, name: &str) -> MediaEntry {
        MediaEntry {
            path: PathBuf::from(directory).join(name),
            file_name: name.to_owned(),
            format: media::MediaFormat::Png,
            file_size: 1,
            modified_time: Duration::default(),
            dimensions: Some((1, 1)),
            thumbnail: None,
        }
    }

    fn session(id: u64, directory: &str, names: &[&str]) -> DirectorySession {
        DirectorySession::new(
            DirectoryId(id),
            PathBuf::from(directory),
            names.iter().map(|name| media_entry(directory, name)).collect(),
        )
    }

    #[test]
    fn switching_directories_keeps_each_selection() {
        let mut state = test_state();
        state.directories = vec![
            session(1, "/first", &["a.png", "b.png"]),
            session(2, "/second", &["c.png", "d.png"]),
        ];
        state.active_directory_id = Some(DirectoryId(1));

        state.select_index(1);
        state.activate_directory(DirectoryId(2));
        state.select_index(0);
        state.activate_directory(DirectoryId(1));

        assert_eq!(
            state.current_entry().map(|entry| entry.file_name.as_str()),
            Some("b.png")
        );
        assert!(state.is_path_selected(Path::new("/first/b.png")));
        assert_eq!(state.config.open_directories.len(), 2);
    }

    #[test]
    fn closing_active_directory_focuses_the_next_row() {
        let mut state = test_state();
        state.directories = vec![
            session(1, "/first", &[]),
            session(2, "/second", &[]),
            session(3, "/third", &[]),
        ];
        state.active_directory_id = Some(DirectoryId(2));

        state.close_directory(DirectoryId(2));
        assert_eq!(state.active_directory_id(), Some(DirectoryId(3)));

        state.close_directory(DirectoryId(3));
        assert_eq!(state.active_directory_id(), Some(DirectoryId(1)));
    }

    #[test]
    fn full_collection_evicts_the_oldest_row() {
        let mut state = test_state();
        state.directories = vec![
            session(1, "/first", &[]),
            session(2, "/second", &[]),
            session(3, "/third", &[]),
        ];
        state.active_directory_id = Some(DirectoryId(3));

        state.evict_oldest_if_full();

        let paths: Vec<&Path> = state.directories.iter().map(DirectorySession::directory).collect();
        assert_eq!(paths, vec![Path::new("/second"), Path::new("/third")]);
        assert_eq!(state.active_directory_id(), Some(DirectoryId(3)));
    }

    #[test]
    fn thumbnail_results_are_routed_by_directory_and_path() {
        let mut state = test_state();
        state.directories = vec![session(1, "/first", &["a.png"])];

        let make_result = |directory_id, path: &str| ThumbnailResult {
            directory_id: DirectoryId(directory_id),
            path: PathBuf::from(path),
            width: 1,
            height: 1,
            pixels: Arc::from([255, 255, 255, 255]),
        };
        state
            .thumbnail_tx
            .try_send(make_result(1, "/first/a.png"))
            .expect("valid result should be queued");
        state
            .thumbnail_tx
            .try_send(make_result(2, "/second/b.png"))
            .expect("stale result should be queued before filtering");

        let results = state.poll_thumbnail_results();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].directory_id, DirectoryId(1));
        assert_eq!(results[0].path, Path::new("/first/a.png"));
    }
}
