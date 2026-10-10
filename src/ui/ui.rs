use crate::app::{
    DirectoryId, DirectorySession, ImageViewMode, LibrarySortField, SortDirection, ViewerState,
    format_file_size,
};
use crate::core::media::MediaEntry;
use crate::render::app_resources::AppResources;
use imgui::{Condition, ImColor32, MouseButton, MouseCursor, StyleColor, StyleVar, TableFlags, Ui};

use super::bookmark_window::render_bookmark_window;
use super::keyboard_shortcuts_window::render_keyboard_shortcuts_window;
use super::layout_constants::{
    MIN_LIBRARY_WIDTH, MIN_VIEWER_WIDTH, SPLITTER_WIDTH,
    grid_cell_size,
};
use super::view_panel::ViewPanel;

const LIBRARY_SORT_FIELDS: [&str; 3] = ["Name", "Date", "Size"];
const LIBRARY_SORT_DIRECTIONS: [&str; 2] = ["Ascending", "Descending"];

#[cfg(target_os = "macos")]
const OPEN_IN_FILE_MANAGER_LABEL: &str = "Open In Finder";
#[cfg(target_os = "windows")]
const OPEN_IN_FILE_MANAGER_LABEL: &str = "Open In Explorer";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const OPEN_IN_FILE_MANAGER_LABEL: &str = "Open In File Manager";

// Library row colors: a very light tint while hovering and a slightly stronger
// tint for selected rows, so multi-selection reads clearly.
const LIBRARY_HOVER_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.12];
const LIBRARY_SELECTED_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.28];

