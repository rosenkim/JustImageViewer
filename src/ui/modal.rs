use imgui::Ui;

const MODAL_MIN_WIDTH: f32 = 300.0;
const BUTTON_WIDTH: f32 = 100.0;

/// Render a reusable Yes/No modal dialog.
pub fn render_yes_no_modal(
    ui: &Ui,
    popup_id: &str,
    message: &str,
    yes_label: &str,
    no_label: &str,
) -> Option<bool> {
    let mut decision = None;

    if let Some(_popup) = ui
        .modal_popup_config(popup_id)
        .always_auto_resize(true)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .begin_popup()
    {
        // Enforce a minimum width so the dialog never feels cramped.
        ui.set_next_item_width(MODAL_MIN_WIDTH);
        ui.text_wrapped(message);
        ui.spacing();
        ui.separator();
        ui.spacing();

        // Centre the two fixed-width buttons with a gap between them.
        let gap = ui.clone_style().item_spacing[0] * 3.0;
        let total = BUTTON_WIDTH * 2.0 + gap;
        let offset = (MODAL_MIN_WIDTH - total).max(0.0) / 2.0;
        ui.set_cursor_pos([ui.cursor_pos()[0] + offset, ui.cursor_pos()[1]]);

        if ui.button_with_size(yes_label, [BUTTON_WIDTH, 0.0]) {
            ui.close_current_popup();
            decision = Some(true);
        }

        ui.same_line_with_spacing(0.0, gap);

        if ui.button_with_size(no_label, [BUTTON_WIDTH, 0.0]) {
            ui.close_current_popup();
            decision = Some(false);
        }
    }

    decision
}
