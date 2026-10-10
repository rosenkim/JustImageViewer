use crate::app::{ViewerState, format_file_size};
use crate::core::media::MediaEntry;
use crate::render::app_resources::AppResources;
use imgui::{MouseCursor, StyleVar, Ui};

use super::image_view_panel::ImageViewPanel;
use super::layout_constants::SPLITTER_WIDTH;

/// A content panel draws inside the right-hand view area.
pub trait ViewContentPanel {
    fn render(
        &mut self,
        ui: &Ui,
        app_state: &mut ViewerState,
        app_resources: &AppResources,
        is_pending: bool,
    );

    fn render_window_menu(&mut self, _ui: &Ui, _app_state: &mut ViewerState) {}

    fn render_aux_windows(&mut self, _ui: &Ui, _app_state: &mut ViewerState) {}
}

pub struct ViewPanel {
    content: Box<dyn ViewContentPanel>,
}

impl ViewPanel {
    pub fn new() -> Self {
        Self::with_content(Box::new(ImageViewPanel::new()))
    }

    /// Keep the right-hand layout independent from the content type.
    pub fn with_content(content: Box<dyn ViewContentPanel>) -> Self {
        Self { content }
    }

    pub fn render(
        &mut self,
        ui: &Ui,
        app_state: &mut ViewerState,
        app_resources: &AppResources,
        is_pending: bool,
    ) {
        ui.child_window("ViewerPanel")
            .size([0.0, 0.0])
            .border(true)
            .build(|| {
                const INFO_WIDTH: f32 = 200.0;
                let _pad = ui.push_style_var(StyleVar::ItemSpacing([4.0, 4.0]));
                let mut content_width = ui.content_region_avail()[0];
                let show_info =
                    app_state.show_info() && content_width > INFO_WIDTH + SPLITTER_WIDTH;

                if show_info {
                    content_width -= INFO_WIDTH + SPLITTER_WIDTH;
                }

                ui.child_window("image_region")
                    .size([content_width.max(100.0), 0.0])
                    .flags(imgui::WindowFlags::HORIZONTAL_SCROLLBAR)
                    .build(|| {
                        self.content
                            .render(ui, app_state, app_resources, is_pending)
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
                        if app_state.is_multi_select() {
                            render_multi_selection_info(ui, app_state);
                        } else {
                            render_file_info(ui, app_state.current_entry());
                        }
                    });
                }
            });
    }

    pub fn render_window_menu(&mut self, ui: &Ui, app_state: &mut ViewerState) {
        self.content.render_window_menu(ui, app_state);
    }

    pub fn render_aux_windows(&mut self, ui: &Ui, app_state: &mut ViewerState) {
        self.content.render_aux_windows(ui, app_state);
    }
}

fn render_file_info(ui: &Ui, entry: Option<&MediaEntry>) {
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

/// Show the count and file names for a multi-selection in the info area.
fn render_multi_selection_info(ui: &Ui, app_state: &ViewerState) {
    ui.text(format!("{} files selected", app_state.selected_count()));
    ui.separator();
    for entry in app_state.selected_entries() {
        ui.text_wrapped(&entry.file_name);
    }
}