pub fn render_ui(
    ui: &imgui::Ui,
    app_state: &mut ViewerState,
    view_panel: &mut ViewPanel,
    is_pending: bool,
    app_resources: &AppResources,
    running: &mut bool,
) {
    render_main_menu_bar(ui, app_state, view_panel, running);

    let display = ui.io().display_size;
    // Compute heights using the effective font size so scaled fonts keep layout tight.
    let menu_height = scaled_frame_height(ui);
    let status_height = scaled_frame_height_with_spacing(ui) + scaled_constant(ui, 6.0)+ app_state.config().ui_font_size_pt;
    let content_height = (display[1] - menu_height - status_height).max(120.0);
    let window_flags = imgui::WindowFlags::NO_MOVE
        | imgui::WindowFlags::NO_RESIZE
        | imgui::WindowFlags::NO_COLLAPSE
        | imgui::WindowFlags::NO_TITLE_BAR
        | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;

    let mut clicked_item: Option<(DirectoryId, usize)> = None;
    let mut activate_directory: Option<DirectoryId> = None;
    let mut close_directory: Option<DirectoryId> = None;
    let mut force_scroll_to_selected = false;

    let _style_token = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));

    ui.window("MainLayout")
        .position([0.0, menu_height], Condition::Always)
        .size([display[0], content_height], Condition::Always)
        .flags(window_flags)
        .build(|| {
            if app_state.show_library() {
                let available_width = display[0];
                let splitter_width = SPLITTER_WIDTH;
                let minimum_total = MIN_LIBRARY_WIDTH + MIN_VIEWER_WIDTH;

                // Clamp logic
                let current_width = app_state.library_width();
                let clamped_width = if available_width - splitter_width > minimum_total {
                    current_width.clamp(
                        MIN_LIBRARY_WIDTH,
                        available_width - splitter_width - MIN_VIEWER_WIDTH,
                    )
                } else {
                    (available_width - splitter_width) * 0.5
                };

                // Only update if changed significantly (avoid cycles), but here we just use it for rendering
                // We do NOT update app_state here to avoid fighting with the splitter logic below,
                // unless it's out of bounds.
                if (current_width - clamped_width).abs() > 0.1 {
                    app_state.set_library_width(clamped_width);
                }

                ui.child_window("LibraryPanel")
                    .size([clamped_width, 0.0])
                    .border(true)
                    .build(|| {
                        let _pad = ui.push_style_var(StyleVar::ItemSpacing([4.0, 4.0]));
                        ui.text("Sort:");
                        ui.same_line();
                        let mut sort_field_index = match app_state.library_sort_field() {
                            LibrarySortField::Name => 0,
                            LibrarySortField::Date => 1,
                            LibrarySortField::Size => 2,
                        };
                        ui.set_next_item_width(88.0);
                        if ui.combo_simple_string(
                            "##library_sort_field",
                            &mut sort_field_index,
                            &LIBRARY_SORT_FIELDS,
                        ) {
                            let field = match sort_field_index {
                                1 => LibrarySortField::Date,
                                2 => LibrarySortField::Size,
                                _ => LibrarySortField::Name,
                            };
                            app_state.set_library_sort_field(field);
                        }
                        ui.same_line();
                        let mut sort_direction_index = match app_state.sort_direction() {
                            SortDirection::Ascending => 0,
                            SortDirection::Descending => 1,
                        };
                        ui.set_next_item_width(96.0);
                        if ui.combo_simple_string(
                            "##library_sort_direction",
                            &mut sort_direction_index,
                            &LIBRARY_SORT_DIRECTIONS,
                        ) {
                            let direction = if sort_direction_index == 1 {
                                SortDirection::Descending
                            } else {
                                SortDirection::Ascending
                            };
                            app_state.set_sort_direction(direction);
                        }
                        let mut show_thumbnail = app_state.show_thumbnail();
                        if ui.checkbox("Thumbnail", &mut show_thumbnail) {
                            app_state.set_show_thumbnail(show_thumbnail);
                            force_scroll_to_selected = true;
                        }
                        ui.same_line();
                        let mut show_grid_view = app_state.show_grid_view();
                        if ui.checkbox("Grid", &mut show_grid_view) {
                            app_state.set_show_grid_view(show_grid_view);
                            force_scroll_to_selected = true;
                        }
                        ui.separator();
                        // Reserve two action rows below the directory lists.
                        let buttons_height =
                            2.0 * ui.frame_height() + ui.clone_style().item_spacing[1] + 8.0;
                        let ids: Vec<DirectoryId> = app_state
                            .directory_sessions()
                            .iter()
                            .map(DirectorySession::id)
                            .collect();
                        let available_rows_height =
                            (ui.content_region_avail()[1] - buttons_height).max(0.0);

                        if ids.is_empty() {
                            ui.child_window("empty_library")
                                .size([0.0, available_rows_height])
                                .border(true)
                                .build(|| {
                                    ui.text_wrapped(
                                        "Drag a directory/file or use File > Open Directory",
                                    )
                                });
                        } else {
                            let spacing = ui.clone_style().item_spacing[1];
                            // Library thumbnail size is user-configurable in settings.toml.
                            let thumbnail_size = app_state.config().library_thumbnail_size;
                            let row_height = ((available_rows_height
                                - spacing * (ids.len() - 1) as f32)
                                / ids.len() as f32)
                                .max(1.0);

                            for id in ids {
                                let mut pending_scroll_direction =
                                    app_state.take_pending_library_scroll_to_selection(id);
                                if force_scroll_to_selected {
                                    pending_scroll_direction = Some(0);
                                }
                                let items_per_row = calculate_library_items_per_row(
                                    ui,
                                    app_state.show_grid_view(),
                                    thumbnail_size,
                                );
                                app_state.set_library_items_per_row(id, items_per_row);

                                let is_active = app_state.active_directory_id() == Some(id);
                                let row_id = format!("directory_row_{:?}", id);
                                ui.child_window(&row_id)
                                    .size([0.0, row_height])
                                    .border(true)
                                    .build(|| {
                                        let Some(session) = app_state.directory_session(id) else {
                                            return;
                                        };
                                        let header_color = if is_active {
                                            LIBRARY_SELECTED_COLOR
                                        } else {
                                            [1.0, 1.0, 1.0, 0.0]
                                        };
                                        let header = format!(
                                            "{}  ({} items)##header_{:?}",
                                            session.directory().display(),
                                            session.media_items().len(),
                                            id
                                        );
                                        // Keep the close button outside the header hit area.
                                        let close_width = ui.calc_text_size("X")[0]
                                            + ui.clone_style().frame_padding[0] * 2.0;
                                        let header_width = (ui.content_region_avail()[0]
                                            - close_width
                                            - ui.clone_style().item_spacing[0])
                                            .max(1.0);
                                        let _header_token =
                                            ui.push_style_color(StyleColor::Header, header_color);
                                        let header_clicked = ui
                                            .selectable_config(&header)
                                            .selected(is_active)
                                            .size([header_width, 0.0])
                                            .build();
                                        if header_clicked {
                                            activate_directory = Some(id);
                                        }
                                        ui.same_line();
                                        if ui.small_button(format!("X##close_{:?}", id)) {
                                            close_directory = Some(id);
                                        }
                                        drop(_header_token);

                                        ui.separator();
                                        let list_id = format!("directory_list_{:?}", id);
                                        ui.child_window(&list_id).size([0.0, 0.0]).build(|| {
                                            let _hover_token = ui.push_style_color(
                                                StyleColor::HeaderHovered,
                                                LIBRARY_HOVER_COLOR,
                                            );
                                            let _selected_token = ui.push_style_color(
                                                StyleColor::Header,
                                                LIBRARY_SELECTED_COLOR,
                                            );
                                            let _active_token = ui.push_style_color(
                                                StyleColor::HeaderActive,
                                                LIBRARY_SELECTED_COLOR,
                                            );

                                            if app_state.show_grid_view() {
                                                if let Some(index) = render_library_grid(
                                                    ui,
                                                    session,
                                                    app_state.show_thumbnail(),
                                                    app_resources,
                                                    items_per_row,
                                                    thumbnail_size,
                                                    &mut pending_scroll_direction,
                                                ) {
                                                    clicked_item = Some((id, index));
                                                }
                                            } else {
                                                for (index, entry) in
                                                    session.media_items().iter().enumerate()
                                                {
                                                    if render_library_item_row(
                                                        ui,
                                                        session,
                                                        app_state.library_width(),
                                                        app_state.show_thumbnail(),
                                                        app_resources,
                                                        index,
                                                        entry,
                                                        thumbnail_size,
                                                    ) {
                                                        clicked_item = Some((id, index));
                                                    }
                                                    handle_scroll_to_selected(
                                                        ui,
                                                        session.current_index(),
                                                        index,
                                                        &mut pending_scroll_direction,
                                                    );
                                                }
                                            }
                                            if ui.is_window_hovered()
                                                && ui.is_mouse_clicked(MouseButton::Left)
                                                && clicked_item.is_none()
                                            {
                                                activate_directory = Some(id);
                                            }
                                        });
                                    });
                            }
                        }
                        if ui.button("Open Directory") {
                            app_state.open_directory_dialog();
                        }
                        ui.same_line();
                        if ui.button("Refresh") {
                            app_state.refresh_current_directory();
                        }
                        if ui.button("+D") {
                            app_state.bookmark_current_directory();
                        }
                        ui.same_line();
                        if ui.button("+F") {
                            app_state.bookmark_current_file();
                        }
                        ui.same_line();
                        if ui.button("Bookmark") {
                            app_state.set_show_bookmark_window(true);
                        }
                        ui.same_line();
                        if ui.button(OPEN_IN_FILE_MANAGER_LABEL) {
                            app_state.open_current_directory_in_file_manager();
                        }
                    });

                ui.same_line();

                // Splitter
                ui.invisible_button("splitter", [splitter_width, ui.content_region_avail()[1]]);
                if ui.is_item_active() {
                    let available = (display[0] - splitter_width).max(0.0);
                    let next = if available > minimum_total {
                        (app_state.library_width() + ui.io().mouse_delta[0])
                            .clamp(MIN_LIBRARY_WIDTH, available - MIN_VIEWER_WIDTH)
                    } else {
                        available * 0.5
                    };
                    app_state.set_library_width(next);
                }
                if ui.is_item_hovered() {
                    ui.set_mouse_cursor(Some(MouseCursor::ResizeEW));
                }

                ui.same_line();
            }

            view_panel.render(ui, app_state, app_resources, is_pending);
        });

    ui.window("Status")
        .position([0.0, menu_height + content_height], Condition::Always)
        .size([display[0], status_height], Condition::Always)
        .flags(window_flags | imgui::WindowFlags::NO_TITLE_BAR)
        .build(|| {
            ui.text(format!("Status: {}", app_state.status_message()));
            ui.same_line();
            if is_pending {
                ui.text("| Loading...");
            }
        });

    if app_state.show_keyboard_shortcuts() {
        let mut open = true;
        render_keyboard_shortcuts_window(ui, &mut open);
        app_state.set_show_keyboard_shortcuts(open);
    }
    render_bookmark_window(ui, app_state);
    view_panel.render_aux_windows(ui, app_state);

    if let Some(id) = close_directory {
        app_state.close_directory(id);
    } else if let Some((id, index)) = clicked_item {
        // Plain click selects one; Shift+click toggles that single file.
        app_state.activate_directory(id);
        if ui.io().key_shift {
            app_state.toggle_selection_at(index);
        } else {
            app_state.select_index(index);
        }
    } else if let Some(id) = activate_directory {
        app_state.activate_directory(id);
    }
}

