use imgui::Ui;

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
        ui.text_wrapped(message);
        ui.spacing();

        if ui.button(yes_label) {
            ui.close_current_popup();
            decision = Some(true);
        }

        ui.same_line();
        if ui.button(no_label) {
            ui.close_current_popup();
            decision = Some(false);
        }
    }

    decision
}
