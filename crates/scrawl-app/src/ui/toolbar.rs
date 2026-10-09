use egui::{KeyboardShortcut, Ui};
use scrawl_core::ops::{Align, BooleanOp, Reorder};

use crate::app::{ScrawlApp, shortcuts};
use crate::canvas::Tool;
use crate::ui::icons::{self, Icon};

fn item(ui: &mut Ui, label: &str, shortcut: Option<KeyboardShortcut>, enabled: bool) -> bool {
    let mut b = egui::Button::new(label);
    if let Some(sc) = shortcut {
        b = b.shortcut_text(ui.ctx().format_shortcut(&sc));
    }
    ui.add_enabled(enabled, b).clicked()
}

fn key_item(ui: &mut Ui, label: &str, key: &str) -> bool {
    ui.add(egui::Button::new(label).shortcut_text(key)).clicked()
}

pub fn boolean_icon(op: BooleanOp) -> Icon {
    match op {
        BooleanOp::Union => Icon::Union,
        BooleanOp::Subtract => Icon::Subtract,
        BooleanOp::Intersect => Icon::Intersect,
        BooleanOp::Difference => Icon::Difference,
    }
}

pub fn menu_bar(app: &mut ScrawlApp, ui: &mut Ui) {
    let has_sel = !app.selection.is_empty();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if item(ui, "New", Some(shortcuts::NEW), true) {
                app.new_document();
            }
            if item(ui, "Open…", Some(shortcuts::OPEN), true) {
                app.open();
            }
            ui.separator();
            if item(ui, "Save", Some(shortcuts::SAVE), true) {
                app.save();
            }
            if item(ui, "Save As…", Some(shortcuts::SAVE_AS), true) {
                app.save_as();
            }
        });
        ui.menu_button("Edit", |ui| {
            if item(ui, "Undo", Some(shortcuts::UNDO), app.history.can_undo() || app.history.is_dirty(&app.doc)) {
                app.undo();
            }
            if item(ui, "Redo", Some(shortcuts::REDO), app.history.can_redo()) {
                app.redo();
            }
            ui.separator();
            if item(ui, "Cut", None, has_sel) {
                app.cut(ui.ctx());
            }
            if item(ui, "Copy", None, has_sel) {
                app.copy(ui.ctx());
            }
            if item(ui, "Paste", None, true) {
                app.paste(None);
            }
            if item(ui, "Duplicate", Some(shortcuts::DUPLICATE), has_sel) {
                app.duplicate_selection();
            }
            if item(ui, "Delete", None, has_sel) {
                app.delete_selection();
            }
            ui.separator();
            if item(ui, "Select All", Some(shortcuts::SELECT_ALL), true) {
                app.select_all();
            }
        });
        ui.menu_button("Insert", |ui| {
            for tool in Tool::INSERT {
                if key_item(ui, tool.label(), tool.key()) {
                    app.tool = tool;
                }
            }
        });
        ui.menu_button("Arrange", |ui| {
            for (label, sc, how) in [
                ("Bring Forward", shortcuts::FORWARD, Reorder::Forward),
                ("Bring to Front", shortcuts::TO_FRONT, Reorder::ToFront),
                ("Send Backward", shortcuts::BACKWARD, Reorder::Backward),
                ("Send to Back", shortcuts::TO_BACK, Reorder::ToBack),
            ] {
                if item(ui, label, Some(sc), has_sel) {
                    app.reorder(how);
                }
            }
            ui.separator();
            if item(ui, "Group", Some(shortcuts::GROUP), has_sel) {
                app.group_selection();
            }
            if item(ui, "Ungroup", Some(shortcuts::UNGROUP), has_sel) {
                app.ungroup_selection();
            }
            ui.separator();
            ui.menu_button("Align", |ui| {
                for (label, how) in ALIGNMENTS.map(|(_, l, a)| (l, a)) {
                    if item(ui, label, None, has_sel) {
                        app.align(how);
                    }
                }
            });
        });
        ui.menu_button("Layer", |ui| {
            let multi = app.selection.len() > 1;
            ui.menu_button("Combine", |ui| {
                for op in BooleanOp::ALL {
                    if item(ui, op.label(), Some(shortcuts::boolean(op)), multi) {
                        app.boolean(op);
                    }
                }
            });
            ui.menu_button("Path", |ui| {
                if item(ui, "Edit Points", None, app.selection.len() == 1) {
                    app.enter_vector_edit(app.selection[0]);
                }
                if item(ui, "Convert to Path", Some(shortcuts::CONVERT_TO_PATH), has_sel) {
                    app.convert_to_paths();
                }
                if item(ui, "Convert to Outlines", Some(shortcuts::OUTLINE), has_sel) {
                    app.outline_strokes();
                }
                if item(ui, "Reverse Path Direction", None, has_sel) {
                    app.reverse_paths();
                }
            });
        });
        ui.menu_button("View", |ui| {
            if item(ui, "Zoom In", Some(shortcuts::ZOOM_IN), true) {
                app.step_zoom(true);
            }
            if item(ui, "Zoom Out", Some(shortcuts::ZOOM_OUT), true) {
                app.step_zoom(false);
            }
            if item(ui, "Actual Size", Some(shortcuts::ACTUAL_SIZE), true) {
                app.actual_size();
            }
            if item(ui, "Zoom to Fit", Some(shortcuts::ZOOM_FIT), true) {
                app.zoom_to_fit();
            }
            if item(ui, "Zoom to Selection", Some(shortcuts::ZOOM_SELECTION), has_sel) {
                app.zoom_to_selection(crate::viewport::MAX_ZOOM);
            }
        });
    });
}

