//! Rulers along the canvas edges, ruler guides, and the square and layout grids drawn over
//! artboards.
//!
//! Rulers measure from the artboard containing the selection (or the page origin). Clicking a
//! ruler adds a guide to that artboard (or the page); dragging a guide's marker moves it, and
//! dragging it off the ruler removes it.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect as ERect, Sense, Stroke, vec2};
use scrawl_core::grid::Guide;
use scrawl_core::kurbo::Point;
use scrawl_core::snap::{Axis, Targets};
use scrawl_core::{LayerId, LayerKind, Page};

use crate::app::ScrawlApp;
use crate::style::ACCENT;

/// Ruler thickness in screen points.
pub const RULER: f32 = 18.0;
const GUIDE_COLOR: Color32 = Color32::from_rgb(0x00, 0xB2, 0xD6);
/// Page guides have no natural extent; this stands in for "everywhere".
const FAR: f64 = 1.0e7;
/// How close (screen points) the pointer must be to a guide marker to grab it.
const GRAB: f32 = 4.0;

/// A guide being dragged on a ruler.
#[derive(Clone, Copy, Debug)]
pub struct GuideDrag {
    container: Option<LayerId>,
    index: usize,
}

/// A guide placed in page space.
pub struct Placed {
    pub axis: Axis,
    pub pos: f64,
    /// Extent on the other axis.
    pub span: (f64, f64),
}

/// The artboard rulers measure from: the root artboard holding (or being) the first selected
/// layer.
pub fn origin_artboard(app: &ScrawlApp) -> Option<LayerId> {
    let page = app.page();
    let &id = app.selection.first()?;
    let root = page.ancestors(id)?.first().copied().unwrap_or(id);
    page.find(root).filter(|l| l.is_artboard()).map(|l| l.id)
}

fn container_origin(page: &Page, container: Option<LayerId>) -> Point {
    container.and_then(|id| page.find(id)).map_or(Point::ORIGIN, |l| l.frame.origin())
}

fn guides(page: &Page, container: Option<LayerId>) -> Option<&Vec<Guide>> {
    match container {
        None => Some(&page.guides),
        Some(id) => match &page.find(id)?.kind {
            LayerKind::Artboard(a) => Some(&a.guides),
            _ => None,
        },
    }
}

fn guides_mut(page: &mut Page, container: Option<LayerId>) -> Option<&mut Vec<Guide>> {
    match container {
        None => Some(&mut page.guides),
        Some(id) => match &mut page.find_mut(id)?.kind {
            LayerKind::Artboard(a) => Some(&mut a.guides),
            _ => None,
        },
    }
}

/// Every guide on the page, in page space: page guides span everything, artboard guides their
/// artboard.
pub fn placed_guides(page: &Page) -> Vec<Placed> {
    let mut out: Vec<Placed> = page.guides.iter().map(|g| Placed { axis: g.axis, pos: g.pos, span: (-FAR, FAR) }).collect();
    for l in page.layers.iter().filter(|l| l.visible) {
        let LayerKind::Artboard(a) = &l.kind else { continue };
        let r = l.frame.to_rect();
        for g in &a.guides {
            out.push(match g.axis {
                Axis::X => Placed { axis: Axis::X, pos: r.x0 + g.pos, span: (r.y0, r.y1) },
                Axis::Y => Placed { axis: Axis::Y, pos: r.y0 + g.pos, span: (r.x0, r.x1) },
            });
        }
    }
    out
}

