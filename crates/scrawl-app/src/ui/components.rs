//! Left sidebar's Components tab: symbols, shared text and layer styles, color variables, and
//! linked libraries offering more of each.

use egui::{Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use scrawl_core::library::{Component, LibraryId};
use scrawl_core::{Fill, LayerId, LayerKind, SharedStyle, StyleId, SwatchId, SymbolId, ops};

use crate::app::ScrawlApp;
use crate::ui::icons::{self, Icon};
use crate::ui::paint::to_color32;

const ROW_HEIGHT: f32 = 26.0;

pub fn show(app: &mut ScrawlApp, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        symbols(app, ui);
        styles(app, ui, true);
        styles(app, ui, false);
        color_variables(app, ui);
        libraries(app, ui);
    });
}

fn header(ui: &mut Ui, title: &str, add_tip: &str, can_add: bool) -> bool {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(title).small().strong().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(can_add, |ui| icons::small_button(ui, Icon::Plus, add_tip).clicked()).inner
        })
        .inner
    })
    .inner
}

/// A short, wrapping explanation shown in place of an empty list.
fn hint(ui: &mut Ui, text: &str) {
    egui::Frame::NONE.inner_margin(egui::Margin { left: 8, right: 4, top: 0, bottom: 2 }).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(text).weak()).wrap());
    });
}

/// Top-level selected layers, excluding containers, which have no style of their own.
fn styleable_selection(app: &ScrawlApp) -> Vec<LayerId> {
    let page = app.page();
    ops::normalize_selection(page, &app.selection)
        .into_iter()
        .filter(|&id| page.find(id).is_some_and(|l| !l.is_artboard() && !l.is_group()))
        .collect()
}

/// A full-width clickable row: a preview drawn by `preview` in a square on the left, then `name`.
fn row(ui: &mut Ui, name: &str, preview: impl FnOnce(&egui::Painter, Rect)) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    let visuals = ui.visuals();
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(20.0, 20.0));
    preview(ui.painter(), icon);
    let galley = ui.painter().layout_no_wrap(name.to_owned(), FontId::proportional(13.0), visuals.text_color());
    ui.painter().galley(pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0), galley, visuals.text_color());
    resp
}

/// Symbol masters. Clicking one inserts an instance.
fn symbols(app: &mut ScrawlApp, ui: &mut Ui) {
    if header(ui, "SYMBOLS", "Create a symbol from the selection (⌥⌘Y)", !app.selection.is_empty()) {
        app.create_symbol();
    }
    let masters: Vec<(SymbolId, String, f64, f64)> = app
        .doc
        .symbol_masters()
        .into_iter()
        .filter_map(|(_, l)| Some((l.master_of()?, l.name.clone(), l.frame.width, l.frame.height)))
        .collect();
    if masters.is_empty() {
        hint(ui, "Select layers and press + to make a reusable symbol.");
    }
    for (symbol, name, w, h) in masters {
        ui.push_id(symbol, |ui| {
            let resp = row(ui, &name, |p, r| {
                icons::paint(p, r.shrink(2.0), Icon::Symbol, crate::canvas::SYMBOL);
            })
            .on_hover_text(format!("{w:.0}×{h:.0} — click to insert"));
            if resp.clicked() {
                app.insert_symbol(symbol);
            }
            resp.context_menu(|ui| symbol_menu(app, ui, symbol));
        });
    }
}

fn symbol_menu(app: &mut ScrawlApp, ui: &mut Ui, symbol: SymbolId) {
    if let Some((page, id)) = app.doc.find_master(symbol)
        && let Some(l) = app.doc.page_mut(page).and_then(|p| p.find_mut(id))
    {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut l.name);
        });
    }
    if ui.button("Edit Master").clicked() {
        app.edit_master(symbol);
        ui.close();
    }
    if ui.button("Select Instances").clicked() {
        let mut ids = vec![];
        app.page().walk(|l| {
            if matches!(&l.kind, LayerKind::Symbol(i) if i.symbol_id == symbol) {
                ids.push(l.id);
            }
        });
        app.selection = ids;
        ui.close();
    }
    if ui.button("Delete Symbol").on_hover_text("Instances become groups").clicked() {
        scrawl_core::symbols::remove_symbol(&mut app.doc, symbol);
        app.sanitize_selection();
        ui.close();
    }
}

