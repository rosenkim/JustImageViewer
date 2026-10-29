use crate::app::ViewerState;
use crate::infra::config::BackgroundMode;
use imgui::{ImColor32, Ui};

use super::layout_constants::CHECKER_TILE_SIZE;

pub(super) fn render_view_background(
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