/// Adds guides and visible grid lines to a drag's snap targets. `scope` is the artboard being
/// snapped within, or `None` for the page root.
pub fn add_targets(app: &ScrawlApp, scope: Option<LayerId>, t: &mut Targets) {
    let page = app.page();
    if app.show_rulers {
        let span = |axis| match scope.and_then(|id| page.find(id)) {
            Some(l) => {
                let r = l.frame.to_rect();
                if axis == Axis::X { (r.y0, r.y1) } else { (r.x0, r.x1) }
            }
            None => (-FAR, FAR),
        };
        if let Some(gs) = guides(page, scope) {
            let o = container_origin(page, scope);
            for g in gs {
                let pos = if g.axis == Axis::X { o.x + g.pos } else { o.y + g.pos };
                t.add_line(g.axis, pos, span(g.axis));
            }
        }
    }
    let Some(l) = scope.and_then(|id| page.find(id)) else { return };
    let LayerKind::Artboard(a) = &l.kind else { return };
    let r = l.frame.to_rect();
    if app.show_grid && a.grid.size > 0.0 && (r.width() + r.height()) / a.grid.size < 2000.0 {
        a.grid.lines(r.width()).for_each(|(x, _)| t.add_line(Axis::X, r.x0 + x, (r.y0, r.y1)));
        a.grid.lines(r.height()).for_each(|(y, _)| t.add_line(Axis::Y, r.y0 + y, (r.x0, r.x1)));
    }
    if app.show_layout {
        if let Some(c) = &a.layout.columns {
            for (x0, x1) in c.spans(r.width()) {
                t.add_line(Axis::X, r.x0 + x0, (r.y0, r.y1));
                t.add_line(Axis::X, r.x0 + x1, (r.y0, r.y1));
            }
        }
        if let Some(rows) = &a.layout.rows {
            for (y0, y1) in rows.spans(r.height()) {
                t.add_line(Axis::Y, r.y0 + y0, (r.x0, r.x1));
                t.add_line(Axis::Y, r.y0 + y1, (r.x0, r.x1));
            }
        }
    }
}

/// Screen strips for the horizontal (top) and vertical (left) rulers.
fn strips(rect: ERect) -> (ERect, ERect) {
    let top = ERect::from_min_max(rect.min + vec2(RULER, 0.0), Pos2::new(rect.max.x, rect.min.y + RULER));
    let left = ERect::from_min_max(rect.min + vec2(0.0, RULER), Pos2::new(rect.min.x + RULER, rect.max.y));
    (top, left)
}

/// Page coordinate → screen coordinate along one axis.
fn to_screen(app: &ScrawlApp, rect: ERect, axis: Axis, v: f64) -> f32 {
    let vp = app.viewport();
    let s = vp.transform() * Point::new(v, v);
    match axis {
        Axis::X => rect.min.x + s.x as f32,
        Axis::Y => rect.min.y + s.y as f32,
    }
}

fn to_page(app: &ScrawlApp, rect: ERect, axis: Axis, s: f32) -> f64 {
    let p = app.viewport().to_page(rect, Pos2::new(s, s));
    match axis {
        Axis::X => p.x,
        Axis::Y => p.y,
    }
}

fn along(p: Pos2, axis: Axis) -> f32 {
    match axis {
        Axis::X => p.x,
        Axis::Y => p.y,
    }
}

