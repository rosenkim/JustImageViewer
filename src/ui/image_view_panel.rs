use crate::app::{ImageViewMode, ViewerState};
use crate::infra::config::BackgroundMode;
use crate::math::{Point2D, Rect2D};
use crate::render::app_resources::AppResources;
use imgui::{Condition, ImColor32, StyleVar, TableFlags, Ui};

use super::image_mouse_handler::ImageMouseHandler;
use super::layout_constants::{CHECKER_TILE_SIZE, MIN_SELECTION_SIZE};
use super::ui::{fit_scale_in_cell, resolve_thumbnail};
use super::view_panel::ViewContentPanel;

// Square cell size for each thumbnail when several selected images are shown.
const MULTI_VIEW_CELL_SIZE: f32 = 160.0;

pub struct ImageViewPanel {
    mouse_handler: ImageMouseHandler,
    selection_draft: Rect2D,
}

impl ImageViewPanel {
    pub fn new() -> Self {
        Self {
            mouse_handler: ImageMouseHandler,
            selection_draft: Rect2D::from_point_size(0.0, 0.0, 1.0, 1.0),
        }
    }
}

impl ViewContentPanel for ImageViewPanel {
    fn render(
        &mut self,
        ui: &Ui,
        app_state: &mut ViewerState,
        app_resources: &AppResources,
        is_pending: bool,
    ) {
        // Selected thumbnails are already in memory, so the grid needs no extra load.
        if app_state.is_multi_select() {
            render_selected_images_grid(ui, app_state, app_resources);
            return;
        }

        if let Some(ref texture) = app_state.current_texture() {
            let avail = ui.content_region_avail();
            let fb_scale = ui.io().display_framebuffer_scale[0];
            let width_scale = avail[0] / texture.width as f32;
            let height_scale = avail[1] / texture.height as f32;
            let scale = image_display_scale(
                app_state.image_view_mode(),
                width_scale,
                height_scale,
                fb_scale,
            );
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

            imgui::Image::new(texture.id, display_size)
                .uv0([0.0, 0.0])
                .uv1([1.0, 1.0])
                .build(ui);

            let view_panel_min = ui.window_pos();
            let view_panel_max = [
                view_panel_min[0] + ui.window_size()[0],
                view_panel_min[1] + ui.window_size()[1],
            ];

            self.mouse_handler.handle(
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

    fn render_window_menu(&mut self, ui: &Ui, app_state: &mut ViewerState) {
        if !image_is_visible(app_state) {
            return;
        }

        let show = app_state.show_selection_window();
        if ui.menu_item_config("Selection").selected(show).build() {
            app_state.set_show_selection_window(!show);
        }
    }

    fn render_aux_windows(&mut self, ui: &Ui, app_state: &mut ViewerState) {
        if image_is_visible(app_state) && app_state.show_selection_window() {
            render_selection_window(ui, app_state, &mut self.selection_draft);
        }
    }
}

fn image_is_visible(app_state: &ViewerState) -> bool {
    should_show_image_selection(
        app_state.is_multi_select(),
        app_state.current_texture().is_some(),
    )
}

fn should_show_image_selection(is_multi_select: bool, has_texture: bool) -> bool {
    !is_multi_select && has_texture
}

fn image_display_scale(
    mode: ImageViewMode,
    width_scale: f32,
    height_scale: f32,
    fb_scale: f32,
) -> f32 {
    match mode {
        ImageViewMode::Original => 1.0 / fb_scale,
        ImageViewMode::FitToWindow => width_scale.min(height_scale),
        ImageViewMode::FitToWidth => width_scale,
    }
    .max(0.01)
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

fn render_selected_images_grid(ui: &Ui, app_state: &ViewerState, app_resources: &AppResources) {
    let cell = MULTI_VIEW_CELL_SIZE;
    let spacing = ui.clone_style().item_spacing[0].max(4.0);
    let available = ui.content_region_avail()[0].max(cell);
    let columns = (((available + spacing) / (cell + spacing)).floor() as usize).max(1);

    let mut column = 0usize;
    for entry in app_state.selected_entries() {
        if column != 0 {
            ui.same_line();
        }

        let (texture_id, uvs, img_w, img_h) = resolve_thumbnail(entry, app_resources);
        let (draw_w, draw_h) = fit_scale_in_cell(img_w, img_h, cell);
        // Keep each thumbnail centered in its cell for aligned rows.
        let origin = ui.cursor_screen_pos();
        let img_x = origin[0] + (cell - draw_w) * 0.5;
        let img_y = origin[1] + (cell - draw_h) * 0.5;
        ui.get_window_draw_list()
            .add_image(texture_id, [img_x, img_y], [img_x + draw_w, img_y + draw_h])
            .uv_min([uvs[0], uvs[1]])
            .uv_max([uvs[2], uvs[3]])
            .build();
        ui.dummy([cell, cell]);

        column += 1;
        if column >= columns {
            column = 0;
        }
    }
}

fn render_selection_window(ui: &Ui, app_state: &mut ViewerState, selection_draft: &mut Rect2D) {
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

            let current_selection = app_state.image_selection();
            if current_selection.is_none() {
                ui.text("No selection.");
                ui.text("Enter a region or drag on the image.");
            }

            let mut edited = current_selection.unwrap_or(*selection_draft);
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
                if current_selection.is_some() {
                    let clamped =
                        clamp_selection_rect_to_image(edited, [image_w as f32, image_h as f32]);
                    app_state.set_image_selection(Some(clamped));
                } else {
                    *selection_draft = edited;
                }
            }

            ui.dummy([0.0, 8.0]);
            if current_selection.is_none() {
                if ui.button("Create Selection") {
                    let clamped = clamp_selection_rect_to_image(
                        *selection_draft,
                        [image_w as f32, image_h as f32],
                    );
                    app_state.set_image_selection(Some(clamped));
                }
            } else {
                let _pad = ui.push_style_var(StyleVar::ItemSpacing([4.0, 4.0]));
                if ui.button("Copy to Clipboard") {
                    app_state.copy_region_to_clipboard(None);
                }

                if ui.button("Clear Selection") {
                    if let Some(selection) = app_state.image_selection() {
                        *selection_draft = selection;
                    }
                    app_state.clear_image_selection_state();
                }
            }
        });

    app_state.set_show_selection_window(open);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_modes_keep_their_display_scales() {
        // Each menu mode keeps its existing display scale.
        assert_eq!(
            image_display_scale(ImageViewMode::Original, 0.5, 0.25, 2.0),
            0.5
        );
        assert_eq!(
            image_display_scale(ImageViewMode::FitToWindow, 0.5, 0.25, 2.0),
            0.25
        );
        assert_eq!(
            image_display_scale(ImageViewMode::FitToWidth, 0.5, 0.25, 2.0),
            0.5
        );
    }

    #[test]
    fn selection_controls_only_show_with_a_single_visible_image() {
        // Show Selection only when a single image is visible.
        assert!(should_show_image_selection(false, true));
        assert!(!should_show_image_selection(false, false));
        assert!(!should_show_image_selection(true, true));
    }

    #[test]
    fn numeric_selection_stays_inside_image_bounds() {
        // Keep a numeric selection inside the image bounds.
        let entered = Rect2D::from_points(Point2D::new(10.0, 20.0), Point2D::new(50.0, 60.0));
        let result = clamp_selection_rect_to_image(entered, [25.0, 30.0]);
        assert_eq!(result.min, Point2D::new(10.0, 20.0));
        assert_eq!(result.max, Point2D::new(25.0, 30.0));
    }
}
