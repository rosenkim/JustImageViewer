use crate::app::{ImageSelectionDragMode, ImageSelectionResizeHandle, ViewerState};
use crate::math::{Point2D, Rect2D};
use imgui::{ImColor32, Key, MouseButton, MouseCursor, Ui};

const SELECTION_POPUP_ID: &str = "image_selection_popup";
const SELECTION_DASH_LENGTH: f32 = 8.0;
const SELECTION_GAP_LENGTH: f32 = 6.0;
const SELECTION_LINE_THICKNESS: f32 = 1.5;
const MIN_SELECTION_SIZE: f32 = 1.0;
const SELECTION_RESIZE_HIT_PADDING: f32 = 8.0;

struct SelectionPopupItem {
    name: &'static str,
}

const SELECTION_POPUP_ITEMS: &[SelectionPopupItem] = &[
    SelectionPopupItem { name: "Copy" },
];

pub fn render_image_selection_widget(
    ui: &Ui,
    app_state: &mut ViewerState,
    is_pending: bool,
    view_panel_min: [f32; 2],
    view_panel_max: [f32; 2],
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) {
    // Escape key clears any selection/drag state
    if ui.is_key_pressed(Key::Escape) {
        app_state.clear_image_selection_state();
        return;
    }

    // If image metrics are invalid, bail out early
    if image_display_size[0] <= 0.0
        || image_display_size[1] <= 0.0
        || image_pixel_size[0] <= 0.0
        || image_pixel_size[1] <= 0.0
    {
        app_state.clear_image_selection_drag();
        return;
    }

    let is_hovering_view_panel = ui.is_mouse_hovering_rect(view_panel_min, view_panel_max);
    let mouse_pos: Point2D = Point2D::from_array(ui.io().mouse_pos);

    let active_drag_mode = app_state.image_selection_drag_mode();
    if let Some(mode) = active_drag_mode {
        match mode {
            ImageSelectionDragMode::Resize { handle, .. } => {
                ui.set_mouse_cursor(Some(cursor_for_resize_handle(handle)));
            }
            ImageSelectionDragMode::Move { .. } => {
                ui.set_mouse_cursor(Some(MouseCursor::ResizeAll));
            }
            ImageSelectionDragMode::Create => {}
        }
    } else if is_hovering_view_panel {
        if let Some(selection) = app_state.image_selection() {
            if let Some(handle) = resolve_resize_handle(
                mouse_pos,
                selection,
                image_screen_min,
                image_display_size,
                image_pixel_size,
            ) {
                ui.set_mouse_cursor(Some(cursor_for_resize_handle(handle)));
            } else {
                let mouse_image = screen_to_image(
                    mouse_pos,
                    image_screen_min,
                    image_display_size,
                    image_pixel_size,
                );
                if selection.contain_point(mouse_image) {
                    ui.set_mouse_cursor(Some(MouseCursor::ResizeAll));
                }
            }
        }
    }

    if is_hovering_view_panel && ui.is_mouse_clicked(MouseButton::Left) {
        let mouse_image = screen_to_image(
            mouse_pos,
            image_screen_min,
            image_display_size,
            image_pixel_size,
        )
        .to_array();

        // Decide whether to resize, move, or create a selection.
        let drag_mode = app_state
            .image_selection()
            .and_then(|selection| {
                resolve_resize_handle(
                    mouse_pos,
                    selection,
                    image_screen_min,
                    image_display_size,
                    image_pixel_size,
                )
                .map(|handle| ImageSelectionDragMode::Resize {
                    handle,
                    original: selection,
                })
                .or_else(|| {
                    let mouse_image = Point2D::from_array(mouse_image);
                    if selection.contain_point(mouse_image) {
                        Some(ImageSelectionDragMode::Move { original: selection })
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(ImageSelectionDragMode::Create);

        app_state.begin_image_selection_drag(mouse_image, drag_mode);
    }

    if let Some(saved_selection) = app_state.image_selection() {
        draw_dashed_selection(
            ui,
            image_display_size,
            image_pixel_size,
            image_screen_min,
            saved_selection,
            ImColor32::from_rgba(80, 180, 255, 240),
        );
    }

    let mut popup_screen_pos_override: Option<[f32; 2]> = None;

    // Handle drag preview and finalize selection
    if let (Some(start), Some(drag_mode)) = (
        app_state.image_selection_drag_start(),
        app_state.image_selection_drag_mode(),
    ) {
        // Get current mouse position in image space
        let current = screen_to_image(
            Point2D::from_array(ui.io().mouse_pos),
            image_screen_min,
            image_display_size,
            image_pixel_size,
        );

        // Live preview rect based on drag mode
        let preview = match drag_mode {
            ImageSelectionDragMode::Create => {
                // Create a new selection from start to current mouse position
                Rect2D::from_points(Point2D::from_array(start), current)
            }
            ImageSelectionDragMode::Move { original } => move_selection(
                original,
                Point2D::from_array(start),
                current,
                image_pixel_size,
            ),
            ImageSelectionDragMode::Resize { handle, original } => {
                // Resize existing selection based on handle and mouse movement
                resize_selection(
                    original,
                    handle,
                    Point2D::from_array(start),
                    current,
                    image_pixel_size,
                )
            }
        };

        if ui.is_mouse_down(MouseButton::Left) {
            draw_dashed_selection(
                ui,
                image_display_size,
                image_pixel_size,
                image_screen_min,
                preview,
                ImColor32::from_rgba(255, 210, 90, 255),
            );
        }

        if ui.is_mouse_released(MouseButton::Left) {
            if preview.width() >= MIN_SELECTION_SIZE && preview.height() >= MIN_SELECTION_SIZE {
                app_state.set_image_selection(Some(preview));
                if matches!(drag_mode, ImageSelectionDragMode::Create) {
                    ui.open_popup(SELECTION_POPUP_ID);
                    popup_screen_pos_override = Some(
                        image_to_screen(
                            preview.max,
                            image_screen_min,
                            image_display_size,
                            image_pixel_size,
                        )
                        .to_array(),
                    );
                }
            }
            app_state.clear_image_selection_drag();
            if let Some(selection) = app_state.image_selection() {
                app_state.set_image_selection(Some(floor_selection(selection)));
            }
        }
    }

    if is_hovering_view_panel
        && ui.is_mouse_clicked(MouseButton::Right)
        && app_state.image_selection_drag_start().is_none()
    {
        ui.open_popup(SELECTION_POPUP_ID);
        popup_screen_pos_override = Some(mouse_pos.to_array());
    }

    if let Some(popup_screen_pos) = popup_screen_pos_override {
        // imgui-rs doesn't provide a builder for setting popup position, so we use FFI directly
        unsafe {
            imgui::sys::igSetNextWindowPos(
                imgui::sys::ImVec2 {
                    x: popup_screen_pos[0],
                    y: popup_screen_pos[1],
                },
                imgui::sys::ImGuiCond_Always as i32,
                imgui::sys::ImVec2 { x: 0.0, y: 0.0 },
            );
        }
    }

    render_selection_popup(ui, app_state, is_pending);
}

fn draw_dashed_selection(
    ui: &Ui,
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
    image_screen_min: [f32; 2],
    selection: Rect2D,
    color: ImColor32,
) {
    // Convert selection bounds to screen space and draw dashed outline
    let draw_list = ui.get_window_draw_list();
    let selection_screen_min = image_to_screen(
        selection.min,
        image_screen_min,
        image_display_size,
        image_pixel_size,
    )
    .to_array();
    let selection_screen_max = image_to_screen(
        selection.max,
        image_screen_min,
        image_display_size,
        image_pixel_size,
    )
    .to_array();

    draw_dashed_rect(
        &draw_list,
        selection_screen_min,
        selection_screen_max,
        color,
    );
}

fn resolve_resize_handle(
    mouse_screen: Point2D,
    selection: Rect2D,
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) -> Option<ImageSelectionResizeHandle> {
    // Hit-test entirely in screen space to avoid scale-dependent issues
    let sel_screen_min = image_to_screen(selection.min, image_screen_min, image_display_size, image_pixel_size);
    let sel_screen_max = image_to_screen(selection.max, image_screen_min, image_display_size, image_pixel_size);
    let padding = SELECTION_RESIZE_HIT_PADDING;

    let near_left = (mouse_screen.x - sel_screen_min.x).abs() <= padding;
    let near_right = (mouse_screen.x - sel_screen_max.x).abs() <= padding;
    let near_top = (mouse_screen.y - sel_screen_min.y).abs() <= padding;
    let near_bottom = (mouse_screen.y - sel_screen_max.y).abs() <= padding;

    if near_left && near_top {
        return Some(ImageSelectionResizeHandle::TopLeft);
    } else if near_right && near_top {
        return Some(ImageSelectionResizeHandle::TopRight);
    } else if near_left && near_bottom {
        return Some(ImageSelectionResizeHandle::BottomLeft);
    } else if near_right && near_bottom {
        return Some(ImageSelectionResizeHandle::BottomRight);
    } else if near_left {
        return Some(ImageSelectionResizeHandle::Left);
    } else if near_right {
        return Some(ImageSelectionResizeHandle::Right);
    } else if near_top {
        return Some(ImageSelectionResizeHandle::Top);
    } else if near_bottom {
        return Some(ImageSelectionResizeHandle::Bottom);
    }
    None
}

fn cursor_for_resize_handle(handle: ImageSelectionResizeHandle) -> MouseCursor {
    match handle {
        ImageSelectionResizeHandle::Left | ImageSelectionResizeHandle::Right => {
            MouseCursor::ResizeEW
        }
        ImageSelectionResizeHandle::Top | ImageSelectionResizeHandle::Bottom => {
            MouseCursor::ResizeNS
        }
        ImageSelectionResizeHandle::TopLeft | ImageSelectionResizeHandle::BottomRight => {
            MouseCursor::ResizeNWSE
        }
        ImageSelectionResizeHandle::TopRight | ImageSelectionResizeHandle::BottomLeft => {
            MouseCursor::ResizeNESW
        }
    }
}

fn floor_selection(selection: Rect2D) -> Rect2D {
    Rect2D {
        min: Point2D {
            x: selection.min.x.floor(),
            y: selection.min.y.floor(),
        },
        max: Point2D {
            x: selection.max.x.floor(),
            y: selection.max.y.floor(),
        },
    }
}

fn resize_selection(
    selection: Rect2D,
    handle: ImageSelectionResizeHandle,
    drag_start: Point2D,
    drag_current: Point2D,
    image_pixel_size: [f32; 2],
) -> Rect2D {
    // Move the grabbed edge/corner while clamping to image bounds
    let delta_x = drag_current.x - drag_start.x;
    let delta_y = drag_current.y - drag_start.y;
    let mut min = selection.min;
    let mut max = selection.max;

    match handle {
        ImageSelectionResizeHandle::Left => {
            min.x = selection.min.x + delta_x;
        }
        ImageSelectionResizeHandle::Right => {
            max.x = selection.max.x + delta_x;
        }
        ImageSelectionResizeHandle::Top => {
            min.y = selection.min.y + delta_y;
        }
        ImageSelectionResizeHandle::Bottom => {
            max.y = selection.max.y + delta_y;
        }
        ImageSelectionResizeHandle::TopLeft => {
            min.x = selection.min.x + delta_x;
            min.y = selection.min.y + delta_y;
        }
        ImageSelectionResizeHandle::TopRight => {
            max.x = selection.max.x + delta_x;
            min.y = selection.min.y + delta_y;
        }
        ImageSelectionResizeHandle::BottomLeft => {
            min.x = selection.min.x + delta_x;
            max.y = selection.max.y + delta_y;
        }
        ImageSelectionResizeHandle::BottomRight => {
            max.x = selection.max.x + delta_x;
            max.y = selection.max.y + delta_y;
        }
    }

    min.x = min.x.clamp(0.0, image_pixel_size[0]);
    min.y = min.y.clamp(0.0, image_pixel_size[1]);
    max.x = max.x.clamp(0.0, image_pixel_size[0]);
    max.y = max.y.clamp(0.0, image_pixel_size[1]);

    Rect2D { min, max }
}

fn move_selection(
    selection: Rect2D,
    drag_start: Point2D,
    drag_current: Point2D,
    image_pixel_size: [f32; 2],
) -> Rect2D {
    let width = selection.width();
    let height = selection.height();

    let delta = drag_current.sub(drag_start);
    let max_min_x = (image_pixel_size[0] - width).max(0.0);
    let max_min_y = (image_pixel_size[1] - height).max(0.0);
    let new_min_x = (selection.min.x + delta.x).clamp(0.0, max_min_x);
    let new_min_y = (selection.min.y + delta.y).clamp(0.0, max_min_y);

    Rect2D::from_point_size(new_min_x, new_min_y, width, height)
}

fn draw_dashed_rect(
    draw_list: &imgui::DrawListMut<'_>,
    min: [f32; 2],
    max: [f32; 2],
    color: ImColor32,
) {
    if max[0] - min[0] <= 0.0 || max[1] - min[1] <= 0.0 {
        return;
    }

    draw_dashed_line(draw_list, [min[0], min[1]], [max[0], min[1]], color);
    draw_dashed_line(draw_list, [max[0], min[1]], [max[0], max[1]], color);
    draw_dashed_line(draw_list, [max[0], max[1]], [min[0], max[1]], color);
    draw_dashed_line(draw_list, [min[0], max[1]], [min[0], min[1]], color);
}

fn draw_dashed_line(
    draw_list: &imgui::DrawListMut<'_>,
    start: [f32; 2],
    end: [f32; 2],
    color: ImColor32,
) {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length = (dx * dx + dy * dy).sqrt();

    if length <= f32::EPSILON {
        return;
    }

    let unit = [dx / length, dy / length];
    let mut offset = 0.0;
    while offset < length {
        let dash_end = (offset + SELECTION_DASH_LENGTH).min(length);
        let p1 = [start[0] + unit[0] * offset, start[1] + unit[1] * offset];
        let p2 = [start[0] + unit[0] * dash_end, start[1] + unit[1] * dash_end];
        draw_list
            .add_line(p1, p2, color)
            .thickness(SELECTION_LINE_THICKNESS)
            .build();
        offset += SELECTION_DASH_LENGTH + SELECTION_GAP_LENGTH;
    }
}

fn screen_to_image(
    screen_pos: Point2D,
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) -> Point2D {
    let normalized_x =
        ((screen_pos.x - image_screen_min[0]) / image_display_size[0]).clamp(0.0, 1.0);
    let normalized_y =
        ((screen_pos.y - image_screen_min[1]) / image_display_size[1]).clamp(0.0, 1.0);
    Point2D {
        x: normalized_x * image_pixel_size[0],
        y: normalized_y * image_pixel_size[1],
    }
}

fn image_to_screen(
    image_pos: Point2D,
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) -> Point2D {
    Point2D {
        x: image_screen_min[0] + (image_pos.x / image_pixel_size[0]) * image_display_size[0],
        y: image_screen_min[1] + (image_pos.y / image_pixel_size[1]) * image_display_size[1],
    }
}

fn render_selection_popup(
    ui: &Ui,
    app_state: &mut ViewerState,
    is_pending: bool,
) {
    if let Some(_popup) = ui.begin_popup(SELECTION_POPUP_ID) {
        for item in SELECTION_POPUP_ITEMS {
            // Copy is enabled only when image is ready (not pending) and texture exists.
            let enabled = !is_pending && app_state.current_texture().is_some();
            if ui
                .menu_item_config(item.name)
                .enabled(enabled)
                .build()
            {
                if item.name == "Copy" {
                    app_state.copy_region_to_clipboard(None);
                }
                ui.close_current_popup();
                break;
            }
        }
    }
}