//! Smart guides on the canvas: gathering what a drag can snap to, and painting the guide lines,
//! equal-spacing markers and ⌥ distance measurements.

use egui::{Color32, Painter, Pos2, Stroke, vec2};
use scrawl_core::kurbo::{Affine, Point, Rect};
use scrawl_core::snap::{Axis, Feedback, Measure, Targets};
use scrawl_core::{LayerId, Page};

use crate::app::ScrawlApp;
use crate::canvas::to_pos;

/// Smart guide color.
pub const GUIDE: Color32 = Color32::from_rgb(0xFF, 0x3B, 0x6B);

/// Snap distance in screen points.
pub const THRESHOLD: f64 = 5.0;

/// The artboard whose contents a drag in `page` snaps against: the one containing the first
/// moving layer, or (for new layers) the one under `at`. `None` means the page root.
fn scope(page: &Page, moving: &[LayerId], at: Option<Point>) -> Option<LayerId> {
    match moving.first() {
        Some(&id) => page.ancestors(id)?.first().copied().filter(|&a| page.find(a).is_some_and(|l| l.is_artboard())),
        None => page.artboard_at(at?),
    }
}

/// Everything a drag of `moving` (or a new layer drawn at `at`) can snap to.
pub fn targets(app: &ScrawlApp, moving: &[LayerId], at: Option<Point>) -> Targets {
    let page = app.page();
    let mut t = Targets::default();
    // Moving layers' ancestors and descendants never count.
    let mut skip: Vec<LayerId> = moving.to_vec();
    for &id in moving {
        skip.extend(page.ancestors(id).unwrap_or_default());
        if let Some(l) = page.find(id) {
            l.walk(&mut |d| skip.push(d.id));
        }
    }
    let candidates: Vec<LayerId> = match scope(page, moving, at) {
        Some(ab) => {
            if let Some(b) = page.world_bounds(ab) {
                t.add_edges(b);
            }
            let mut ids = vec![];
            if let Some(children) = page.find(ab).and_then(|l| l.children()) {
                for c in children {
                    c.walk(&mut |d| {
                        if d.visible {
                            ids.push(d.id)
                        }
                    });
                }
            }
            ids
        }
        None => page.layers.iter().filter(|l| l.visible).map(|l| l.id).collect(),
    };
    for id in candidates.into_iter().filter(|id| !skip.contains(id)) {
        if let Some(b) = page.world_bounds(id) {
            t.add_rect(b);
        }
    }
    t
}

/// Whether `t` maps local axes onto page axes (no rotation), so snapping in page space is exact.
pub fn axis_aligned(t: Affine) -> bool {
    let [_, b, c, _, _, _] = t.as_coeffs();
    b.abs() < 1e-9 && c.abs() < 1e-9
}

fn line(p: &Painter, a: Pos2, b: Pos2) {
    p.line_segment([a, b], Stroke::new(1.0, GUIDE));
}

/// A measurement line with end ticks and a centered label.
fn measurement(p: &Painter, a: Pos2, b: Pos2, value: f64) {
    line(p, a, b);
    let d = (b - a).normalized();
    let n = vec2(-d.y, d.x) * 3.5;
    line(p, a - n, a + n);
    line(p, b - n, b + n);
    label(p, a + (b - a) / 2.0, &format_len(value));
}

pub fn format_len(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 { format!("{v:.0}") } else { format!("{v:.1}") }
}

fn label(p: &Painter, center: Pos2, text: &str) {
    let galley = p.layout_no_wrap(text.to_owned(), egui::FontId::proportional(10.5), Color32::WHITE);
    let bg = egui::Rect::from_center_size(center, galley.size() + vec2(8.0, 3.0));
    p.rect_filled(bg, 3.0, GUIDE);
    p.galley(bg.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

/// Paints snap guide lines and equal-gap markers.
pub fn paint_feedback(p: &Painter, fb: &Feedback, to_screen: Affine) {
    let clip = p.clip_rect();
    for l in &fb.lines {
        let (a, b) = match l.axis {
            Axis::X => (Point::new(l.pos, l.from), Point::new(l.pos, l.to)),
            Axis::Y => (Point::new(l.from, l.pos), Point::new(l.to, l.pos)),
        };
        let (a, b) = (to_pos(to_screen * a), to_pos(to_screen * b));
        // Infinite spans (guides) are clipped to the canvas.
        let a = Pos2::new(a.x.clamp(clip.left(), clip.right()), a.y.clamp(clip.top(), clip.bottom()));
        let b = Pos2::new(b.x.clamp(clip.left(), clip.right()), b.y.clamp(clip.top(), clip.bottom()));
        line(p, a, b);
    }
    for g in &fb.gaps {
        for &(g0, g1) in &g.intervals {
            let (a, b) = match g.axis {
                Axis::X => (Point::new(g0, g.at), Point::new(g1, g.at)),
                Axis::Y => (Point::new(g.at, g0), Point::new(g.at, g1)),
            };
            measurement(p, to_pos(to_screen * a), to_pos(to_screen * b), g1 - g0);
        }
    }
}

/// Paints ⌥ distance measurements between the selection and `other` (both page-space bounds).
pub fn paint_measurements(p: &Painter, sel: Rect, other: Rect, to_screen: Affine) {
    let (measures, extensions) = scrawl_core::snap::measure(sel, other);
    for (a, b) in extensions {
        let pts = [to_pos(to_screen * a), to_pos(to_screen * b)];
        p.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, GUIDE), 3.0, 3.0));
    }
    // Outline what's being measured to.
    let r = egui::Rect::from_two_pos(to_pos(to_screen * other.origin()), to_pos(to_screen * Point::new(other.x1, other.y1)));
    p.rect_stroke(r, 0.0, Stroke::new(1.0, GUIDE), egui::StrokeKind::Middle);
    for Measure { a, b } in measures {
        measurement(p, to_pos(to_screen * a), to_pos(to_screen * b), (b - a).hypot());
    }
}
