//! Right sidebar: properties of the selection.

use egui::{DragValue, Id, RichText, Ui};
use scrawl_core::kurbo::Rect;
use scrawl_core::kurbo::Point;
use scrawl_core::{Border, BorderPosition, Color, Fill, FillRule, Layer, LayerKind, PointMode};

use crate::app::ScrawlApp;
use crate::canvas::Tool;
use crate::ui::icons::{self, Icon};
use crate::ui::toolbar::ALIGNMENTS;

const FIELD_WIDTH: f32 = 64.0;

/// (name, width, height)
type Preset = (&'static str, f64, f64);

const ARTBOARD_PRESETS: &[(&str, &[Preset])] = &[
    ("Apple", &[
        ("iPhone 16", 393.0, 852.0),
        ("iPhone 16 Pro Max", 440.0, 956.0),
        ("iPhone SE", 375.0, 667.0),
        ("iPad Pro 11\"", 834.0, 1194.0),
        ("iPad Pro 13\"", 1032.0, 1376.0),
        ("MacBook Air", 1280.0, 832.0),
    ]),
    ("Android", &[("Android Compact", 412.0, 917.0), ("Android Medium", 700.0, 840.0)]),
    ("Web", &[("Desktop", 1440.0, 1024.0), ("Desktop HD", 1920.0, 1080.0), ("Tablet", 768.0, 1024.0)]),
    ("Social", &[("Square Post", 1080.0, 1080.0), ("Story", 1080.0, 1920.0)]),
];

pub fn show(app: &mut ScrawlApp, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        alignment_row(app, ui);
        ui.separator();

        if app.tool == Tool::Artboard {
            artboard_presets(app, ui);
            return;
        }
        if app.canvas.edit.is_some() {
            points_section(app, ui);
            return;
        }
        match app.selection[..] {
            [] => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.weak("No selection"));
            }
            [id] => {
                let Some(original) = app.page().find(id).cloned() else { return };
                let mut layer = original.clone();
                layer_properties(app, ui, &mut layer);
                if layer != original {
                    let page = app.page_mut();
                    if let Some(l) = page.find_mut(id) {
                        *l = layer;
                    }
                    if let Some(Some(parent)) = page.parent(id) {
                        page.refit_ancestor_groups(parent);
                    }
                }
            }
            _ => multi_selection(app, ui),
        }
    });
}

fn alignment_row(app: &mut ScrawlApp, ui: &mut Ui) {
    let enabled = !app.selection.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (icon, label, how) in ALIGNMENTS {
                if icons::button(ui, icon, false, label).clicked() {
                    app.align(how);
                }
            }
        });
    });
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title.to_uppercase()).small().strong().weak());
    ui.add_space(2.0);
}

/// A labelled numeric field. Returns whether the value changed.
fn num(ui: &mut Ui, label: &str, v: &mut f64, speed: f64, suffix: &str) -> bool {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let changed = ui
            .add_sized([FIELD_WIDTH, 20.0], DragValue::new(v).speed(speed).max_decimals(2).suffix(suffix))
            .changed();
        ui.label(RichText::new(label).small().weak());
        changed
    })
    .inner
}

