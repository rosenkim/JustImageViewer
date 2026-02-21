use crate::app::{
    ImageSelectionDragMode, ImageSelectionRect, ImageSelectionResizeHandle, ViewerState,
};
use imgui::{ImColor32, Key, MouseButton, MouseCursor, Ui};

const SELECTION_POPUP_ID: &str = "image_selection_popup";
const SELECTION_DASH_LENGTH: f32 = 8.0;
const SELECTION_GAP_LENGTH: f32 = 6.0;
const SELECTION_LINE_THICKNESS: f32 = 1.5;
const MIN_SELECTION_SIZE: f32 = 1.0;
const SELECTION_RESIZE_HIT_PADDING: f32 = 8.0;

pub fn render_image_selection_widget(
    ui: &Ui,
    app_state: &mut ViewerState,
    view_panel_min: [f32; 2],
    view_panel_max: [f32; 2],
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) {
    if ui.is_key_pressed(Key::Escape) {
        app_state.clear_image_selection_state();
        return;
    }

    if image_display_size[0] <= 0.0
        || image_display_size[1] <= 0.0
        || image_pixel_size[0] <= 0.0
        || image_pixel_size[1] <= 0.0
    {
        app_state.clear_image_selection_drag();
        return;
    }

    let is_hovering_view_panel = ui.is_mouse_hovering_rect(view_panel_min, view_panel_max);
    let mouse_pos = ui.io().mouse_pos;

    let active_resize_handle = app_state
        .image_selection_drag_mode()
        .and_then(|mode| match mode {
            ImageSelectionDragMode::Resize { handle, .. } => Some(handle),
            ImageSelectionDragMode::Create => None,
        });
    let hovered_resize_handle = if is_hovering_view_panel {
        app_state.image_selection().and_then(|selection| {
            resolve_resize_handle(
                mouse_pos,
                selection,
                image_screen_min,
                image_display_size,
                image_pixel_size,
            )
        })
    } else {
        None
    };
    let cursor_handle = active_resize_handle.or(hovered_resize_handle);
    if let Some(handle) = cursor_handle {
        ui.set_mouse_cursor(Some(cursor_for_resize_handle(handle)));
    }

    if is_hovering_view_panel && ui.is_mouse_clicked(MouseButton::Left) {
        let mouse_image = screen_to_image(
            mouse_pos,
            image_screen_min,
            image_display_size,
            image_pixel_size,
        );

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

    if let (Some(start), Some(drag_mode)) = (
        app_state.image_selection_drag_start(),
        app_state.image_selection_drag_mode(),
    ) {
        let current = screen_to_image(
            ui.io().mouse_pos,
            image_screen_min,
            image_display_size,
            image_pixel_size,
        );
        let preview = match drag_mode {
            ImageSelectionDragMode::Create => ImageSelectionRect::from_points(start, current),
            ImageSelectionDragMode::Resize { handle, original } => {
                resize_selection(original, handle, start, current, image_pixel_size)
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
                    popup_screen_pos_override = Some(image_to_screen(
                        preview.max,
                        image_screen_min,
                        image_display_size,
                        image_pixel_size,
                    ));
                }
            }
            app_state.clear_image_selection_drag();
        }
    }

    if is_hovering_view_panel
        && ui.is_mouse_clicked(MouseButton::Right)
        && app_state.image_selection().is_some()
        && app_state.image_selection_drag_start().is_none()
    {
        ui.open_popup(SELECTION_POPUP_ID);
        popup_screen_pos_override = Some(mouse_pos);
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

    if let Some(_popup) = ui.begin_popup(SELECTION_POPUP_ID) {
        if app_state.image_selection().is_none() {
            ui.close_current_popup();
            return;
        }

        if ui.menu_item("Copy") {
            copy_selected_region(app_state);
            ui.close_current_popup();
        }
        if ui.menu_item("Cut") {
            cut_selected_region(app_state);
            ui.close_current_popup();
        }
    }
}

fn draw_dashed_selection(
    ui: &Ui,
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
    image_screen_min: [f32; 2],
    selection: ImageSelectionRect,
    color: ImColor32,
) {
    let draw_list = ui.get_window_draw_list();
    let selection_screen_min = image_to_screen(
        selection.min,
        image_screen_min,
        image_display_size,
        image_pixel_size,
    );
    let selection_screen_max = image_to_screen(
        selection.max,
        image_screen_min,
        image_display_size,
        image_pixel_size,
    );

    draw_dashed_rect(
        &draw_list,
        selection_screen_min,
        selection_screen_max,
        color,
    );
}

fn resolve_resize_handle(
    mouse_screen: [f32; 2],
    selection: ImageSelectionRect,
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) -> Option<ImageSelectionResizeHandle> {
    let selection_screen_min = image_to_screen(
        selection.min,
        image_screen_min,
        image_display_size,
        image_pixel_size,
    );
    let selection_screen_max = image_to_screen(
        selection.max,
        image_screen_min,
        image_display_size,
        image_pixel_size,
    );
    let padding = SELECTION_RESIZE_HIT_PADDING;
    let in_expanded_bounds = mouse_screen[0] >= selection_screen_min[0] - padding
        && mouse_screen[0] <= selection_screen_max[0] + padding
        && mouse_screen[1] >= selection_screen_min[1] - padding
        && mouse_screen[1] <= selection_screen_max[1] + padding;
    if !in_expanded_bounds {
        return None;
    }

    let near_left = (mouse_screen[0] - selection_screen_min[0]).abs() <= padding;
    let near_right = (mouse_screen[0] - selection_screen_max[0]).abs() <= padding;
    let near_top = (mouse_screen[1] - selection_screen_min[1]).abs() <= padding;
    let near_bottom = (mouse_screen[1] - selection_screen_max[1]).abs() <= padding;

    if near_left && near_top {
        return Some(ImageSelectionResizeHandle::TopLeft);
    }
    if near_right && near_top {
        return Some(ImageSelectionResizeHandle::TopRight);
    }
    if near_left && near_bottom {
        return Some(ImageSelectionResizeHandle::BottomLeft);
    }
    if near_right && near_bottom {
        return Some(ImageSelectionResizeHandle::BottomRight);
    }
    if near_left {
        return Some(ImageSelectionResizeHandle::Left);
    }
    if near_right {
        return Some(ImageSelectionResizeHandle::Right);
    }
    if near_top {
        return Some(ImageSelectionResizeHandle::Top);
    }
    if near_bottom {
        return Some(ImageSelectionResizeHandle::Bottom);
    }

    let inside_selection = mouse_screen[0] >= selection_screen_min[0]
        && mouse_screen[0] <= selection_screen_max[0]
        && mouse_screen[1] >= selection_screen_min[1]
        && mouse_screen[1] <= selection_screen_max[1];
    if !inside_selection {
        return None;
    }

    let center = [
        (selection_screen_min[0] + selection_screen_max[0]) * 0.5,
        (selection_screen_min[1] + selection_screen_max[1]) * 0.5,
    ];
    let is_left = mouse_screen[0] <= center[0];
    let is_top = mouse_screen[1] <= center[1];

    Some(match (is_left, is_top) {
        (true, true) => ImageSelectionResizeHandle::TopLeft,
        (false, true) => ImageSelectionResizeHandle::TopRight,
        (true, false) => ImageSelectionResizeHandle::BottomLeft,
        (false, false) => ImageSelectionResizeHandle::BottomRight,
    })
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

fn resize_selection(
    original: ImageSelectionRect,
    handle: ImageSelectionResizeHandle,
    drag_start: [f32; 2],
    drag_current: [f32; 2],
    image_pixel_size: [f32; 2],
) -> ImageSelectionRect {
    let delta_x = drag_current[0] - drag_start[0];
    let delta_y = drag_current[1] - drag_start[1];
    let mut min = original.min;
    let mut max = original.max;

    match handle {
        ImageSelectionResizeHandle::Left => {
            min[0] = (original.min[0] + delta_x).clamp(0.0, original.max[0] - MIN_SELECTION_SIZE);
        }
        ImageSelectionResizeHandle::Right => {
            max[0] = (original.max[0] + delta_x)
                .clamp(original.min[0] + MIN_SELECTION_SIZE, image_pixel_size[0]);
        }
        ImageSelectionResizeHandle::Top => {
            min[1] = (original.min[1] + delta_y).clamp(0.0, original.max[1] - MIN_SELECTION_SIZE);
        }
        ImageSelectionResizeHandle::Bottom => {
            max[1] = (original.max[1] + delta_y)
                .clamp(original.min[1] + MIN_SELECTION_SIZE, image_pixel_size[1]);
        }
        ImageSelectionResizeHandle::TopLeft => {
            min[0] = (original.min[0] + delta_x).clamp(0.0, original.max[0] - MIN_SELECTION_SIZE);
            min[1] = (original.min[1] + delta_y).clamp(0.0, original.max[1] - MIN_SELECTION_SIZE);
        }
        ImageSelectionResizeHandle::TopRight => {
            max[0] = (original.max[0] + delta_x)
                .clamp(original.min[0] + MIN_SELECTION_SIZE, image_pixel_size[0]);
            min[1] = (original.min[1] + delta_y).clamp(0.0, original.max[1] - MIN_SELECTION_SIZE);
        }
        ImageSelectionResizeHandle::BottomLeft => {
            min[0] = (original.min[0] + delta_x).clamp(0.0, original.max[0] - MIN_SELECTION_SIZE);
            max[1] = (original.max[1] + delta_y)
                .clamp(original.min[1] + MIN_SELECTION_SIZE, image_pixel_size[1]);
        }
        ImageSelectionResizeHandle::BottomRight => {
            max[0] = (original.max[0] + delta_x)
                .clamp(original.min[0] + MIN_SELECTION_SIZE, image_pixel_size[0]);
            max[1] = (original.max[1] + delta_y)
                .clamp(original.min[1] + MIN_SELECTION_SIZE, image_pixel_size[1]);
        }
    }

    ImageSelectionRect { min, max }
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
    screen_pos: [f32; 2],
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) -> [f32; 2] {
    let normalized_x =
        ((screen_pos[0] - image_screen_min[0]) / image_display_size[0]).clamp(0.0, 1.0);
    let normalized_y =
        ((screen_pos[1] - image_screen_min[1]) / image_display_size[1]).clamp(0.0, 1.0);
    [
        normalized_x * image_pixel_size[0],
        normalized_y * image_pixel_size[1],
    ]
}

fn image_to_screen(
    image_pos: [f32; 2],
    image_screen_min: [f32; 2],
    image_display_size: [f32; 2],
    image_pixel_size: [f32; 2],
) -> [f32; 2] {
    [
        image_screen_min[0] + (image_pos[0] / image_pixel_size[0]) * image_display_size[0],
        image_screen_min[1] + (image_pos[1] / image_pixel_size[1]) * image_display_size[1],
    ]
}

fn copy_selected_region(_app_state: &mut ViewerState) {
    // TODO: implement selected region copy.
}

fn cut_selected_region(_app_state: &mut ViewerState) {
    // TODO: implement selected region cut.
}
