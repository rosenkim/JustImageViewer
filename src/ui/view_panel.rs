use crate::app::{ViewerState, format_file_size};
use crate::core::media::{MediaEntry, MediaType};
use crate::render::app_resources::AppResources;
use crate::render::view_gpu::ViewGpuContext;
use imgui::{MouseCursor, StyleVar, Ui};

use super::image_view_panel::ImageViewPanel;
use super::layout_constants::SPLITTER_WIDTH;
use super::model_view_panel::ModelViewPanel;

/// A content panel draws inside the right-hand view area.
pub trait ViewContentPanel {
    /// Ask the panel to reload its current content.
    fn request_reload(&mut self) {}

    /// Prepare content and GPU resources for the next frame.
    fn prepare_frame(&mut self, _app_state: &ViewerState, _ctx: &mut ViewGpuContext<'_>) {}

    /// Add this panel's GPU commands to the frame.
    fn encode_frame(&mut self, _encoder: &mut wgpu::CommandEncoder) {}

    /// Release GPU resources owned by this panel.
    fn release(&mut self, _ctx: &mut ViewGpuContext<'_>) {}

    /// Return whether the panel still has work in progress.
    fn is_pending(&self) -> bool {
        false
    }

    /// Draw the panel inside the view area.
    fn render(
        &mut self,
        ui: &Ui,
        app_state: &mut ViewerState,
        app_resources: &AppResources,
        is_pending: bool,
    );

    /// Draw this panel's items in the Window menu.
    fn render_window_menu(&mut self, _ui: &Ui, _app_state: &mut ViewerState) {}

    /// Draw any extra windows owned by this panel.
    fn render_aux_windows(&mut self, _ui: &Ui, _app_state: &mut ViewerState) {}
}

/// Owns the image and model panels and shows the active one.
pub struct ViewPanel {
    /// Shows images and the grid for multiple selected files.
    image_content: Box<dyn ViewContentPanel>,
    /// Shows 3D models.
    model_content: Box<dyn ViewContentPanel>,
}

impl ViewPanel {
    /// Ask both content panels to reload.
    pub fn request_reload(&mut self) {
        self.image_content.request_reload();
        self.model_content.request_reload();
    }

    /// Prepare both content panels for the next frame.
    pub fn prepare_frame(&mut self, app_state: &ViewerState, ctx: &mut ViewGpuContext<'_>) {
        self.image_content.prepare_frame(app_state, ctx);
        self.model_content.prepare_frame(app_state, ctx);
    }

    /// Add GPU commands from both content panels to the frame.
    pub fn encode_frame(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.image_content.encode_frame(encoder);
        self.model_content.encode_frame(encoder);
    }

    /// Release GPU resources from both content panels.
    pub fn release(&mut self, ctx: &mut ViewGpuContext<'_>) {
        self.image_content.release(ctx);
        self.model_content.release(ctx);
    }

    /// Return whether either content panel still has work in progress.
    pub fn is_pending(&self) -> bool {
        self.image_content.is_pending() || self.model_content.is_pending()
    }

    /// Create a view panel with the standard image and model panels.
    pub fn new() -> Self {
        Self::with_contents(
            Box::new(ImageViewPanel::new()),
            Box::new(ModelViewPanel::new()),
        )
    }

    /// Create a view panel with the given image and model panels.
    pub fn with_contents(
        image_content: Box<dyn ViewContentPanel>,
        model_content: Box<dyn ViewContentPanel>,
    ) -> Self {
        Self {
            image_content,
            model_content,
        }
    }

    /// Choose the panel for the current selection.
    fn active_content(&mut self, app_state: &ViewerState) -> &mut dyn ViewContentPanel {
        // Keep the shared thumbnail grid for multiple selected files.
        let media_type = if app_state.is_multi_select() {
            MediaType::Image2D
        } else {
            app_state
                .current_entry()
                .map(|entry| entry.format.media_type())
                .unwrap_or(MediaType::Image2D)
        };

        match media_type {
            MediaType::Image2D => self.image_content.as_mut(),
            MediaType::Model3D => self.model_content.as_mut(),
        }
    }

    /// Draw the active content and the optional file information area.
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
                        self.active_content(app_state).render(
                            ui,
                            app_state,
                            app_resources,
                            is_pending,
                        )
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

    /// Draw the active content panel's Window menu items.
    pub fn render_window_menu(&mut self, ui: &Ui, app_state: &mut ViewerState) {
        self.active_content(app_state)
            .render_window_menu(ui, app_state);
    }

    /// Draw the active content panel's extra windows.
    pub fn render_aux_windows(&mut self, ui: &Ui, app_state: &mut ViewerState) {
        self.active_content(app_state)
            .render_aux_windows(ui, app_state);
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