fn render_main_menu_bar(
    ui: &imgui::Ui,
    app_state: &mut ViewerState,
    view_panel: &mut ViewPanel,
    running: &mut bool,
) {
    ui.main_menu_bar(|| {
        ui.menu("File", || {
            if ui.menu_item("Open Directory...") {
                app_state.open_directory_dialog();
            }
            ui.menu("Recent Directories", || {
                let mut selected = None;
                let config = app_state.config();
                if app_state.recent_directories().is_empty() || config.recent_directory_count == 0 {
                    ui.menu_item_config("No recent directories").enabled(false).build();
                }
                for (index, path) in app_state.recent_directories().iter()
                    .take(config.recent_directory_count).enumerate()
                {
                    let _id = ui.push_id_usize(index);
                    if ui.menu_item(path.to_string_lossy()) {
                        selected = Some(path.clone());
                    }
                }
                if let Some(path) = selected {
                    app_state.load_directory(path, None);
                }
            });
            if ui.menu_item("Quit") {
                *running = false;
            }
        });
        ui.menu("View", || {
            ui.menu("Image", || {
                let image_mode = app_state.image_view_mode();
                if ui
                    .selectable_config("Original")
                    .selected(image_mode == ImageViewMode::Original)
                    .build()
                {
                    app_state.set_image_view_mode(ImageViewMode::Original);
                }
                if ui
                    .selectable_config("Fit to Window")
                    .selected(image_mode == ImageViewMode::FitToWindow)
                    .build()
                {
                    app_state.set_image_view_mode(ImageViewMode::FitToWindow);
                }
                if ui
                    .selectable_config("Fit to Width")
                    .selected(image_mode == ImageViewMode::FitToWidth)
                    .build()
                {
                    app_state.set_image_view_mode(ImageViewMode::FitToWidth);
                }
            });
        });
        ui.menu("Window", || {
            ui.menu("Layout", || {
                let mut show_library = app_state.show_library();
                if ui
                    .menu_item_config("Library")
                    .selected(show_library)
                    .build()
                {
                    show_library = !show_library;
                    app_state.set_show_library(show_library);
                }

                let mut show_info = app_state.show_info();
                if ui.menu_item_config("Info").selected(show_info).build() {
                    show_info = !show_info;
                    app_state.set_show_info(show_info);
                }
            });
            view_panel.render_window_menu(ui, app_state);
        });
        ui.menu("Help", || {
            if ui.menu_item("Keyboard Shortcuts") {
                app_state.set_show_keyboard_shortcuts(true);
            }
        });
    });
}

