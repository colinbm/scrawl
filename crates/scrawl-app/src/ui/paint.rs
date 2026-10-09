//! The paint popover used by fills, borders and shadows: solid colors and gradients, a color
//! picker, color variables and document swatches.

use egui::color_picker::{Alpha, color_picker_hsva_2d};
use egui::ecolor::Hsva;
use egui::{Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use scrawl_core::{BlendMode, Color, FillType, Gradient, GradientKind, Swatch, SwatchId};

/// What a paint popover edits. Gradients and blend modes are optional (shadows have neither),
/// as is the color-variable link (artboard backgrounds have none).
pub struct PaintTarget<'a> {
    pub fill_type: Option<&'a mut FillType>,
    pub color: &'a mut Color,
    pub gradient: Option<&'a mut Gradient>,
    pub blend: Option<&'a mut BlendMode>,
    pub swatch: Option<&'a mut Option<SwatchId>>,
}

impl<'a> PaintTarget<'a> {
    /// A plain color with no gradient, blend mode or variable link.
    pub fn color(color: &'a mut Color) -> Self {
        Self { fill_type: None, color, gradient: None, blend: None, swatch: None }
    }
}

/// The document's reusable colors, offered in every paint popover.
pub struct Palette<'a> {
    /// Document swatches: plain colors.
    pub colors: &'a mut Vec<Color>,
    /// Color variables, which colors link to.
    pub variables: &'a mut Vec<Swatch>,
}

pub struct PaintResponse {
    /// Whether the popover is open and showing a gradient (for on-canvas handles).
    pub editing_gradient: bool,
}

pub fn to_color32(c: Color) -> Color32 {
    let [r, g, b, a] = c.to_rgba8();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Paints a checkerboard behind translucent colors.
fn checkerboard(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect, 0.0, Color32::from_gray(250));
    let s = 4.0;
    let mut y = rect.top();
    let mut row = 0;
    while y < rect.bottom() {
        let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { s };
        while x < rect.right() {
            let r = Rect::from_min_max(pos2(x, y), pos2((x + s).min(rect.right()), (y + s).min(rect.bottom())));
            p.rect_filled(r, 0.0, Color32::from_gray(205));
            x += 2.0 * s;
        }
        y += s;
        row += 1;
    }
}

/// Draws a gradient left to right in `rect`.
fn paint_gradient_bar(p: &egui::Painter, rect: Rect, g: &Gradient) {
    checkerboard(p, rect);
    let n = 48;
    let mut mesh = egui::Mesh::default();
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let c = to_color32(g.color_at(t as f64));
        let x = rect.left() + t * rect.width();
        mesh.colored_vertex(pos2(x, rect.top()), c);
        mesh.colored_vertex(pos2(x, rect.bottom()), c);
        if i > 0 {
            let k = (2 * i) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    p.add(mesh);
}

/// The swatch shown in inspector rows.
fn paint_swatch(p: &egui::Painter, rect: Rect, fill_type: FillType, color: Color, gradient: Option<&Gradient>) {
    match (fill_type, gradient) {
        (FillType::Gradient, Some(g)) => paint_gradient_bar(p, rect, g),
        _ => {
            checkerboard(p, rect);
            p.rect_filled(rect, 0.0, to_color32(color));
        }
    }
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, Color32::from_black_alpha(60)), StrokeKind::Inside);
}

/// A swatch button that opens the paint popover.
pub fn paint_button(ui: &mut Ui, id_salt: &str, target: PaintTarget<'_>, palette: &mut Palette<'_>) -> PaintResponse {
    let fill_type = target.fill_type.as_deref().copied().unwrap_or(FillType::Color);
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 20.0), Sense::click());
    let label = format!("{} paint", id_salt);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    paint_swatch(ui.painter(), rect, fill_type, *target.color, target.gradient.as_deref());
    let popup_id = ui.id().with(("paint_popup", id_salt));
    let mut editing_gradient = false;
    egui::Popup::from_toggle_button_response(&resp)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(232.0);
            editing_gradient = popover(ui, popup_id, target, palette);
        });
    PaintResponse { editing_gradient }
}

