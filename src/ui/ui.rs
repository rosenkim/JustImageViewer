use crate::app::{ImageViewMode, LibrarySortField, SortDirection, ViewerState, format_file_size};
use crate::core::media::MediaEntry;
use crate::math::{Point2D, Rect2D};
use crate::render::app_resources::AppResources;
use chrono::{DateTime, Local, Utc};
use imgui::{Condition, MouseCursor, StyleVar, TableFlags, Ui};
use std::time::Duration;

use super::helper::render_image_selection_widget;
use super::keyboard_shortcuts_window::render_keyboard_shortcuts_window;

const SPLITTER_WIDTH: f32 = 6.0;
const MIN_LIBRARY_WIDTH: f32 = 220.0;
const MIN_VIEWER_WIDTH: f32 = 280.0;
const LIBRARY_SORT_FIELDS: [&str; 3] = ["Name", "Date", "Size"];
const LIBRARY_SORT_DIRECTIONS: [&str; 2] = ["Ascending", "Descending"];
const LIBRARY_THUMBNAIL_SIZE: f32 = 96.0;
const GRID_CELL_SIZE: f32 = LIBRARY_THUMBNAIL_SIZE + 16.0;
const MIN_SELECTION_SIZE: f32 = 1.0;

pub fn render_ui(
    ui: &imgui::Ui,
    app_state: &mut ViewerState,
    is_pending: bool,
    app_resources: &AppResources,
    running: &mut bool,
) {
    render_main_menu_bar(ui, app_state, running);

    let display = ui.io().display_size;
    // Use current frame metrics so layout follows the real font size.
    let menu_height = ui.frame_height_with_spacing();
    let status_height = ui.frame_height_with_spacing() + 6.0;
    let content_height = (display[1] - menu_height - status_height).max(120.0);
    let window_flags = imgui::WindowFlags::NO_MOVE
        | imgui::WindowFlags::NO_RESIZE
        | imgui::WindowFlags::NO_COLLAPSE
        | imgui::WindowFlags::NO_TITLE_BAR
        | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;

    let mut clicked_index: Option<usize> = None;

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
                        }
                        ui.same_line();
                        let mut show_grid_view = app_state.show_grid_view();
                        if ui.checkbox("Grid", &mut show_grid_view) {
                            app_state.set_show_grid_view(show_grid_view);
                        }
                        ui.separator();
                        ui.child_window("library_scroll")
                            .size([0.0, -36.0])
                            .build(|| {
                                let mut pending_scroll_direction =
                                    app_state.take_pending_library_scroll_to_selection();
                                if app_state.show_grid_view() {
                                    if let Some(idx) =
                                        render_library_grid(ui, app_state, app_resources)
                                    {
                                        clicked_index = Some(idx);
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

                                        if app_state.current_index() == Some(index)
                                            && let Some(direction) = pending_scroll_direction
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
                                            pending_scroll_direction = None;
                                        }
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
                    let _pad = ui.push_style_var(StyleVar::ItemSpacing([4.0, 4.0]));
                    let metadata_height = if app_state.show_info() { 86.0 } else { 0.0 };
                    ui.child_window("image_region")
                        .size([0.0, -metadata_height])
                        .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
                        .build(|| {
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
                                let display_size =
                                    [texture.width as f32 * scale, texture.height as f32 * scale];
                                let cursor = ui.cursor_pos();
                                let centered = [
                                    (avail[0] - display_size[0]).max(0.0) * 0.5,
                                    (avail[1] - display_size[1]).max(0.0) * 0.5,
                                ];
                                ui.set_cursor_pos([
                                    (cursor[0] + centered[0]).floor(),
                                    (cursor[1] + centered[1]).floor(),
                                ]);

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
                                ui.text("Welcome to Vibe Image Viewer");
                                ui.text("Open an image directory to begin.");
                            }
                        });

                    if app_state.show_info() {
                        render_file_info(ui, app_state);
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

pub fn render_file_info(ui: &imgui::Ui, app_state: &ViewerState) {
    ui.separator();
    if let Some(entry) = app_state.current_entry() {
        ui.text(format!("File: {}", entry.file_name));
        ui.text(format!("Format: {}", entry.format.as_str()));
        ui.text(format!("Size: {}", format_file_size(entry.file_size)));
        if let Some((w, h)) = app_state.current_image_size() {
            ui.text(format!("Resolution: {} x {}", w, h));
        }
    } else {
        ui.text("No file selected");
    }
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
        ui.child_window(&image_view_id)
            .size(thumbnail_size_xy)
            .border(false)
            .build(|| {
                let cell = LIBRARY_THUMBNAIL_SIZE;
                let (texture_id, uvs, img_w, img_h) = if let Some(thumbnail) = &entry.thumbnail {
                    let (w, h) = thumbnail.image_size;
                    (thumbnail.texture_index, thumbnail.uvs, w, h)
                } else {
                    let region = &app_resources.empty_icon_region;
                    let (w, h) = region.image_size;
                    (region.texture_id, region.uvs, w, h)
                };
                let scale = (cell / img_w as f32).min(cell / img_h as f32);
                let draw_w = img_w as f32 * scale;
                let draw_h = img_h as f32 * scale;
                let cursor = ui.cursor_pos();
                ui.set_cursor_pos([
                    cursor[0] + (cell - draw_w) * 0.5,
                    cursor[1] + (cell - draw_h) * 0.5,
                ]);
                imgui::Image::new(texture_id, [draw_w, draw_h])
                    .uv0([uvs[0], uvs[1]])
                    .uv1([uvs[2], uvs[3]])
                    .build(ui);
            });
        ui.same_line();

        let dimensions_text = match entry.dimensions {
            Some((width, height)) => format!("- {width} x {height}"),
            None => "- Unknown x Unknown".to_owned(),
        };
        let modified_date_text = format_modified_date("- ", entry.modified_time);
        // Show file name + resolution + modified date in one selectable row.
        let selectable_label = format!(
            "{}\n{}\n{}##library_item_{index}",
            entry.file_name, dimensions_text, modified_date_text
        );
        return ui
            .selectable_config(&selectable_label)
            .selected(app_state.current_index() == Some(index))
            .size([
                (current_width - thumbnail_size_xy[0]) as f32,
                thumbnail_size_xy[1],
            ])
            .build();
    } else {
        ui.selectable_config(&entry.file_name)
            .selected(app_state.current_index() == Some(index))
            .build()
    }
}

fn render_library_grid(
    ui: &Ui,
    app_state: &ViewerState,
    app_resources: &AppResources,
) -> Option<usize> {
    let available_width = app_state.library_width() - 16.0;
    let cols = ((available_width / GRID_CELL_SIZE) as usize).max(1);
    let cell = GRID_CELL_SIZE;
    let show_thumbnail = app_state.show_thumbnail();
    // Cell height: thumbnail area + label row, or just label row
    let label_h = ui.frame_height_with_spacing();
    let cell_h = if show_thumbnail {
        LIBRARY_THUMBNAIL_SIZE + label_h
    } else {
        label_h
    };
    let mut clicked: Option<usize> = None;

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

    let items: Vec<(usize, String, Option<crate::core::media::ThumbnailInfo>)> = app_state
        .media_items()
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.file_name.clone(), e.thumbnail.clone()))
        .collect();

    for (index, file_name, thumbnail) in &items {
        let col = index % cols;
        if col == 0 {
            ui.table_next_row();
        }
        ui.table_set_column_index(col);

        let is_selected = app_state.current_index() == Some(*index);
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
            clicked = Some(*index);
        }

        // Draw thumbnail image or placeholder on top via draw_list
        if show_thumbnail {
            let (texture_id, uvs, img_w, img_h) = if let Some(thumb) = thumbnail {
                let (w, h) = thumb.image_size;
                (thumb.texture_index, thumb.uvs, w, h)
            } else {
                let region = &app_resources.empty_icon_region;
                let (w, h) = region.image_size;
                (region.texture_id, region.uvs, w, h)
            };
            let scale =
                (LIBRARY_THUMBNAIL_SIZE / img_w as f32).min(LIBRARY_THUMBNAIL_SIZE / img_h as f32);
            let draw_w = img_w as f32 * scale;
            let draw_h = img_h as f32 * scale;
            let img_x = cell_origin[0] + ((cell - draw_w) * 0.5).max(0.0);
            let img_y = cell_origin[1] + ((LIBRARY_THUMBNAIL_SIZE - draw_h) * 0.5).max(0.0);
            ui.get_window_draw_list()
                .add_image(texture_id, [img_x, img_y], [img_x + draw_w, img_y + draw_h])
                .uv_min([uvs[0], uvs[1]])
                .uv_max([uvs[2], uvs[3]])
                .build();
        }

        // Draw file name label below the thumbnail (or at top if no thumbnail)
        let label_y_offset = if show_thumbnail {
            LIBRARY_THUMBNAIL_SIZE
        } else {
            0.0
        };
        let label_pos = [cursor_pos[0] + 2.0, cursor_pos[1] + label_y_offset];
        ui.set_cursor_pos(label_pos);
        let label_w = cell - 4.0;
        let display_name = truncate_text_to_width(ui, file_name, label_w);
        ui.text(&display_name);
    }

    clicked
}