pub const ALIGNMENTS: [(Icon, &str, Align); 6] = [
    (Icon::AlignLeft, "Align Left", Align::Left),
    (Icon::AlignHCenter, "Align Horizontally", Align::HCenter),
    (Icon::AlignRight, "Align Right", Align::Right),
    (Icon::AlignTop, "Align Top", Align::Top),
    (Icon::AlignVCenter, "Align Vertically", Align::VCenter),
    (Icon::AlignBottom, "Align Bottom", Align::Bottom),
];

pub fn toolbar(app: &mut ScrawlApp, ui: &mut Ui) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        for tool in [Tool::Select].into_iter().chain(Tool::INSERT) {
            let tip = if tool.key().is_empty() { tool.label().to_owned() } else { format!("{} ({})", tool.label(), tool.key()) };
            if icons::button(ui, tool.icon(), app.tool == tool, &tip).clicked() {
                app.tool = tool;
            }
        }
        ui.separator();
        let has_sel = !app.selection.is_empty();
        ui.add_enabled_ui(has_sel, |ui| {
            if icons::button(ui, Icon::Group, false, "Group (⌘G)").clicked() {
                app.group_selection();
            }
            if icons::button(ui, Icon::Ungroup, false, "Ungroup (⇧⌘G)").clicked() {
                app.ungroup_selection();
            }
            ui.separator();
            if icons::button(ui, Icon::Forward, false, "Bring Forward (⌘])").clicked() {
                app.reorder(Reorder::Forward);
            }
            if icons::button(ui, Icon::Backward, false, "Send Backward (⌘[)").clicked() {
                app.reorder(Reorder::Backward);
            }
        });
        ui.separator();
        ui.add_enabled_ui(app.selection.len() > 1, |ui| {
            for op in BooleanOp::ALL {
                let tip = format!("{} ({})", op.label(), ui.ctx().format_shortcut(&shortcuts::boolean(op)));
                if icons::button(ui, boolean_icon(op), false, &tip).clicked() {
                    app.boolean(op);
                }
            }
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, Icon::Plus, false, "Zoom In").clicked() {
                app.step_zoom(true);
            }
            let pct = format!("{:.0}%", app.viewport().zoom * 100.0);
            ui.menu_button(pct, |ui| {
                for (label, zoom) in [("50%", 0.5), ("100%", 1.0), ("200%", 2.0), ("400%", 4.0)] {
                    if ui.button(label).clicked() {
                        let canvas = app.canvas.rect;
                        app.viewport_mut().set_zoom_about(canvas, canvas.center(), zoom);
                    }
                }
                ui.separator();
                if ui.button("Zoom to Fit").clicked() {
                    app.zoom_to_fit();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Zoom to Selection")).clicked() {
                    app.zoom_to_selection(crate::viewport::MAX_ZOOM);
                }
            });
            if icons::button(ui, Icon::Minus, false, "Zoom Out").clicked() {
                app.step_zoom(false);
            }
        });
    });
    ui.add_space(2.0);
}