fn popover(ui: &mut Ui, id: Id, target: PaintTarget<'_>, palette: &mut Palette<'_>) -> bool {
    let PaintTarget { mut fill_type, color, mut gradient, blend, mut swatch } = target;
    let stop_id = id.with("stop");
    let mut selected_stop: usize = ui.data(|d| d.get_temp(stop_id)).unwrap_or(0);

    // Type selector.
    if let (Some(ft), Some(g)) = (fill_type.as_deref_mut(), gradient.as_deref_mut()) {
        ui.horizontal(|ui| {
            if ui.selectable_label(*ft == FillType::Color, "Solid").clicked() {
                *ft = FillType::Color;
            }
            for kind in GradientKind::ALL {
                if ui.selectable_label(*ft == FillType::Gradient && g.kind == kind, kind.label()).clicked() {
                    if *ft == FillType::Color {
                        // Start from the current color rather than an arbitrary default.
                        *g = Gradient { kind, ..Gradient::from_color(*color) };
                    }
                    *ft = FillType::Gradient;
                    g.kind = kind;
                }
            }
        });
        ui.separator();
    }

    let is_gradient = fill_type.as_deref() == Some(&FillType::Gradient) && gradient.is_some();
    // Only solid colors link to variables; gradient stops just take their value.
    let link = if is_gradient { None } else { swatch.as_deref_mut() };
    if let Some(link) = link
        && let Some(var) = link.and_then(|id| palette.variables.iter().find(|v| v.id == id))
    {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("◆ {}", var.name)).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Detach").on_hover_text("Stop following this color variable").clicked() {
                    *link = None;
                }
            });
        });
        ui.separator();
    }
    let editing: &mut Color = if is_gradient {
        let g = gradient.expect("checked");
        selected_stop = selected_stop.min(g.stops.len().saturating_sub(1));
        stops_editor(ui, id, g, &mut selected_stop);
        &mut g.stops[selected_stop].color
    } else {
        color
    };

    // Color picker.
    // The picker sizes itself from the slider width; fill the popover.
    ui.spacing_mut().slider_width = ui.available_width() - 8.0;
    let mut hsva = Hsva::from_srgba_unmultiplied(editing.to_rgba8());
    if color_picker_hsva_2d(ui, &mut hsva, Alpha::OnlyBlend) {
        let [r, g, b, a] = hsva.to_srgba_unmultiplied();
        *editing = Color::from_rgba8(r, g, b, a);
    }
    ui.horizontal(|ui| {
        ui.label("Hex");
        crate::ui::inspector::hex_field(ui, editing);
        ui.label("Alpha");
        crate::ui::inspector::alpha_field(ui, editing);
    });

    if let Some(blend) = blend {
        ui.horizontal(|ui| {
            ui.label("Blend");
            blend_combo(ui, id.with("blend"), blend);
        });
    }

    // Color variables.
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("COLOR VARIABLES").small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::icons::small_button(ui, crate::ui::icons::Icon::Plus, "Create color variable").clicked() {
                let id = Swatch::add_to(palette.variables, *editing);
                if !is_gradient && let Some(link) = swatch.as_deref_mut() {
                    *link = Some(id);
                }
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for v in palette.variables.iter() {
            let linked = !is_gradient && swatch.as_deref().is_some_and(|l| *l == Some(v.id));
            let resp = swatch_chip(ui, v.color, linked).on_hover_text(&v.name);
            if resp.clicked() {
                *editing = v.color;
                if !is_gradient && let Some(link) = swatch.as_deref_mut() {
                    *link = Some(v.id);
                }
            }
        }
        if palette.variables.is_empty() {
            ui.weak("None yet");
        }
    });

    // Document swatches.
    let swatches = &mut *palette.colors;
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("DOCUMENT COLORS").small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::icons::small_button(ui, crate::ui::icons::Icon::Plus, "Add current color").clicked()
                && !swatches.contains(editing)
            {
                swatches.push(*editing);
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let mut remove = None;
        for (i, &c) in swatches.iter().enumerate() {
            let resp = swatch_chip(ui, c, false);
            if resp.clicked() {
                *editing = c;
            }
            resp.context_menu(|ui| {
                if ui.button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            swatches.remove(i);
        }
    });

    ui.data_mut(|d| d.insert_temp(stop_id, selected_stop));
    is_gradient
}

/// A small clickable color square; `selected` rings it in the accent color.
pub fn swatch_chip(ui: &mut Ui, c: Color, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    checkerboard(ui.painter(), rect);
    ui.painter().rect_filled(rect, 2.0, to_color32(c));
    let stroke = if selected { Stroke::new(2.0, crate::style::ACCENT) } else { Stroke::new(1.0, Color32::from_black_alpha(60)) };
    ui.painter().rect_stroke(rect, 2.0, stroke, StrokeKind::Inside);
    resp
}

/// The gradient bar with draggable stops. Click empty space on the bar to add a stop.
fn stops_editor(ui: &mut Ui, id: Id, g: &mut Gradient, selected: &mut usize) {
    let (bar, bar_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click());
    paint_gradient_bar(ui.painter(), bar, g);
    let (track, _) = ui.allocate_exact_size(vec2(bar.width(), 12.0), Sense::hover());
    let x_of = |t: f64| bar.left() + t as f32 * bar.width();
    let t_of = |pos: Pos2| ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0) as f64;

    if bar_resp.clicked()
        && let Some(pos) = bar_resp.interact_pointer_pos()
    {
        *selected = g.add_stop(t_of(pos));
    }
    for i in 0..g.stops.len() {
        let center = pos2(x_of(g.stops[i].position), track.center().y);
        let r = Rect::from_center_size(center, vec2(10.0, 12.0));
        let resp = ui.interact(r, id.with(("stop", i)), Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            *selected = i;
        }
        if resp.dragged()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            g.stops[i].position = t_of(pos);
            *selected = g.sort_stops(i);
        }
        let p = ui.painter();
        let pts = vec![center + vec2(0.0, -6.0), center + vec2(5.0, 0.0), center + vec2(5.0, 6.0), center + vec2(-5.0, 6.0), center + vec2(-5.0, 0.0)];
        let stroke_color = if i == *selected { crate::style::ACCENT } else { Color32::from_gray(120) };
        p.add(egui::Shape::convex_polygon(pts, to_color32(g.stops[i].color.with_alpha(1.0)), Stroke::new(if i == *selected { 2.0 } else { 1.0 }, stroke_color)));
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Stop {:.0}%", g.stops.get(*selected).map_or(0.0, |s| s.position * 100.0))).small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(g.stops.len() > 2, egui::Button::new("Remove Stop").small()).clicked() {
                g.stops.remove(*selected);
                *selected = selected.saturating_sub(1);
            }
            if ui.add(egui::Button::new("Flip").small()).clicked() {
                for s in &mut g.stops {
                    s.position = 1.0 - s.position;
                }
                g.stops.reverse();
                *selected = g.stops.len() - 1 - *selected;
            }
        });
    });
}

pub fn blend_combo(ui: &mut Ui, id: impl egui::AsIdSalt, blend: &mut BlendMode) {
    egui::ComboBox::from_id_salt(id).selected_text(blend.label()).width(110.0).show_ui(ui, |ui| {
        for mode in BlendMode::ALL {
            ui.selectable_value(blend, mode, mode.label());
        }
    });
}
