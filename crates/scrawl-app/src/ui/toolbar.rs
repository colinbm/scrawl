use egui::Ui;
use scrawl_core::ops::{Align, BooleanOp, Reorder};

use crate::app::ScrawlApp;
use crate::commands::{self, Command, Entry};
use crate::canvas::Tool;
use crate::ui::icons::{self, Icon};

pub fn boolean_icon(op: BooleanOp) -> Icon {
    match op {
        BooleanOp::Union => Icon::Union,
        BooleanOp::Subtract => Icon::Subtract,
        BooleanOp::Intersect => Icon::Intersect,
        BooleanOp::Difference => Icon::Difference,
    }
}

/// The in-window menu bar, used where there's no native one.
pub fn menu_bar(app: &mut ScrawlApp, ui: &mut Ui) {
    let menus = commands::menus(app, &app.menu_context);
    let mut chosen = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in &menus {
            ui.menu_button(menu.title, |ui| entries(ui, &menu.entries, &mut chosen));
        }
    });
    if let Some(command) = chosen {
        commands::execute(app, command, ui.ctx());
    }
}

fn entries(ui: &mut Ui, list: &[Entry], chosen: &mut Option<Command>) {
    for entry in list {
        match entry {
            Entry::Separator => {
                ui.separator();
            }
            Entry::Submenu { label, entries: sub, enabled } => {
                ui.add_enabled_ui(*enabled, |ui| {
                    ui.menu_button(label, |ui| entries(ui, sub, chosen));
                });
            }
            Entry::Item(item) => {
                let clicked = match item.checked {
                    Some(on) => {
                        let mut on = on;
                        let tip = item.shortcut.map(|s| ui.ctx().format_shortcut(&s)).unwrap_or_default();
                        let r = ui.add_enabled(item.enabled, egui::Checkbox::new(&mut on, &item.label));
                        if tip.is_empty() { r.clicked() } else { r.on_hover_text(tip).clicked() }
                    }
                    None => {
                        let mut b = egui::Button::new(&item.label);
                        if let Some(sc) = item.shortcut {
                            b = b.shortcut_text(ui.ctx().format_shortcut(&sc));
                        } else if !item.hint.is_empty() {
                            b = b.shortcut_text(item.hint);
                        }
                        ui.add_enabled(item.enabled, b).clicked()
                    }
                };
                if clicked {
                    *chosen = Some(item.command.clone());
                    ui.close();
                }
            }
        }
    }
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
                let tip = format!("{} ({})", op.label(), Command::Boolean(op).shortcut().map(|s| ui.ctx().format_shortcut(&s)).unwrap_or_default());
                if icons::button(ui, boolean_icon(op), false, &tip).clicked() {
                    app.boolean(op);
                }
            }
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let tip = format!("Preview ({})", Command::Preview.shortcut().map(|s| ui.ctx().format_shortcut(&s)).unwrap_or_default());
            if icons::button(ui, Icon::Play, app.preview.is_some(), &tip).clicked() {
                if app.preview.is_some() {
                    app.preview = None;
                } else {
                    crate::preview::open(app);
                }
            }
            ui.separator();
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
