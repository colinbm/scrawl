//! Small vector icons drawn with the egui painter, so they don't depend on font glyph coverage.

use egui::{Color32, Painter, Rect, Stroke, StrokeKind, Vec2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Select,
    Artboard,
    Rectangle,
    Oval,
    Line,
    Path,
    Pen,
    Triangle,
    Polygon,
    Star,
    Union,
    Subtract,
    Intersect,
    Difference,
    Group,
    Ungroup,
    Eye,
    EyeClosed,
    Lock,
    Plus,
    Minus,
    Forward,
    Backward,
    AlignLeft,
    AlignHCenter,
    AlignRight,
    AlignTop,
    AlignVCenter,
    AlignBottom,
    FlipH,
    FlipV,
}

/// Paints `icon` centered in `rect`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let u = s / 16.0; // design grid unit
    let st = Stroke::new(1.2_f32.max(u), color);
    let at = |x: f32, y: f32| c + vec2(x * u, y * u);
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_two_pos(at(x0, y0), at(x1, y1));
    match icon {
        Icon::Select => {
            let pts = vec![at(-4.0, -6.0), at(-4.0, 5.0), at(-1.0, 2.0), at(1.5, 6.5), at(3.0, 5.8), at(0.6, 1.4), at(4.5, 1.2)];
            p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Artboard => {
            p.rect_stroke(r(-5.0, -3.0, 5.0, 6.0), 0.0, st, StrokeKind::Middle);
            p.line_segment([at(-5.0, -5.5), at(-1.0, -5.5)], st);
        }
        Icon::Rectangle => {
            p.rect_stroke(r(-5.5, -5.5, 5.5, 5.5), 1.0, st, StrokeKind::Middle);
        }
        Icon::Oval => {
            p.circle_stroke(c, 5.5 * u, st);
        }
        Icon::Line => {
            p.line_segment([at(-5.5, 5.5), at(5.5, -5.5)], st);
        }
        Icon::Path => {
            p.add(egui::Shape::line(vec![at(-6.0, 4.0), at(-2.0, -5.0), at(2.0, 3.0), at(6.0, -4.0)], st));
        }
        Icon::Pen => {
            let nib = vec![at(0.0, -7.0), at(4.5, 1.0), at(0.0, 6.0), at(-4.5, 1.0)];
            p.add(egui::Shape::closed_line(nib, st));
            p.line_segment([at(0.0, -7.0), at(0.0, 1.5)], st);
            p.circle_filled(at(0.0, 1.5), 1.2 * u, color);
        }
        Icon::Triangle | Icon::Polygon | Icon::Star => {
            let (n, radii): (usize, &[f32]) = match icon {
                Icon::Triangle => (3, &[6.5]),
                Icon::Polygon => (5, &[6.5]),
                _ => (10, &[7.0, 3.0]),
            };
            let pts = (0..n)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
                    let r = radii[i % radii.len()];
                    at(r * a.cos(), r * a.sin() + 0.5)
                })
                .collect();
            p.add(egui::Shape::closed_line(pts, st));
        }
        Icon::Union | Icon::Subtract | Icon::Intersect | Icon::Difference => {
            let a = r(-6.5, -6.5, 2.5, 2.5);
            let b = r(-2.5, -2.5, 6.5, 6.5);
            let overlap = r(-2.5, -2.5, 2.5, 2.5);
            let faint = color.gamma_multiply(0.35);
            let (fa, fb, fo) = match icon {
                Icon::Union => (color, color, color),
                Icon::Subtract => (color, Color32::TRANSPARENT, Color32::TRANSPARENT),
                Icon::Intersect => (Color32::TRANSPARENT, Color32::TRANSPARENT, color),
                _ => (color, color, Color32::TRANSPARENT),
            };
            p.rect_filled(a, 0.0, fa);
            p.rect_filled(b, 0.0, fb);
            if fo == Color32::TRANSPARENT {
                p.rect_filled(overlap, 0.0, ui_bg(p));
            } else {
                p.rect_filled(overlap, 0.0, fo);
            }
            p.rect_stroke(a, 0.0, Stroke::new(1.0, faint), StrokeKind::Middle);
            p.rect_stroke(b, 0.0, Stroke::new(1.0, faint), StrokeKind::Middle);
        }
        Icon::Group => {
            p.rect_stroke(r(-6.0, -4.0, 6.0, 5.5), 1.0, st, StrokeKind::Middle);
            p.line_segment([at(-6.0, -4.0), at(-5.0, -6.0)], st);
            p.line_segment([at(-5.0, -6.0), at(-1.0, -6.0)], st);
            p.line_segment([at(-1.0, -6.0), at(0.0, -4.0)], st);
        }
        Icon::Ungroup => {
            p.rect_stroke(r(-6.0, -6.0, 1.0, 1.0), 0.0, st, StrokeKind::Middle);
            p.rect_stroke(r(-1.0, -1.0, 6.0, 6.0), 0.0, Stroke::new(st.width, color.gamma_multiply(0.6)), StrokeKind::Middle);
        }
        Icon::Eye | Icon::EyeClosed => {
            let mut pts = vec![];
            for i in 0..=16 {
                let t = i as f32 / 16.0 * std::f32::consts::PI;
                pts.push(at(-6.5 * t.cos(), -3.8 * t.sin()));
            }
            for i in 0..=16 {
                let t = i as f32 / 16.0 * std::f32::consts::PI;
                pts.push(at(6.5 * t.cos(), 3.8 * t.sin()));
            }
            p.add(egui::Shape::closed_line(pts, st));
            if icon == Icon::Eye {
                p.circle_filled(c, 1.8 * u, color);
            } else {
                p.line_segment([at(-6.0, 6.0), at(6.0, -6.0)], st);
            }
        }
        Icon::Lock => {
            p.rect_filled(r(-4.5, -1.0, 4.5, 6.0), 1.0, color);
            let mut pts = vec![];
            for i in 0..=12 {
                let t = i as f32 / 12.0 * std::f32::consts::PI;
                pts.push(at(-3.0 * t.cos(), -1.0 - 4.0 * t.sin()));
            }
            p.add(egui::Shape::line(pts, st));
        }
        Icon::Plus => {
            p.line_segment([at(-5.0, 0.0), at(5.0, 0.0)], st);
            p.line_segment([at(0.0, -5.0), at(0.0, 5.0)], st);
        }
        Icon::Minus => {
            p.line_segment([at(-5.0, 0.0), at(5.0, 0.0)], st);
        }
        Icon::Forward | Icon::Backward => {
            let (front, back) = if icon == Icon::Forward { ((-1.0, -1.0), (-5.0, -5.0)) } else { ((-5.0, -5.0), (-1.0, -1.0)) };
            let sq = |o: (f32, f32)| r(o.0, o.1, o.0 + 6.0, o.1 + 6.0);
            p.rect_stroke(sq(back), 0.0, Stroke::new(st.width, color.gamma_multiply(0.5)), StrokeKind::Middle);
            p.rect_filled(sq(front), 0.0, color);
        }
        Icon::AlignLeft | Icon::AlignHCenter | Icon::AlignRight => {
            let x = match icon {
                Icon::AlignLeft => -6.0,
                Icon::AlignHCenter => 0.0,
                _ => 6.0,
            };
            p.line_segment([at(x, -7.0), at(x, 7.0)], st);
            let bar = |w: f32, y: f32| {
                let x0 = match icon {
                    Icon::AlignLeft => -6.0,
                    Icon::AlignHCenter => -w / 2.0,
                    _ => 6.0 - w,
                };
                r(x0, y, x0 + w, y + 3.5)
            };
            p.rect_filled(bar(10.0, -5.0), 0.5, color.gamma_multiply(0.75));
            p.rect_filled(bar(6.0, 1.5), 0.5, color.gamma_multiply(0.75));
        }
        Icon::AlignTop | Icon::AlignVCenter | Icon::AlignBottom => {
            let y = match icon {
                Icon::AlignTop => -6.0,
                Icon::AlignVCenter => 0.0,
                _ => 6.0,
            };
            p.line_segment([at(-7.0, y), at(7.0, y)], st);
            let bar = |h: f32, x: f32| {
                let y0 = match icon {
                    Icon::AlignTop => -6.0,
                    Icon::AlignVCenter => -h / 2.0,
                    _ => 6.0 - h,
                };
                r(x, y0, x + 3.5, y0 + h)
            };
            p.rect_filled(bar(10.0, -5.0), 0.5, color.gamma_multiply(0.75));
            p.rect_filled(bar(6.0, 1.5), 0.5, color.gamma_multiply(0.75));
        }
        Icon::FlipH => {
            p.line_segment([at(0.0, -7.0), at(0.0, 7.0)], Stroke::new(st.width * 0.7, color.gamma_multiply(0.6)));
            p.add(egui::Shape::convex_polygon(vec![at(-2.0, -5.0), at(-2.0, 5.0), at(-7.0, 5.0)], color, Stroke::NONE));
            p.add(egui::Shape::closed_line(vec![at(2.0, -5.0), at(2.0, 5.0), at(7.0, 5.0)], st));
        }
        Icon::FlipV => {
            p.line_segment([at(-7.0, 0.0), at(7.0, 0.0)], Stroke::new(st.width * 0.7, color.gamma_multiply(0.6)));
            p.add(egui::Shape::convex_polygon(vec![at(-5.0, -2.0), at(5.0, -2.0), at(5.0, -7.0)], color, Stroke::NONE));
            p.add(egui::Shape::closed_line(vec![at(-5.0, 2.0), at(5.0, 2.0), at(5.0, 7.0)], st));
        }
    }
}