/// Text styles (`text`) or layer styles. Clicking one applies it to the selection.
fn styles(app: &mut ScrawlApp, ui: &mut Ui, text: bool) {
    let selection = styleable_selection(app);
    let source = selection.first().and_then(|&id| app.page().find(id)).filter(|l| l.is_text() == text).cloned();
    let (title, tip) = if text {
        ("TEXT STYLES", "Create a text style from the selected text layer")
    } else {
        ("LAYER STYLES", "Create a layer style from the selected layer")
    };
    if header(ui, title, tip, source.is_some())
        && let Some(layer) = source
    {
        let id = app.doc.create_shared_style(&layer);
        let s = app.doc.shared_style(id).cloned();
        if let (Some(s), Some(l)) = (s, app.page_mut().find_mut(layer.id)) {
            s.apply(l);
        }
    }
    let list = if text { app.doc.text_styles.clone() } else { app.doc.layer_styles.clone() };
    if list.is_empty() {
        hint(ui, if text { "Select a text layer and press + to share its style." } else { "Select a shape and press + to share its style." });
    }
    for s in list {
        ui.push_id(s.id, |ui| {
            let resp = row(ui, &s.name, |p, r| style_preview(p, r, &s)).on_hover_text("Click to apply to the selection");
            if resp.clicked() {
                apply_style(app, &s);
            }
            resp.context_menu(|ui| style_menu(app, ui, s.id));
        });
    }
}

fn style_preview(p: &egui::Painter, r: Rect, s: &SharedStyle) {
    let fill = s.style.fills.iter().rev().find(|f| f.enabled).map_or(Color32::TRANSPARENT, |f| match f.fill_type {
        scrawl_core::FillType::Color => to_color32(f.color),
        scrawl_core::FillType::Gradient => to_color32(f.gradient.color_at(0.5)),
    });
    if let Some(ts) = &s.text {
        let size = (ts.font_size as f32).clamp(9.0, 18.0);
        let color = if fill == Color32::TRANSPARENT { p.ctx().style_of(p.ctx().theme()).visuals.text_color() } else { fill };
        p.text(r.center(), egui::Align2::CENTER_CENTER, "Ag", FontId::proportional(size), color);
        return;
    }
    let border = s.style.borders.iter().find(|b| b.enabled).map(|b| Stroke::new(1.5, to_color32(b.color)));
    let r = r.shrink(2.0);
    p.rect_filled(r, 3.0, fill);
    p.rect_stroke(r, 3.0, border.unwrap_or(Stroke::new(1.0, Color32::from_black_alpha(40))), egui::StrokeKind::Inside);
}

/// Applies a shared style to every compatible selected layer.
fn apply_style(app: &mut ScrawlApp, s: &SharedStyle) {
    let ids = styleable_selection(app);
    let page = app.page_mut();
    for id in ids {
        if let Some(l) = page.find_mut(id)
            && l.is_text() == s.is_text()
        {
            s.apply(l);
        }
    }
}

fn style_menu(app: &mut ScrawlApp, ui: &mut Ui, id: StyleId) {
    if let Some(s) = app.doc.layer_styles.iter_mut().chain(&mut app.doc.text_styles).find(|s| s.id == id) {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut s.name);
        });
    }
    if ui.button("Select Layers Using Style").clicked() {
        let mut ids = vec![];
        app.page().walk(|l| {
            if l.shared_style == Some(id) {
                ids.push(l.id);
            }
        });
        app.selection = ids;
        ui.close();
    }
    if ui.button("Delete Style").clicked() {
        app.doc.remove_shared_style(id);
        ui.close();
    }
}

fn color_variables(app: &mut ScrawlApp, ui: &mut Ui) {
    if header(ui, "COLOR VARIABLES", "Create a color variable", true) {
        // Start from the selection's top fill, if any.
        let page = app.page();
        let color = styleable_selection(app)
            .first()
            .and_then(|&id| page.find(id))
            .and_then(|l| l.style.fills.last().map(|f| f.color))
            .unwrap_or(scrawl_core::Color::DEFAULT_FILL);
        app.doc.add_swatch(color);
    }
    let vars = app.doc.swatches.clone();
    if vars.is_empty() {
        hint(ui, "Colors used across the document. Fills and borders can link to them.");
    }
    for v in vars {
        ui.push_id(v.id, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let chip = crate::ui::paint::swatch_chip(ui, v.color, false).on_hover_text("Edit color");
                egui::Popup::from_toggle_button_response(&chip)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        let mut c = to_color32(v.color);
                        if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::OnlyBlend) {
                            let [r, g, b, a] = c.to_srgba_unmultiplied();
                            app.doc.set_swatch_color(v.id, scrawl_core::Color::from_rgba8(r, g, b, a));
                        }
                    });
                let label = egui::Atoms::new((v.name.as_str(), egui::Atom::grow(), RichText::new(v.color.to_hex()).weak().small()));
                let resp = ui
                    .add(egui::Button::new(label).frame_when_inactive(false).min_size(vec2(ui.available_width(), 0.0)))
                    .on_hover_text("Click to fill the selection with this color");
                if resp.clicked() {
                    apply_variable(app, v.id, v.color);
                }
                resp.context_menu(|ui| variable_menu(app, ui, v.id));
            });
        });
    }
}

