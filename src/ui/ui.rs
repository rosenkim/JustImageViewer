use crate::app::{ImageViewMode, LibrarySortField, SortDirection, ViewerState, format_file_size};
use crate::render::texture_manager::UploadedTexture;
use imgui::{Condition, MouseCursor, StyleVar};

use super::helper::render_image_selection_widget;
use super::keyboard_shortcuts_window::render_keyboard_shortcuts_window;

const SPLITTER_WIDTH: f32 = 6.0;
const MIN_LIBRARY_WIDTH: f32 = 220.0;
const MIN_VIEWER_WIDTH: f32 = 280.0;
const LIBRARY_SORT_FIELDS: [&str; 3] = ["Name", "Date", "Size"];
const LIBRARY_SORT_DIRECTIONS: [&str; 2] = ["Ascending", "Descending"];

pub fn render_ui(
    ui: &imgui::Ui,
    app_state: &mut ViewerState,
    current_texture: Option<UploadedTexture>,
    running: &mut bool,
) {
    render_main_menu_bar(ui, app_state, running);

    let display = ui.io().display_size;
    let menu_height = ui.io().font_global_scale * 20.0;
    let status_height = 28.0;
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
                            if let Some(texture) = current_texture {
                                let view_panel_min = ui.cursor_screen_pos();
                                let view_panel_size = ui.content_region_avail();
                                let view_panel_max = [
                                    view_panel_min[0] + view_panel_size[0],
                                    view_panel_min[1] + view_panel_size[1],
                                ];
                                let avail = ui.content_region_avail();
                                let width_scale = avail[0] / texture.width as f32;
                                let height_scale = avail[1] / texture.height as f32;
                                let scale = match app_state.image_view_mode() {
                                    ImageViewMode::Original => 1.0,
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
                                    cursor[0] + centered[0],
                                    cursor[1] + centered[1],
                                ]);
                                imgui::Image::new(texture.id, display_size).build(ui);
                                render_image_selection_widget(
                                    ui,
                                    app_state,
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
            ui.text("|");
            ui.same_line();
            ui.text(format!("Config: {}", app_state.config_path().display()));
            ui.same_line();
            ui.text("|");
            ui.same_line();
            ui.text(format!(
                "Restore last directory: {}",
                if app_state.restore_last_directory() {
                    "on"
                } else {
                    "off"
                }
            ));
        });

    if app_state.show_keyboard_shortcuts() {
        let mut open = true;
        render_keyboard_shortcuts_window(ui, &mut open);
        app_state.set_show_keyboard_shortcuts(open);
    }

    if let Some(index) = clicked_index {
        app_state.select_index(index);
    }
}

fn render_main_menu_bar(
    ui: &imgui::Ui,
    app_state: &mut ViewerState,
    running: &mut bool,
) {
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
}