/// Truncate `text` so that it fits within `max_width` pixels, appending "…" if needed.
fn truncate_text_to_width(ui: &Ui, text: &str, max_width: f32) -> String {
    let full_width = ui.calc_text_size(text)[0];
    if full_width <= max_width {
        return text.to_owned();
    }
    let ellipsis = "...";
    let ellipsis_w = ui.calc_text_size(ellipsis)[0];
    let mut end = text.len();
    while end > 0 {
        // Step back one char boundary at a time
        end -= 1;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = &text[..end];
        if ui.calc_text_size(candidate)[0] + ellipsis_w <= max_width {
            return format!("{candidate}{ellipsis}");
        }
    }
    ellipsis.to_owned()
}

fn property_grid_float_row(ui: &Ui, name: &str, id: &str, value: &mut f32) -> bool {
    ui.table_next_row();
    ui.table_next_column();
    ui.text(name);
    ui.table_next_column();
    ui.set_next_item_width(-1.0);
    ui.input_float(id, value).display_format("%.1f").build()
}

fn format_modified_date(prefix: &str, modified_time: Duration) -> String {
    let Ok(seconds) = i64::try_from(modified_time.as_secs()) else {
        return format!("{}Unknown", prefix);
    };

    // Convert unix seconds to local time text for list UI.
    let Some(utc_time) = DateTime::<Utc>::from_timestamp(seconds, 0) else {
        return format!("{}Unknown", prefix);
    };

    let local_time = utc_time.with_timezone(&Local);
    format!("{}{}", prefix, local_time.format("%Y-%m-%d %H:%M"))
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