/// Handles clicks and drags on the rulers. Call after the canvas has allocated its rect, so the
/// rulers sit on top of it.
pub fn interact(app: &mut ScrawlApp, ui: &mut egui::Ui, rect: ERect) {
    if !app.show_rulers {
        app.canvas.guide_drag = None;
        return;
    }
    let (top, left) = strips(rect);
    for (strip, axis, id) in [(top, Axis::X, "ruler_x"), (left, Axis::Y, "ruler_y")] {
        let response = ui.interact(strip, ui.id().with(id), Sense::click_and_drag());
        let container = origin_artboard(app);
        let origin = container_origin(app.page(), container);
        let o = if axis == Axis::X { origin.x } else { origin.y };
        let near = |app: &ScrawlApp, s: f32| {
            guides(app.page(), container)?
                .iter()
                .enumerate()
                .filter(|(_, g)| g.axis == axis)
                .map(|(i, g)| (i, (to_screen(app, rect, axis, o + g.pos) - s).abs()))
                .filter(|&(_, d)| d <= GRAB)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        let add = |app: &mut ScrawlApp, s: f32| {
            let pos = (to_page(app, rect, axis, s) - o).round();
            let list = guides_mut(app.page_mut(), container)?;
            list.push(Guide { axis, pos });
            Some(list.len() - 1)
        };

        if response.hovered() {
            let cursor = if axis == Axis::X { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
            ui.ctx().set_cursor_icon(cursor);
        }
        if response.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            let index = near(app, along(p, axis)).or_else(|| add(app, along(p, axis)));
            app.canvas.guide_drag = index.map(|index| GuideDrag { container, index });
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
            && near(app, along(p, axis)).is_none()
        {
            add(app, along(p, axis));
        }
        if response.dragged()
            && let (Some(d), Some(p)) = (app.canvas.guide_drag, response.interact_pointer_pos())
        {
            let pos = (to_page(app, rect, axis, along(p, axis)) - o).round();
            if let Some(g) = guides_mut(app.page_mut(), d.container).and_then(|l| l.get_mut(d.index)) {
                g.pos = pos;
            }
        }
        if response.drag_stopped()
            && let Some(d) = app.canvas.guide_drag.take()
        {
            // Dropped well away from the ruler: remove it.
            let off = response.interact_pointer_pos().is_some_and(|p| !strip.expand(12.0).contains(p));
            if off && let Some(list) = guides_mut(app.page_mut(), d.container) {
                list.remove(d.index);
            }
        }
    }
}

/// Major tick spacing (page units) giving at least `min_px` screen points between labels.
fn tick_step(zoom: f64, min_px: f64) -> f64 {
    let mut step = 1.0;
    loop {
        for m in [1.0, 2.0, 5.0] {
            if step * m * zoom >= min_px {
                return step * m;
            }
        }
        step *= 10.0;
    }
}

/// Paints guides across the canvas, then the rulers.
pub fn paint(app: &ScrawlApp, p: &Painter, rect: ERect) {
    if !app.show_rulers {
        return;
    }
    let page = app.page();
    let to_screen_pt = crate::canvas::screen_transform(app, rect);
    for g in placed_guides(page) {
        let (a, b) = match g.axis {
            Axis::X => (Point::new(g.pos, g.span.0.max(-FAR)), Point::new(g.pos, g.span.1)),
            Axis::Y => (Point::new(g.span.0, g.pos), Point::new(g.span.1, g.pos)),
        };
        let clamp = |q: Pos2| Pos2::new(q.x.clamp(rect.left(), rect.right()), q.y.clamp(rect.top(), rect.bottom()));
        let (a, b) = (clamp(crate::canvas::to_pos(to_screen_pt * a)), clamp(crate::canvas::to_pos(to_screen_pt * b)));
        p.line_segment([a, b], Stroke::new(1.0, GUIDE_COLOR));
    }

    let visuals = p.ctx().style_of(p.ctx().theme()).visuals.clone();
    let bg = visuals.panel_fill;
    let ink = visuals.weak_text_color();
    let edge = visuals.widgets.noninteractive.bg_stroke;
    let (top, left) = strips(rect);
    let container = origin_artboard(app);
    let origin = container_origin(page, container);
    let zoom = app.viewport().zoom;
    let step = tick_step(zoom, 60.0);
    let minor = if step * zoom / 10.0 >= 5.0 { step / 10.0 } else { step / 5.0 };
    let sel = page.selection_bounds(&app.selection);

    for (strip, axis) in [(top, Axis::X), (left, Axis::Y)] {
        p.rect_filled(strip, 0.0, bg);
        let p = p.with_clip_rect(strip);
        let o = if axis == Axis::X { origin.x } else { origin.y };
        // Selection extent.
        if let Some(s) = sel {
            let (a, b) = if axis == Axis::X { (s.x0, s.x1) } else { (s.y0, s.y1) };
            let (a, b) = (to_screen(app, rect, axis, a), to_screen(app, rect, axis, b));
            let band = match axis {
                Axis::X => ERect::from_x_y_ranges(a..=b, strip.y_range()),
                Axis::Y => ERect::from_x_y_ranges(strip.x_range(), a..=b),
            };
            p.rect_filled(band, 0.0, ACCENT.gamma_multiply(0.25));
        }
        // Ticks, labelled relative to the origin.
        let (s0, s1) = match axis {
            Axis::X => (strip.left(), strip.right()),
            Axis::Y => (strip.top(), strip.bottom()),
        };
        let (v0, v1) = (to_page(app, rect, axis, s0) - o, to_page(app, rect, axis, s1) - o);
        let mut v = (v0 / minor).floor() * minor;
        while v <= v1 {
            let s = to_screen(app, rect, axis, o + v);
            let major = ((v / step).round() * step - v).abs() < minor / 2.0;
            let len = if major { RULER * 0.6 } else { RULER * 0.25 };
            let (a, b) = match axis {
                Axis::X => (Pos2::new(s, strip.bottom() - len), Pos2::new(s, strip.bottom())),
                Axis::Y => (Pos2::new(strip.right() - len, s), Pos2::new(strip.right(), s)),
            };
            p.line_segment([a, b], Stroke::new(1.0, ink.gamma_multiply(0.6)));
            if major {
                let label = format!("{}", v.round() as i64);
                let font = FontId::proportional(9.0);
                match axis {
                    Axis::X => {
                        p.text(Pos2::new(s + 3.0, strip.top() + 1.0), Align2::LEFT_TOP, label, font, ink);
                    }
                    Axis::Y => {
                        // Rotated to read bottom-to-top, running up from the tick.
                        let galley = p.layout_no_wrap(label, font, ink);
                        let pos = Pos2::new(strip.left() + 2.0, s - 3.0);
                        p.add(egui::epaint::TextShape::new(pos, galley, ink).with_angle(-std::f32::consts::FRAC_PI_2));
                    }
                }
            }
            v += minor;
        }
        // Guide markers for the origin container.
        if let Some(gs) = guides(page, container) {
            for g in gs.iter().filter(|g| g.axis == axis) {
                let s = to_screen(app, rect, axis, o + g.pos);
                let (a, b) = match axis {
                    Axis::X => (Pos2::new(s, strip.top()), Pos2::new(s, strip.bottom())),
                    Axis::Y => (Pos2::new(strip.left(), s), Pos2::new(strip.right(), s)),
                };
                p.line_segment([a, b], Stroke::new(2.0, GUIDE_COLOR));
            }
        }
        let line = match axis {
            Axis::X => [strip.left_bottom(), strip.right_bottom()],
            Axis::Y => [strip.right_top(), strip.right_bottom()],
        };
        p.line_segment(line, edge);
    }
    // The corner square.
    let corner = ERect::from_min_size(rect.min, vec2(RULER, RULER));
    p.rect_filled(corner, 0.0, bg);
    p.line_segment([corner.right_top(), corner.right_bottom()], edge);
    p.line_segment([corner.left_bottom(), corner.right_bottom()], edge);
}

/// Paints square grids and layout grids over every visible artboard.
pub fn paint_grids(app: &ScrawlApp, p: &Painter, rect: ERect) {
    if !app.show_grid && !app.show_layout {
        return;
    }
    let t = crate::canvas::screen_transform(app, rect);
    let zoom = app.viewport().zoom;
    for l in app.page().layers.iter().filter(|l| l.visible) {
        let LayerKind::Artboard(a) = &l.kind else { continue };
        let r = l.frame.to_rect();
        let screen = ERect::from_two_pos(crate::canvas::to_pos(t * r.origin()), crate::canvas::to_pos(t * Point::new(r.x1, r.y1)));
        let clip = screen.intersect(rect);
        if !clip.is_positive() {
            continue;
        }
        let p = p.with_clip_rect(clip);
        let sx = |x: f64| crate::canvas::to_pos(t * Point::new(r.x0 + x, r.y0)).x;
        let sy = |y: f64| crate::canvas::to_pos(t * Point::new(r.x0, r.y0 + y)).y;
        if app.show_grid && a.grid.size * zoom >= 4.0 {
            let thin = Stroke::new(1.0, Color32::from_black_alpha(18));
            let thick = Stroke::new(1.0, Color32::from_black_alpha(45));
            for (x, heavy) in a.grid.lines(r.width()) {
                p.vline(sx(x), screen.y_range(), if heavy { thick } else { thin });
            }
            for (y, heavy) in a.grid.lines(r.height()) {
                p.hline(screen.x_range(), sy(y), if heavy { thick } else { thin });
            }
        }
        if app.show_layout {
            let column = Color32::from_rgba_unmultiplied(255, 60, 60, 26);
            let row = Color32::from_rgba_unmultiplied(255, 60, 60, 18);
            if let Some(c) = &a.layout.columns {
                for (x0, x1) in c.spans(r.width()) {
                    p.rect_filled(ERect::from_x_y_ranges(sx(x0)..=sx(x1), screen.y_range()), 0.0, column);
                }
            }
            if let Some(rows) = &a.layout.rows {
                for (y0, y1) in rows.spans(r.height()) {
                    p.rect_filled(ERect::from_x_y_ranges(screen.x_range(), sy(y0)..=sy(y1)), 0.0, row);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_steps_follow_1_2_5() {
        assert_eq!(tick_step(1.0, 60.0), 100.0);
        assert_eq!(tick_step(2.0, 60.0), 50.0);
        assert_eq!(tick_step(10.0, 60.0), 10.0);
        assert_eq!(tick_step(0.1, 60.0), 1000.0);
        assert_eq!(tick_step(0.25, 60.0), 500.0);
    }
}