pub fn file_info_text(entry:Option<&MediaEntry>) -> String {
    if let Some(entry) = entry {
        let dimensions_text = match entry.dimensions {
            Some((width, height)) => format!("{width} x {height}"),
            None => "(Unknown)".to_owned(),
        };

        format!("{}\n{}\n{}\n{}",
            entry.file_name,
            entry.format.as_str(),
            format_file_size(entry.file_size),
            dimensions_text
        )
    } else {
        "None".to_string()
    }
}

/// Resolve the thumbnail texture info for an entry, falling back to the empty icon.
pub(super) fn resolve_thumbnail<'a>(
    entry: &'a MediaEntry,
    app_resources: &'a AppResources,
) -> (imgui::TextureId, [f32; 4], u32, u32) {
    if let Some(thumbnail) = &entry.thumbnail {
        let (w, h) = thumbnail.image_size;
        (thumbnail.texture_index, thumbnail.uvs, w, h)
    } else {
        let region = &app_resources.empty_icon_region;
        let (w, h) = region.image_size;
        (region.texture_id, region.uvs, w, h)
    }
}

/// Fit-scale a source image into a square cell of `cell` pixels.
pub(super) fn fit_scale_in_cell(img_w: u32, img_h: u32, cell: f32) -> (f32, f32) {
    let scale = (cell / img_w as f32).min(cell / img_h as f32);
    (img_w as f32 * scale, img_h as f32 * scale)
}