fn layer_properties(app: &mut ScrawlApp, ui: &mut Ui, layer: &mut Layer) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        icons::paint(ui.painter(), rect, icons::for_layer(&layer.kind), ui.visuals().text_color());
        ui.strong(&layer.name);
        ui.weak(layer.kind.type_name());
    });

    section(ui, "Position");
    ui.horizontal(|ui| {
        num(ui, "X", &mut layer.frame.x, 1.0, "");
        num(ui, "Y", &mut layer.frame.y, 1.0, "");
        if !layer.is_artboard() {
            let mut deg = layer.rotation;
            if num(ui, "Rotate", &mut deg, 1.0, "°") {
                layer.rotation = deg.rem_euclid(360.0);
            }
        }
    });

    section(ui, "Size");
    ui.horizontal(|ui| {
        let (w0, h0) = (layer.frame.width, layer.frame.height);
        let (mut w, mut h) = (w0, h0);
        let mut changed = num(ui, "W", &mut w, 1.0, "");
        let lock_tip = if app.lock_aspect { "Unlock aspect ratio" } else { "Lock aspect ratio" };
        ui.vertical(|ui| {
            if icons::small_button(ui, if app.lock_aspect { Icon::Lock } else { Icon::Minus }, lock_tip).clicked() {
                app.lock_aspect = !app.lock_aspect;
            }
        });
        if app.lock_aspect && changed && w0 > 0.0 {
            h = h0 * w / w0;
        }
        if num(ui, "H", &mut h, 1.0, "") {
            changed = true;
            if app.lock_aspect && h0 > 0.0 {
                w = w0 * h / h0;
            }
        }
        if changed {
            // Reframe so groups scale their children, keeping the top-left fixed.
            layer.reframe(Rect::new(0.0, 0.0, w.max(0.0), h.max(0.0)));
        }
    });
    if !layer.is_artboard() {
        ui.horizontal(|ui| {
            if icons::button(ui, Icon::FlipH, layer.flip_horizontal, "Flip Horizontal").clicked() {
                layer.flip_horizontal = !layer.flip_horizontal;
            }
            if icons::button(ui, Icon::FlipV, layer.flip_vertical, "Flip Vertical").clicked() {
                layer.flip_vertical = !layer.flip_vertical;
            }
        });
    }

    match &mut layer.kind {
        LayerKind::Rectangle(rect) => {
            section(ui, "Corners");
            let mut independent = rect.corner_radii.is_some();
            if ui.checkbox(&mut independent, "Independent corners").changed() {
                rect.corner_radii = independent.then_some([rect.corner_radius; 4]);
            }
            ui.horizontal(|ui| match &mut rect.corner_radii {
                Some(radii) => {
                    for (r, label) in radii.iter_mut().zip(["TL", "TR", "BR", "BL"]) {
                        num(ui, label, r, 0.5, "");
                        *r = r.max(0.0);
                    }
                }
                None => {
                    num(ui, "Radius", &mut rect.corner_radius, 0.5, "");
                    rect.corner_radius = rect.corner_radius.max(0.0);
                }
            });
        }
        LayerKind::Polygon(poly) => {
            section(ui, "Polygon");
            ui.horizontal(|ui| {
                let mut sides = poly.sides as f64;
                if num(ui, "Sides", &mut sides, 0.1, "") {
                    poly.sides = sides.round().clamp(3.0, 100.0) as u32;
                }
            });
        }
        LayerKind::Star(star) => {
            section(ui, "Star");
            ui.horizontal(|ui| {
                let mut points = star.points as f64;
                if num(ui, "Points", &mut points, 0.1, "") {
                    star.points = points.round().clamp(3.0, 100.0) as u32;
                }
                let mut pct = star.inner_ratio * 100.0;
                if num(ui, "Radius", &mut pct, 0.5, "%") {
                    star.inner_ratio = (pct / 100.0).clamp(0.01, 1.0);
                }
            });
        }
        _ => {}
    }

    if let LayerKind::Artboard(ab) = &mut layer.kind {
        section(ui, "Artboard");
        let mut has_bg = ab.background.is_some();
        ui.horizontal(|ui| {
            if ui.checkbox(&mut has_bg, "Background").changed() {
                ab.background = has_bg.then_some(Color::WHITE);
            }
            if let Some(bg) = &mut ab.background {
                color_button(ui, bg);
            }
        });
        ui.checkbox(&mut ab.clip_content, "Clip content");
    }

    section(ui, "Appearance");
    ui.horizontal(|ui| {
        let mut pct = layer.opacity * 100.0;
        ui.label("Opacity");
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").max_decimals(0)).changed() {
            layer.opacity = pct / 100.0;
        }
    });

    if !layer.is_artboard() && !layer.is_group() {
        style_sections(ui, layer);
    }
}

fn style_sections(ui: &mut Ui, layer: &mut Layer) {
    let closed = layer.is_closed();
    if closed {
        list_header(ui, "Fills", || layer.style.fills.push(Fill::solid(Color::DEFAULT_FILL)));
        let mut remove = None;
        for (i, fill) in layer.style.fills.iter_mut().enumerate().rev() {
            ui.push_id(("fill", i), |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut fill.enabled, "");
                    color_button(ui, &mut fill.color);
                    hex_field(ui, &mut fill.color);
                    alpha_field(ui, &mut fill.color);
                    if icons::small_button(ui, Icon::Minus, "Remove fill").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            layer.style.fills.remove(i);
        }
        if let LayerKind::Path(p) = &mut layer.kind
            && p.subpaths.len() > 1
        {
            ui.horizontal(|ui| {
                ui.label("Fill rule");
                egui::ComboBox::from_id_salt("fill_rule")
                    .selected_text(match p.fill_rule {
                        FillRule::NonZero => "Non-Zero",
                        FillRule::EvenOdd => "Even-Odd",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.fill_rule, FillRule::NonZero, "Non-Zero");
                        ui.selectable_value(&mut p.fill_rule, FillRule::EvenOdd, "Even-Odd");
                    });
            });
        }
    }

    list_header(ui, "Borders", || layer.style.borders.push(Border::solid(Color::DEFAULT_BORDER, 1.0)));
    let mut remove = None;
    for (i, border) in layer.style.borders.iter_mut().enumerate().rev() {
        ui.push_id(("border", i), |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut border.enabled, "");
                color_button(ui, &mut border.color);
                hex_field(ui, &mut border.color);
                ui.add(DragValue::new(&mut border.thickness).speed(0.1).range(0.0..=1000.0).max_decimals(2));
                if icons::small_button(ui, Icon::Minus, "Remove border").clicked() {
                    remove = Some(i);
                }
            });
            if closed {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    egui::ComboBox::from_id_salt("position")
                        .selected_text(border.position.label())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for pos in BorderPosition::ALL {
                                ui.selectable_value(&mut border.position, pos, pos.label());
                            }
                        });
                });
            }
        });
    }
    if let Some(i) = remove {
        layer.style.borders.remove(i);
    }
}