/// Sets the top fill of each selected layer to a color variable, adding a fill if needed.
fn apply_variable(app: &mut ScrawlApp, id: SwatchId, color: scrawl_core::Color) {
    let ids = styleable_selection(app);
    let page = app.page_mut();
    for lid in ids {
        let Some(l) = page.find_mut(lid) else { continue };
        if !l.is_closed() && !matches!(l.kind, LayerKind::Text(_)) {
            // Open paths show color through their borders.
            if let Some(b) = l.style.borders.last_mut() {
                b.color = color;
                b.swatch = Some(id);
            }
            continue;
        }
        if l.style.fills.is_empty() {
            l.style.fills.push(Fill::solid(color));
        }
        let f = l.style.fills.last_mut().expect("just ensured");
        f.fill_type = scrawl_core::FillType::Color;
        f.color = color;
        f.swatch = Some(id);
    }
}

fn variable_menu(app: &mut ScrawlApp, ui: &mut Ui, id: SwatchId) {
    if let Some(v) = app.doc.swatches.iter_mut().find(|v| v.id == id) {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut v.name);
        });
    }
    if ui.button("Delete Variable").clicked() {
        app.doc.remove_swatch(id);
        ui.close();
    }
}

fn libraries(app: &mut ScrawlApp, ui: &mut Ui) {
    if header(ui, "LIBRARIES", "Add a library…", true) {
        app.add_library();
    }
    let libs = app.doc.libraries.clone();
    if libs.is_empty() {
        hint(ui, "Link another Scrawl document to use its symbols, styles and colors.");
        return;
    }
    for lib in libs {
        ui.push_id(lib.id, |ui| library(app, ui, lib.id, &lib.name, &lib.path));
    }
}

/// One library: its status, and what it offers. Using an item copies it in.
fn library(app: &mut ScrawlApp, ui: &mut Ui, id: LibraryId, name: &str, path: &str) {
    let resp = egui::CollapsingHeader::new(RichText::new(name).strong()).id_salt(("library", id)).default_open(true).show(ui, |ui| {
        // Just what's listed, so the library document isn't borrowed while the app changes.
        let listing = app.library_doc(id).map(|source| {
            let symbols: Vec<(SymbolId, String)> =
                source.symbol_masters().into_iter().filter_map(|(_, m)| Some((m.master_of()?, m.name.clone()))).collect();
            let styles: Vec<SharedStyle> = source.text_styles.iter().chain(&source.layer_styles).cloned().collect();
            (symbols, styles, source.swatches.clone(), app.doc.library_updates(id, source).len())
        });
        let Some((symbols, styles, swatches, updates)) = listing else {
            let why = match app.libraries.get(&id) {
                Some(Err(e)) => e.clone(),
                _ => "Not loaded".into(),
            };
            ui.colored_label(ui.visuals().warn_fg_color, format!("Can't read {path}: {why}"));
            return;
        };
        if updates > 0 {
            ui.horizontal(|ui| {
                ui.label(match updates {
                    1 => "1 update available".to_owned(),
                    n => format!("{n} updates available"),
                });
                if ui.button("Update").clicked() {
                    app.update_library(id);
                }
            });
        }
        for (symbol, name) in symbols {
            let resp = row(ui, &name, |p, r| icons::paint(p, r.shrink(2.0), Icon::Symbol, crate::canvas::SYMBOL))
                .on_hover_text("Click to insert");
            if resp.clicked() && app.import_component(id, Component::Symbol(symbol)) {
                app.insert_symbol(symbol);
            }
        }
        for s in &styles {
            let resp = row(ui, &s.name, |p, r| style_preview(p, r, s)).on_hover_text("Click to apply to the selection");
            if resp.clicked()
                && app.import_component(id, Component::Style(s.id))
                && let Some(s) = app.doc.shared_style(s.id).cloned()
            {
                apply_style(app, &s);
            }
        }
        if !swatches.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                ui.add_space(8.0);
                for v in &swatches {
                    let resp = crate::ui::paint::swatch_chip(ui, v.color, false).on_hover_text(format!("{} — click to fill the selection", v.name));
                    if resp.clicked() && app.import_component(id, Component::Swatch(v.id)) {
                        apply_variable(app, v.id, v.color);
                    }
                }
            });
        }
    });
    resp.header_response.on_hover_text(path).context_menu(|ui| {
        if ui.button("Reload").clicked() {
            app.reload_libraries();
            ui.close();
        }
        if ui.button("Remove Library").on_hover_text("Components in use stay, as local ones").clicked() {
            app.doc.remove_library(id);
            app.libraries.remove(&id);
            ui.close();
        }
    });
}