fn render_library_item_row(
    ui: &Ui,
    session: &DirectorySession,
    library_width: f32,
    show_thumbnail: bool,
    app_resources: &AppResources,
    index: usize,
    entry: &MediaEntry,
    thumbnail_size: f32,
) -> bool {
    let current_width = library_width - 32.0;
    let thumbnail_size_xy = [thumbnail_size, thumbnail_size];
    if show_thumbnail {
        let image_view_id = format!("thumbnail_image_view_{index}");
        let mut thumbnail_clicked = false;
        ui.child_window(&image_view_id)
            .size(thumbnail_size_xy)
            .border(false)
            .build(|| {
                let cell = thumbnail_size;
                let (texture_id, uvs, img_w, img_h) = resolve_thumbnail(entry, app_resources);
                let (draw_w, draw_h) = fit_scale_in_cell(img_w, img_h, cell);
                let cursor = ui.cursor_pos();
                ui.set_cursor_pos([
                    cursor[0] + (cell - draw_w) * 0.5,
                    cursor[1] + (cell - draw_h) * 0.5,
                ]);
                imgui::Image::new(texture_id, [draw_w, draw_h])
                    .uv0([uvs[0], uvs[1]])
                    .uv1([uvs[2], uvs[3]])
                    .build(ui);

                if ui.is_window_hovered() && ui.is_mouse_clicked(MouseButton::Left) {
                    thumbnail_clicked = true;
                }
            });
        ui.same_line();

        let file_info = file_info_text(Some(entry));
        let selectable_label = format!("{}##library_item_{index}", file_info);
        let text_clicked = ui
            .selectable_config(&selectable_label)
            .selected(session.is_path_selected(&entry.path))
            .size([
                (current_width - thumbnail_size_xy[0]) as f32,
                thumbnail_size_xy[1],
            ])
            .build();
        draw_cursor_outline_if_needed(ui, session, index);
        return thumbnail_clicked || text_clicked;
    } else {
        let clicked = ui
            .selectable_config(&entry.file_name)
            .selected(session.is_path_selected(&entry.path))
            .build();
        draw_cursor_outline_if_needed(ui, session, index);
        clicked
    }
}