fn list_header(ui: &mut Ui, title: &str, add: impl FnOnce()) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title.to_uppercase()).small().strong().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small_button(ui, Icon::Plus, &format!("Add {}", title.trim_end_matches('s').to_lowercase())).clicked() {
                add();
            }
        });
    });
}

fn color_button(ui: &mut Ui, color: &mut Color) {
    let mut rgba = color.to_rgba8();
    if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
        *color = Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    }
}

/// Hex text field. The edit buffer lives in egui memory while focused so partial input isn't
/// clobbered by re-formatting every frame.
fn hex_field(ui: &mut Ui, color: &mut Color) {
    let id = ui.id().with("hex");
    let mut buf: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| color.with_alpha(1.0).to_hex());
    let resp = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(58.0).char_limit(7).font(egui::TextStyle::Monospace));
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if (resp.lost_focus() || resp.changed())
        && let Some(c) = Color::from_hex(&buf).filter(|_| buf.trim_start_matches('#').len() == 6)
    {
        *color = c.with_alpha(color.a);
    }
}

fn alpha_field(ui: &mut Ui, color: &mut Color) {
    let mut pct = (color.a * 100.0).round();
    if ui.add(DragValue::new(&mut pct).range(0.0..=100.0).suffix("%").speed(1.0)).changed() {
        color.a = pct / 100.0;
    }
}

fn multi_selection(app: &mut ScrawlApp, ui: &mut Ui) {
    ui.add_space(4.0);
    ui.strong(format!("{} layers selected", app.selection.len()));
    section(ui, "Appearance");
    let page = app.page();
    let first = app.selection.iter().find_map(|&id| page.find(id)).map_or(1.0, |l| l.opacity);
    let mut pct = first * 100.0;
    ui.horizontal(|ui| {
        ui.label("Opacity");
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").max_decimals(0)).changed() {
            let sel = app.selection.clone();
            let page = app.page_mut();
            for id in sel {
                if let Some(l) = page.find_mut(id) {
                    l.opacity = pct / 100.0;
                }
            }
        }
    });
}

fn artboard_presets(app: &mut ScrawlApp, ui: &mut Ui) {
    ui.add_space(4.0);
    ui.weak("Drag on the canvas to draw an artboard, or pick a preset:");
    for (group, presets) in ARTBOARD_PRESETS {
        egui::CollapsingHeader::new(*group).id_salt(Id::new(("presets", *group))).default_open(*group == "Apple").show(ui, |ui| {
            for &(name, w, h) in *presets {
                let resp = ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::Button::new(egui::Atoms::new((name, egui::Atom::grow(), RichText::new(format!("{w:.0}×{h:.0}")).weak()))).frame_when_inactive(false),
                );
                if resp.clicked() {
                    app.insert_artboard_preset(name, w, h);
                }
            }
        });
    }
}

fn points_section(app: &mut ScrawlApp, ui: &mut Ui) {
    let Some(edit) = app.canvas.edit.as_ref() else { return };
    let (id, selected) = (edit.id, edit.selected.clone());
    let page = app.page();
    let shape = page.find(id).and_then(Layer::editable_path);
    let origin = page.parent_transform(id).map_or(Point::ORIGIN, |t| t * Point::ORIGIN);

    ui.horizontal(|ui| {
        ui.strong("Editing points");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Done").clicked() {
                app.exit_vector_edit();
            }
        });
    });
    ui.weak("Click a segment to add a point. Double-click a point to toggle curve. Keys 1–4 set the point type.");
    if selected.is_empty() {
        ui.add_space(8.0);
        ui.weak("No points selected");
        return;
    }

    section(ui, "Point");
    if let Some(b) = crate::vector::selected_points_page(app) {
        ui.horizontal(|ui| {
            let (mut x, mut y) = (b.x0 - origin.x, b.y0 - origin.y);
            let cx = num(ui, "X", &mut x, 1.0, "");
            let cy = num(ui, "Y", &mut y, 1.0, "");
            if cx || cy {
                crate::vector::move_selected_points_to(app, Point::new(x + origin.x, y + origin.y));
            }
        });
    }

    section(ui, "Point Type");
    let current: Option<PointMode> = shape.as_ref().and_then(|s| {
        let modes: Vec<_> = selected.iter().filter_map(|&r| s.point(r).map(|p| p.mode)).collect();
        modes.first().copied().filter(|m| modes.iter().all(|x| x == m))
    });
    ui.horizontal_wrapped(|ui| {
        for (i, mode) in [PointMode::Straight, PointMode::Mirrored, PointMode::Disconnected, PointMode::Asymmetric].into_iter().enumerate() {
            let resp = ui.selectable_label(current == Some(mode), mode.label()).on_hover_text(format!("Key {}", i + 1));
            if resp.clicked() {
                crate::vector::set_point_mode(app, mode);
            }
        }
    });
}
