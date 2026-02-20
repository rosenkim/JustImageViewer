use crate::app::{ImageSelectionRect, ViewerState};
use imgui::{ImColor32, Key, MouseButton, Ui};

const SELECTION_POPUP_ID: &str = "image_selection_popup";
const SELECTION_DASH_LENGTH: f32 = 8.0;
const SELECTION_GAP_LENGTH: f32 = 6.0;
const SELECTION_LINE_THICKNESS: f32 = 1.5;
const MIN_SELECTION_SIZE: f32 = 1.0;

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

    if ui.is_mouse_hovering_rect(view_panel_min, view_panel_max)
        && ui.is_mouse_clicked(MouseButton::Left)
    {
        let start = screen_to_image(
            ui.io().mouse_pos,
            image_screen_min,
            image_display_size,
            image_pixel_size,
        );
        app_state.begin_image_selection_drag(start);
    }

    if let Some(saved_selection) = app_state.image_selection() {
        draw_dashed_selection(
            ui,
            view_panel_min,
            view_panel_max,
            image_display_size,
            image_pixel_size,
            image_screen_min,
            saved_selection,
            ImColor32::from_rgba(80, 180, 255, 240),
        );
    }

    if let Some(start) = app_state.image_selection_drag_start() {
        let current = screen_to_image(
            ui.io().mouse_pos,
            image_screen_min,
            image_display_size,
            image_pixel_size,
        );
        let preview = ImageSelectionRect::from_points(start, current);

        if ui.is_mouse_down(MouseButton::Left) {
            draw_dashed_selection(
                ui,
                view_panel_min,
                view_panel_max,
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
                ui.open_popup(SELECTION_POPUP_ID);
            }
            app_state.clear_image_selection_drag();
        }
    }

    // if let Some(_popup) = ui.begin_popup(SELECTION_POPUP_ID) {
    //     if app_state.image_selection().is_none() {
    //         ui.close_current_popup();
    //         return;
    //     }

    //     if ui.menu_item("Copy") {
    //         copy_selected_region(app_state);
    //         ui.close_current_popup();
    //     }
    //     if ui.menu_item("Cut") {
    //         cut_selected_region(app_state);
    //         ui.close_current_popup();
    //     }
    // }

    if let Some(selection) = app_state.image_selection() {
        let popup_screen_pos = image_to_screen(
            selection.max,
            image_screen_min,
            image_display_size,
            image_pixel_size,
        );

        // imgui-rs에서 Popup 위치 지정 빌더를 제공하지 않으므로 FFI를 직접 호출
        unsafe {
            imgui::sys::igSetNextWindowPos(
                imgui::sys::ImVec2 { x: popup_screen_pos[0] + 5.0, y: popup_screen_pos[1] + 5.0 },
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
    view_panel_min: [f32; 2],
    view_panel_max: [f32; 2],
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
