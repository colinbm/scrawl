use egui::{Pos2, Rect as ERect, Vec2 as EVec2};
use scrawl_core::kurbo::{Affine, Point, Rect, Vec2};

pub const MIN_ZOOM: f64 = 0.02;
pub const MAX_ZOOM: f64 = 256.0;
/// Discrete steps used by zoom in/out shortcuts.
const ZOOM_STEPS: [f64; 17] =
    [0.02, 0.05, 0.1, 0.125, 0.25, 0.333, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 8.0, 16.0, 64.0, 256.0];

/// Maps page space to canvas-local screen points (origin at the canvas's top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Screen position of the page origin, relative to the canvas's top-left.
    pub pan: Vec2,
    pub zoom: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self { pan: Vec2::new(80.0, 80.0), zoom: 1.0 }
    }
}

impl Viewport {
    pub fn transform(&self) -> Affine {
        Affine::translate(self.pan) * Affine::scale(self.zoom)
    }

    #[cfg(test)]
    pub fn to_screen(&self, canvas: ERect, p: Point) -> Pos2 {
        let s = self.transform() * p;
        canvas.min + EVec2::new(s.x as f32, s.y as f32)
    }

    pub fn to_page(&self, canvas: ERect, pos: Pos2) -> Point {
        let local = pos - canvas.min;
        self.transform().inverse() * Point::new(local.x as f64, local.y as f64)
    }

    /// Zooms by `factor`, keeping the page point under `anchor` (screen) fixed.
    pub fn zoom_about(&mut self, canvas: ERect, anchor: Pos2, factor: f64) {
        self.set_zoom_about(canvas, anchor, self.zoom * factor);
    }

    pub fn set_zoom_about(&mut self, canvas: ERect, anchor: Pos2, zoom: f64) {
        let before = self.to_page(canvas, anchor);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_page(canvas, anchor);
        self.pan += (after - before) * self.zoom;
    }

    pub fn step_zoom(&mut self, canvas: ERect, zoom_in: bool) {
        let next = if zoom_in {
            ZOOM_STEPS.iter().copied().find(|&z| z > self.zoom * 1.001).unwrap_or(MAX_ZOOM)
        } else {
            ZOOM_STEPS.iter().rev().copied().find(|&z| z < self.zoom * 0.999).unwrap_or(MIN_ZOOM)
        };
        self.set_zoom_about(canvas, canvas.center(), next);
    }

    /// Fits `r` (page space) inside the canvas with some padding, never zooming past `max_zoom`.
    pub fn fit(&mut self, canvas: ERect, r: Rect, max_zoom: f64) {
        let pad = 60.0;
        let avail = Vec2::new((canvas.width() as f64 - 2.0 * pad).max(1.0), (canvas.height() as f64 - 2.0 * pad).max(1.0));
        let zoom = (avail.x / r.width().max(1.0)).min(avail.y / r.height().max(1.0)).clamp(MIN_ZOOM, max_zoom);
        self.zoom = zoom;
        let c = r.center().to_vec2() * zoom;
        self.pan = Vec2::new(canvas.width() as f64 / 2.0, canvas.height() as f64 / 2.0) - c;
    }

    pub fn pan_by(&mut self, d: EVec2) {
        self.pan += Vec2::new(d.x as f64, d.y as f64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_about_keeps_anchor_fixed() {
        let canvas = ERect::from_min_size(Pos2::new(100.0, 50.0), EVec2::new(800.0, 600.0));
        let mut v = Viewport::default();
        let anchor = Pos2::new(400.0, 300.0);
        let before = v.to_page(canvas, anchor);
        v.zoom_about(canvas, anchor, 2.5);
        let after = v.to_page(canvas, anchor);
        assert!((before - after).hypot() < 1e-6);
        assert!((v.to_screen(canvas, before) - anchor).length() < 1e-3);
    }
}