/// While multiple files are selected, outline the row under the keyboard cursor
/// so the user can see where Spacebar will collapse the selection.
fn draw_cursor_outline_if_needed(ui: &Ui, session: &DirectorySession, index: usize) {
    if session.is_multi_select() && session.current_index() == Some(index) {
        ui.get_window_draw_list()
            .add_rect(
                ui.item_rect_min(),
                ui.item_rect_max(),
                ImColor32::from_rgba(255, 255, 255, 180),
            )
            .thickness(1.5)
            .build();
    }
}

fn handle_scroll_to_selected(
    ui: &Ui,
    current_index: Option<usize>,
    index: usize,
    pending: &mut Option<i32>,
) {
    if current_index == Some(index)
        && let Some(direction) = *pending
    {
        if !ui.is_item_visible() {
            let ratio = if direction < 0 {
                0.2
            } else if direction > 0 {
                0.8
            } else {
                0.5
            };
            ui.set_scroll_here_y_with_ratio(ratio);
        }
        *pending = None;
    }
}

fn render_library_grid(
    ui: &Ui,
    session: &DirectorySession,
    show_thumbnail: bool,
    app_resources: &AppResources,
    cols: usize,
    thumbnail_size: f32,
    pending_scroll_direction: &mut Option<i32>,
) -> Option<usize> {
    let cell = grid_cell_size(thumbnail_size);
    // Cell height: thumbnail area + label row, or a thumbnail-sized text box when hidden.
    let label_h = ui.frame_height_with_spacing();
    let cell_h = if show_thumbnail {
        thumbnail_size + label_h
    } else {
        thumbnail_size
    };
    let mut clicked: Option<usize> = None;
    let current_index = session.current_index();

    let flags = TableFlags::NO_BORDERS_IN_BODY
        | TableFlags::NO_BORDERS_IN_BODY_UNTIL_RESIZE
        | TableFlags::PAD_OUTER_X;
    let token = ui.begin_table_with_flags("grid_table", cols, flags);
    if token.is_none() {
        return None;
    }

    for i in 0..cols {
        ui.table_setup_column(&format!("col_{i}"));
    }

    for (index, entry) in session.media_items().iter().enumerate() {
        let col = index % cols;
        if col == 0 {
            ui.table_next_row();
        }
        ui.table_set_column_index(col);

        let is_selected = session.is_path_selected(&entry.path);
        let selectable_id = format!("##grid_item_{index}");

        // Record top-left of this cell before drawing
        let cell_origin = ui.cursor_screen_pos();
        let cursor_pos = ui.cursor_pos();

        // Draw the selectable spanning the full cell area first
        if ui
            .selectable_config(&selectable_id)
            .selected(is_selected)
            .size([cell, cell_h])
            .build()
        {
            clicked = Some(index);
        }
        draw_cursor_outline_if_needed(ui, session, index);
        handle_scroll_to_selected(ui, current_index, index, pending_scroll_direction);
        if ui.is_item_hovered() {
            let text = file_info_text(Some(entry));
            ui.tooltip_text(text);
        }

        // Draw thumbnail image or placeholder on top via draw_list
        if show_thumbnail {
            let (texture_id, uvs, img_w, img_h) = resolve_thumbnail(entry, app_resources);
            let (draw_w, draw_h) = fit_scale_in_cell(img_w, img_h, thumbnail_size);
            let img_x = cell_origin[0] + ((cell - draw_w) * 0.5).max(0.0);
            let img_y = cell_origin[1] + ((thumbnail_size - draw_h) * 0.5).max(0.0);
            ui.get_window_draw_list()
                .add_image(texture_id, [img_x, img_y], [img_x + draw_w, img_y + draw_h])
                .uv_min([uvs[0], uvs[1]])
                .uv_max([uvs[2], uvs[3]])
                .build();
        }

        // Draw file name label below the thumbnail (or at top if no thumbnail)
        let label_y_offset = if show_thumbnail { thumbnail_size } else { 0.0 };
        let label_pos = [cursor_pos[0] + 2.0, cursor_pos[1] + label_y_offset + 2.0];
        ui.set_cursor_pos(label_pos);
        let label_w = cell - 4.0;
        let max_lines = if show_thumbnail {
            1
        } else {
            let line_h = ui.frame_height_with_spacing().max(1.0);
            ((thumbnail_size - 4.0) / line_h).floor().max(1.0) as usize
        };
        let display_name = wrap_text_to_width_and_lines(ui, &entry.file_name, label_w, max_lines);
        ui.text(&display_name);
    }

    clicked
}

