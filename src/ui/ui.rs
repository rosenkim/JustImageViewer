use crate::app::{ImageViewMode, LibrarySortField, SortDirection, ViewerState, format_file_size};
use crate::core::media::MediaEntry;
use crate::infra::config::BackgroundMode;
use crate::math::{Point2D, Rect2D};
use crate::render::app_resources::AppResources;
use imgui::{Condition, ImColor32, MouseButton, MouseCursor, StyleVar, TableFlags, Ui};

use super::helper::render_image_selection_widget;
use super::keyboard_shortcuts_window::render_keyboard_shortcuts_window;
use super::layout_constants::{
    CHECKER_TILE_SIZE, GRID_CELL_SIZE, LIBRARY_THUMBNAIL_SIZE, MIN_LIBRARY_WIDTH,
    MIN_SELECTION_SIZE, MIN_VIEWER_WIDTH, SPLITTER_WIDTH,
};

const LIBRARY_SORT_FIELDS: [&str; 3] = ["Name", "Date", "Size"];
const LIBRARY_SORT_DIRECTIONS: [&str; 2] = ["Ascending", "Descending"];

pub fn render_ui(
    ui: &imgui::Ui,
    app_state: &mut ViewerState,
    is_pending: bool,
    app_resources: &AppResources,
    running: &mut bool,
) {
    render_main_menu_bar(ui, app_state, running);

    let display = ui.io().display_size;
    // Compute heights using the effective font size so scaled fonts keep layout tight.
    let menu_height = scaled_frame_height(ui);
    let status_height = scaled_frame_height_with_spacing(ui) + scaled_constant(ui, 6.0);
    let content_height = (display[1] - menu_height - status_height).max(120.0);
    let window_flags = imgui::WindowFlags::NO_MOVE
        | imgui::WindowFlags::NO_RESIZE
        | imgui::WindowFlags::NO_COLLAPSE
        | imgui::WindowFlags::NO_TITLE_BAR
        | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;

    let mut clicked_index: Option<usize> = None;
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
                        if let Some(directory) = app_state.current_directory() {
                            ui.text(format!("Directory: {}", directory.display()));
                            ui.text(format!("Items: {}", app_state.media_items().len()));
                        } else {
                            ui.text("Drag a directory/file or use File > Open Directory");
                        }
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
                        ui.child_window("library_scroll")
                            .size([0.0, -36.0])
                            .build(|| {
                                let mut pending_scroll_direction =
                                    app_state.take_pending_library_scroll_to_selection();
                                if force_scroll_to_selected {
                                    pending_scroll_direction = Some(0);
                                }
                                let items_per_row = calculate_library_items_per_row(ui, app_state);
                                app_state.set_library_items_per_row(items_per_row);

                                if app_state.show_grid_view() {
                                    if let Some(index) = render_library_grid(
                                        ui,
                                        app_state,
                                        app_resources,
                                        items_per_row,
                                        &mut pending_scroll_direction,
                                    ) {
                                        clicked_index = Some(index);
                                    }
                                } else {
                                    // render file list
                                    for (index, entry) in app_state.media_items().iter().enumerate()
                                    {
                                        if render_library_item_row(
                                            ui,
                                            app_state,
                                            app_resources,
                                            index,
                                            entry,
                                        ) {
                                            clicked_index = Some(index);
                                        }
                                        handle_scroll_to_selected(
                                            ui,
                                            app_state.current_index(),
                                            index,
                                            &mut pending_scroll_direction,
                                        );
                                    }
                                }
                            });
                        if ui.button("Open Directory...") {
                            app_state.open_directory_dialog();
                        }
                        ui.same_line();
                        if ui.button("Refresh") {
                            app_state.refresh_current_directory();
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

            ui.child_window("ViewerPanel")
                .size([0.0, 0.0])
                .border(true)
                .build(|| {
                    const INFO_WIDTH: f32 = 200.0;
                    let _pad = ui.push_style_var(StyleVar::ItemSpacing([4.0, 4.0]));
                    let mut image_width = ui.content_region_avail()[0];
                    let show_info = app_state.show_info() && image_width > INFO_WIDTH + SPLITTER_WIDTH;

                    if show_info {
                        image_width -= INFO_WIDTH + SPLITTER_WIDTH;
                    }

                    ui.child_window("image_region")
                        .size([image_width.max(100.0), 0.0])
                        .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
                        .build(|| {
                            render_image_content(ui, app_state, is_pending);
                        });

                    if show_info {
                        ui.same_line();
                        ui.invisible_button(
                            "info_splitter",
                            [SPLITTER_WIDTH, ui.content_region_avail()[1]],
                        );
                        if ui.is_item_hovered() {
                            ui.set_mouse_cursor(Some(MouseCursor::ResizeEW));
                        }
                        ui.same_line();

                        ui.child_window("info_region").size([0.0, 0.0]).build(|| {
                            render_file_info(ui, app_state.current_entry());
                        });
                    }
                });
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
    render_selection_window(ui, app_state);

    if let Some(index) = clicked_index {
        app_state.select_index(index);
    }
}

fn render_image_background(
    ui: &Ui,
    app_state: &ViewerState,
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
) {
    if image_display_size[0] <= 0.0 || image_display_size[1] <= 0.0 {
        return;
    }

    let style = &app_state.config().background_style;
    let (color1_rgb, color2_rgb) = style.resolved_colors_rgb();
    let color1 = rgb_to_im_color32(color1_rgb);
    let color2 = rgb_to_im_color32(color2_rgb);

    let draw_list = ui.get_window_draw_list();
    let min = image_screen_min;
    let max = [
        image_screen_min[0] + image_display_size[0],
        image_screen_min[1] + image_display_size[1],
    ];

    match style.mode {
        BackgroundMode::Solid => {
            draw_list.add_rect(min, max, color1).filled(true).build();
        }
        BackgroundMode::Checker => {
            let mut y = min[1];
            let mut row = 0usize;
            let y_end = max[1];
            while y < y_end {
                let y_next = (y + CHECKER_TILE_SIZE).min(y_end);
                let mut x = min[0];

                let mut col = 0usize;
                let x_end = max[0];
                while x < x_end {
                    let x_next = (x + CHECKER_TILE_SIZE).min(x_end);
                    let tile_color = if (row + col) % 2 == 0 { color1 } else { color2 };
                    draw_list
                        .add_rect([x, y], [x_next, y_next], tile_color)
                        .filled(true)
                        .build();
                    x = x_next;
                    col += 1;
                }
                y = y_next;
                row += 1;
            }
        }
    }
}

fn rgb_to_im_color32(rgb: [f32; 3]) -> ImColor32 {
    let r = (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8;
    ImColor32::from_rgba(r, g, b, 255)
}

fn render_main_menu_bar(ui: &imgui::Ui, app_state: &mut ViewerState, running: &mut bool) {
    ui.main_menu_bar(|| {
        ui.menu("File", || {
            if ui.menu_item("Open Directory...") {
                app_state.open_directory_dialog();
            }
            if ui.menu_item("Quit") {
                *running = false;
            }
        });
        ui.menu("View", || {
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
            let mut show_selection_window = app_state.show_selection_window();
            if ui
                .menu_item_config("Selection")
                .selected(show_selection_window)
                .build()
            {
                show_selection_window = !show_selection_window;
                app_state.set_show_selection_window(show_selection_window);
            }
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

pub fn render_file_info(ui: &imgui::Ui, entry:Option<&MediaEntry>) {
    if let Some(entry) = entry {
        ui.text_wrapped(format!("File: {}", entry.file_name));
        ui.text(format!("Format: {}", entry.format.as_str()));
        ui.text(format!("Size: {}", format_file_size(entry.file_size)));
        if let Some((w, h)) = entry.dimensions {
            ui.text(format!("Resolution: {} x {}", w, h));
        }
    } else {
        ui.text("No file selected");
    }
    ui.separator();
}

fn render_selection_window(ui: &Ui, app_state: &mut ViewerState) {
    if !app_state.show_selection_window() {
        return;
    }

    let mut open = true;
    ui.window("Selection")
        .opened(&mut open)
        .size([360.0, 280.0], Condition::FirstUseEver)
        .build(|| {
            let Some((image_w, image_h)) = app_state.current_image_size() else {
                ui.text("No image loaded.");
                return;
            };

            ui.text(format!("Image Size: {} x {}", image_w, image_h));
            ui.spacing();

            let Some(selection) = app_state.image_selection() else {
                ui.text("No selection.");
                ui.text("Drag on the image to create a selection.");
                return;
            };

            let mut edited = selection;
            let mut changed = false;
            let table_flags = TableFlags::BORDERS
                | TableFlags::SIZING_STRETCH_PROP
                | TableFlags::NO_SAVED_SETTINGS;

            ui.dummy([0.0, 8.0]);

            if let Some(_table) =
                ui.begin_table_with_flags("selection_property_grid", 2, table_flags)
            {
                changed |=
                    property_grid_float_row(ui, "Min X", "##selection_min_x", &mut edited.min.x);
                changed |=
                    property_grid_float_row(ui, "Min Y", "##selection_min_y", &mut edited.min.y);
                changed |=
                    property_grid_float_row(ui, "Max X", "##selection_max_x", &mut edited.max.x);
                changed |=
                    property_grid_float_row(ui, "Max Y", "##selection_max_y", &mut edited.max.y);

                let mut width = edited.width();
                if property_grid_float_row(ui, "Width", "##selection_width", &mut width) {
                    edited.max.x = edited.min.x + width.max(MIN_SELECTION_SIZE);
                    changed = true;
                }

                let mut height = edited.height();
                if property_grid_float_row(ui, "Height", "##selection_height", &mut height) {
                    edited.max.y = edited.min.y + height.max(MIN_SELECTION_SIZE);
                    changed = true;
                }
            }

            if changed {
                let clamped =
                    clamp_selection_rect_to_image(edited, [image_w as f32, image_h as f32]);
                app_state.set_image_selection(Some(clamped));
            }

            ui.dummy([0.0, 8.0]);
            if app_state.image_selection().is_some() {
                let _pad = ui.push_style_var(StyleVar::ItemSpacing([4.0, 4.0]));
                if ui.button("Copy to Clipboard") {
                    app_state.copy_region_to_clipboard(None);
                }

                if ui.button("Clear Selection") {
                    app_state.clear_image_selection_state();
                }
            }
        });

    app_state.set_show_selection_window(open);
}

/// Resolve the thumbnail texture info for an entry, falling back to the empty icon.
fn resolve_thumbnail<'a>(
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
fn fit_scale_in_cell(img_w: u32, img_h: u32, cell: f32) -> (f32, f32) {
    let scale = (cell / img_w as f32).min(cell / img_h as f32);
    (img_w as f32 * scale, img_h as f32 * scale)
}

fn render_library_item_row(
    ui: &Ui,
    app_state: &ViewerState,
    app_resources: &AppResources,
    index: usize,
    entry: &MediaEntry,
) -> bool {
    let current_width = app_state.library_width() - 32.0;
    let thumbnail_size_xy = [LIBRARY_THUMBNAIL_SIZE, LIBRARY_THUMBNAIL_SIZE];
    if app_state.show_thumbnail() {
        let image_view_id = format!("thumbnail_image_view_{index}");
        let mut thumbnail_clicked = false;
        ui.child_window(&image_view_id)
            .size(thumbnail_size_xy)
            .border(false)
            .build(|| {
                let cell = LIBRARY_THUMBNAIL_SIZE;
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
            .selected(app_state.current_index() == Some(index))
            .size([
                (current_width - thumbnail_size_xy[0]) as f32,
                thumbnail_size_xy[1],
            ])
            .build();
        return thumbnail_clicked || text_clicked;
    } else {
        ui.selectable_config(&entry.file_name)
            .selected(app_state.current_index() == Some(index))
            .build()
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
    app_state: &ViewerState,
    app_resources: &AppResources,
    cols: usize,
    pending_scroll_direction: &mut Option<i32>,
) -> Option<usize> {
    let cell = GRID_CELL_SIZE;
    let show_thumbnail = app_state.show_thumbnail();
    // Cell height: thumbnail area + label row, or a thumbnail-sized text box when hidden.
    let label_h = ui.frame_height_with_spacing();
    let cell_h = if show_thumbnail {
        LIBRARY_THUMBNAIL_SIZE + label_h
    } else {
        LIBRARY_THUMBNAIL_SIZE
    };
    let mut clicked: Option<usize> = None;
    let current_index = app_state.current_index();

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

    for (index, entry) in app_state.media_items().iter().enumerate() {
        let col = index % cols;
        if col == 0 {
            ui.table_next_row();
        }
        ui.table_set_column_index(col);

        let is_selected = app_state.current_index() == Some(index);
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
        handle_scroll_to_selected(ui, current_index, index, pending_scroll_direction);
        if ui.is_item_hovered() {
            let text = file_info_text(Some(entry));
            ui.tooltip_text(text);
        }

        // Draw thumbnail image or placeholder on top via draw_list
        if show_thumbnail {
            let (texture_id, uvs, img_w, img_h) = resolve_thumbnail(entry, app_resources);
            let (draw_w, draw_h) = fit_scale_in_cell(img_w, img_h, LIBRARY_THUMBNAIL_SIZE);
            let img_x = cell_origin[0] + ((cell - draw_w) * 0.5).max(0.0);
            let img_y = cell_origin[1] + ((LIBRARY_THUMBNAIL_SIZE - draw_h) * 0.5).max(0.0);
            ui.get_window_draw_list()
                .add_image(texture_id, [img_x, img_y], [img_x + draw_w, img_y + draw_h])
                .uv_min([uvs[0], uvs[1]])
                .uv_max([uvs[2], uvs[3]])
                .build();
        }

        // Draw file name label below the thumbnail (or at top if no thumbnail)
        let label_y_offset = if show_thumbnail { LIBRARY_THUMBNAIL_SIZE } else { 0.0 };
        let label_pos = [cursor_pos[0] + 2.0, cursor_pos[1] + label_y_offset + 2.0];
        ui.set_cursor_pos(label_pos);
        let label_w = cell - 4.0;
        let max_lines = if show_thumbnail {
            1
        } else {
            let line_h = ui.frame_height_with_spacing().max(1.0);
            ((LIBRARY_THUMBNAIL_SIZE - 4.0) / line_h).floor().max(1.0) as usize
        };
        let display_name = wrap_text_to_width_and_lines(ui, &entry.file_name, label_w, max_lines);
        ui.text(&display_name);
    }

    clicked
}

fn calculate_library_items_per_row(ui: &Ui, app_state: &ViewerState) -> usize {
    if !app_state.show_grid_view() {
        return 1;
    }
    // Use the current scroll area width to get real visible column count.
    let available_width = ui.content_region_avail()[0].max(GRID_CELL_SIZE);
    ((available_width / GRID_CELL_SIZE).floor() as usize).max(1)
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

fn property_grid_float_row(ui: &Ui, name: &str, id: &str, value: &mut f32) -> bool {
    ui.table_next_row();
    ui.table_next_column();
    ui.text(name);
    ui.table_next_column();
    ui.set_next_item_width(-1.0);
    ui.input_float(id, value).display_format("%.1f").build()
}

fn clamp_selection_rect_to_image(rect: Rect2D, image_size: [f32; 2]) -> Rect2D {
    let (min_x, max_x) = clamp_selection_axis(rect.min.x, rect.max.x, image_size[0]);
    let (min_y, max_y) = clamp_selection_axis(rect.min.y, rect.max.y, image_size[1]);

    Rect2D::new(Point2D::new(min_x, min_y), Point2D::new(max_x, max_y))
}

fn clamp_selection_axis(mut min: f32, mut max: f32, bound: f32) -> (f32, f32) {
    let axis_bound = bound.max(MIN_SELECTION_SIZE);
    min = min.clamp(0.0, axis_bound);
    max = max.clamp(0.0, axis_bound);
    if min > max {
        std::mem::swap(&mut min, &mut max);
    }
    if max - min < MIN_SELECTION_SIZE {
        max = (min + MIN_SELECTION_SIZE).min(axis_bound);
        min = (max - MIN_SELECTION_SIZE).max(0.0);
    }
    (min, max)
}

fn render_image_content(ui: &imgui::Ui, app_state: &mut ViewerState, is_pending: bool) {
    if let Some(ref texture) = app_state.current_texture() {
        let avail = ui.content_region_avail();
        let fb_scale = ui.io().display_framebuffer_scale[0];
        let width_scale = avail[0] / texture.width as f32;
        let height_scale = avail[1] / texture.height as f32;
        let scale = match app_state.image_view_mode() {
            ImageViewMode::Original => 1.0 / fb_scale,
            ImageViewMode::FitToWindow => width_scale.min(height_scale),
            ImageViewMode::FitToWidth => width_scale,
        }
        .max(0.01);
        let display_size = [texture.width as f32 * scale, texture.height as f32 * scale];
        let cursor = ui.cursor_pos();
        let centered = [
            (avail[0] - display_size[0]).max(0.0) * 0.5,
            (avail[1] - display_size[1]).max(0.0) * 0.5,
        ];
        ui.set_cursor_pos([
            (cursor[0] + centered[0]).floor(),
            (cursor[1] + centered[1]).floor(),
        ]);

        let image_screen_min = ui.cursor_screen_pos();
        render_image_background(ui, app_state, image_screen_min, display_size);

        let uv0 = [0.0, 0.0];
        let uv1 = [1.0, 1.0];
        imgui::Image::new(texture.id, display_size)
            .uv0(uv0)
            .uv1(uv1)
            .build(ui);

        let view_panel_min = ui.window_pos();
        let view_panel_max = [
            view_panel_min[0] + ui.window_size()[0],
            view_panel_min[1] + ui.window_size()[1],
        ];

        render_image_selection_widget(
            ui,
            app_state,
            is_pending,
            view_panel_min,
            view_panel_max,
            ui.item_rect_min(),
            display_size,
            [texture.width as f32, texture.height as f32],
        );
    } else if app_state.current_directory().is_some() {
        ui.text("No image selected or decode failed.");
    } else {
        ui.text("Welcome to Just Image Viewer");
        ui.text("Open an image directory to begin.");
    }
}
