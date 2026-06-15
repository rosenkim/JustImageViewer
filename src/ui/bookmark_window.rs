use imgui::{
    Condition, MouseButton, SelectableFlags, TableColumnFlags, TableColumnSetup, TableFlags, Ui,
};

use crate::app::ViewerState;

use super::modal::render_yes_no_modal;

const DELETE_BOOKMARK_MODAL_ID: &str = "delete_bookmark_modal";

/// Render the bookmark list window and its actions.
pub fn render_bookmark_window(ui: &Ui, app_state: &mut ViewerState) {
    if !app_state.show_bookmark_window() {
        return;
    }

    let mut open = true;
    let mut open_target = None;
    let mut open_delete_modal = false;
    let bookmarks = app_state.bookmarks().to_vec();

    ui.window("Bookmarks")
        .opened(&mut open)
        .size([760.0, 420.0], Condition::FirstUseEver)
        .build(|| {
            ui.text(format!("Bookmarks: {}", bookmarks.len()));
            ui.separator();

            ui.child_window("bookmark_list")
                .size([0.0, 0.0])
                .border(false)
                .build(|| {
                    let table_flags = TableFlags::BORDERS_INNER_V
                        | TableFlags::ROW_BG
                        | TableFlags::RESIZABLE
                        | TableFlags::SCROLL_Y
                        | TableFlags::SIZING_STRETCH_PROP;

                    if let Some(_table) =
                        ui.begin_table_with_flags("bookmark_table", 3, table_flags)
                    {
                        let mut location_column = TableColumnSetup::new("Location");
                        location_column.flags = TableColumnFlags::WIDTH_STRETCH;
                        ui.table_setup_column_with(location_column);

                        let mut go_column = TableColumnSetup::new("Go");
                        go_column.flags = TableColumnFlags::WIDTH_FIXED;
                        go_column.init_width_or_weight = 56.0;
                        ui.table_setup_column_with(go_column);

                        let mut del_column = TableColumnSetup::new("Del");
                        del_column.flags = TableColumnFlags::WIDTH_FIXED;
                        del_column.init_width_or_weight = 56.0;
                        ui.table_setup_column_with(del_column);
                        ui.table_headers_row();

                        for bookmark in &bookmarks {
                            let path_text = bookmark.path.to_string_lossy().into_owned();
                            let key = bookmark.key();

                            ui.table_next_row();

                            ui.table_next_column();
                            let clicked = ui
                                .selectable_config(&format!("{path_text}##bookmark_{key}"))
                                .flags(SelectableFlags::ALLOW_DOUBLE_CLICK)
                                .span_all_columns(false)
                                .build();
                            if clicked && ui.is_mouse_double_clicked(MouseButton::Left) {
                                open_target = Some(bookmark.path.clone());
                            }
                            if ui.is_item_hovered() {
                                ui.tooltip_text(format!(
                                    "Bookmarked at {}",
                                    bookmark.bookmarked_at
                                ));
                            }

                            ui.table_next_column();
                            if ui.small_button(format!("Go##{key}")) {
                                open_target = Some(bookmark.path.clone());
                            }

                            ui.table_next_column();
                            if ui.small_button(format!("Del##{key}")) {
                                app_state.request_delete_bookmark(bookmark.path.clone());
                                // Defer open_popup to outside the child window so the modal
                                // ID resolves in the same scope as begin_popup_modal below.
                                open_delete_modal = true;
                            }
                        }
                    }
                });

            // open_popup and begin_popup_modal must share the same window ID context.
            if open_delete_modal {
                ui.open_popup(DELETE_BOOKMARK_MODAL_ID);
            }
            if let Some(decision) = render_yes_no_modal(
                ui,
                DELETE_BOOKMARK_MODAL_ID,
                "Delete this bookmark?",
                "Yes",
                "No",
            ) {
                if decision {
                    app_state.confirm_delete_bookmark();
                } else {
                    app_state.cancel_delete_bookmark();
                }
            }
        });

    if let Some(path) = open_target {
        app_state.open_bookmark_path(&path);
        open = false;
    }

    app_state.set_show_bookmark_window(open);
}
