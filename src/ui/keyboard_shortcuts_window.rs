use imgui::Condition;

pub fn render_keyboard_shortcuts_window(ui: &imgui::Ui, open: &mut bool) {
    ui.window("Keyboard Shortcuts")
        .size([440.0, 240.0], Condition::FirstUseEver)
        .opened(open)
        .build(|| {
            ui.text("Navigation");
            ui.separator();
            ui.bullet_text("Arrow Left / Arrow Up / Page Up: Previous image");
            ui.bullet_text("Arrow Right / Arrow Down / Page Down: Next image");

            ui.spacing();
            ui.text("General");
            ui.separator();
            ui.bullet_text("Ctrl/Cmd + O: Open directory");
            ui.bullet_text("Esc: Quit application");
        });
}