fn calculate_library_items_per_row(ui: &Ui, show_grid_view: bool, thumbnail_size: f32) -> usize {
    if !show_grid_view {
        return 1;
    }
    // Use the current scroll area width to get real visible column count.
    let cell = grid_cell_size(thumbnail_size);
    let available_width = ui.content_region_avail()[0].max(cell);
    ((available_width / cell).floor() as usize).max(1)
}

/// Wrap `text` to fit within `max_width` pixels and `max_lines` lines.
/// If the last line doesn't fit, it will be truncated with "...".
/// If `max_lines` is 1, this acts like a simple truncate with ellipsis.
fn wrap_text_to_width_and_lines(ui: &Ui, text: &str, max_width: f32, max_lines: usize) -> String {
    if text.is_empty() || max_lines == 0 {
        return String::new();
    }

    let ellipsis = "...";
    let ellipsis_w = ui.calc_text_size(ellipsis)[0];
    let mut remaining = text.trim();
    let mut lines: Vec<String> = Vec::with_capacity(max_lines);

    for line_index in 0..max_lines {
        if remaining.is_empty() {
            break;
        }

        let full_width = ui.calc_text_size(remaining)[0];
        if full_width <= max_width {
            lines.push(remaining.to_owned());
            break;
        }

        // Last line: truncate with ellipsis
        if line_index == max_lines - 1 {
            let mut end = remaining.len();
            while end > 0 {
                // Step back one char boundary at a time
                end -= 1;
                while !remaining.is_char_boundary(end) {
                    end -= 1;
                }
                let candidate = &remaining[..end];
                if ui.calc_text_size(candidate)[0] + ellipsis_w <= max_width {
                    lines.push(format!("{candidate}{ellipsis}"));
                    break;
                }
            }
            if lines.len() == line_index {
                lines.push(ellipsis.to_owned());
            }
            break;
        }

        // Find how many characters fit in this line (without ellipsis)
        let mut fit_end = 0usize;
        for (idx, ch) in remaining.char_indices() {
            let next = idx + ch.len_utf8();
            if ui.calc_text_size(&remaining[..next])[0] <= max_width {
                fit_end = next;
            } else {
                break;
            }
        }

        if fit_end == 0 {
            fit_end = remaining
                .char_indices()
                .nth(1)
                .map(|(idx, _)| idx)
                .unwrap_or(remaining.len());
        }

        let line = remaining[..fit_end].trim_end();
        lines.push(line.to_owned());
        remaining = remaining[fit_end..].trim_start();
    }

    lines.join("\n")
}

fn scaled_frame_height(ui: &Ui) -> f32 {
    scale_with_font_global(ui, ui.frame_height())
}

fn scaled_frame_height_with_spacing(ui: &Ui) -> f32 {
    scale_with_font_global(ui, ui.frame_height_with_spacing())
}

fn scaled_constant(ui: &Ui, value: f32) -> f32 {
    scale_with_font_global(ui, value)
}

fn scale_with_font_global(ui: &Ui, value: f32) -> f32 {
    let scale = ui.io().font_global_scale.max(0.01);
    value * scale
}