/// The panel background, used to "cut out" shapes in icons.
fn ui_bg(p: &Painter) -> Color32 {
    p.ctx().style_of(p.ctx().theme()).visuals.panel_fill
}

/// A square, frameless icon button with hover/selected highlight.
pub fn button(ui: &mut egui::Ui, icon: Icon, selected: bool, tooltip: &str) -> egui::Response {
    icon_button_sized(ui, icon, selected, tooltip, 26.0)
}

pub fn small_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    icon_button_sized(ui, icon, false, tooltip, 20.0)
}

fn icon_button_sized(ui: &mut egui::Ui, icon: Icon, selected: bool, tooltip: &str, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
    let visuals = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 4.0, visuals.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if selected { visuals.selection.stroke.color } else { visuals.text_color() };
    paint(ui.painter(), rect.shrink(size * 0.18), icon, color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// The icon used for a layer in the layer list.
pub fn for_layer(kind: &scrawl_core::LayerKind) -> Icon {
    use scrawl_core::LayerKind as K;
    match kind {
        K::Artboard(_) => Icon::Artboard,
        K::Group(_) => Icon::Group,
        K::Rectangle(_) => Icon::Rectangle,
        K::Oval => Icon::Oval,
        K::Polygon(p) if p.sides == 3 => Icon::Triangle,
        K::Polygon(_) => Icon::Polygon,
        K::Star(_) => Icon::Star,
        K::Path(_) if kind.type_name() == "Line" => Icon::Line,
        K::Path(_) => Icon::Path,
    }
}
