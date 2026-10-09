use kurbo::{Point, Rect, Size, Vec2};
use serde::{Deserialize, Serialize};

/// A layer's unrotated bounds, expressed in its parent's coordinate space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Frame {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    /// Builds a frame from any rect, normalizing negative extents.
    pub fn from_rect(r: Rect) -> Self {
        let r = r.abs();
        Self::new(r.x0, r.y0, r.width(), r.height())
    }

    pub fn to_rect(self) -> Rect {
        Rect::new(self.x, self.y, self.x + self.width, self.y + self.height)
    }

    /// The frame in the layer's own coordinate space: `(0, 0, width, height)`.
    pub fn local_rect(self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    pub fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn center(self) -> Point {
        self.to_rect().center()
    }

    pub fn translate(&mut self, d: Vec2) {
        self.x += d.x;
        self.y += d.y;
    }
}

/// Bounding box of `rects`, or `None` if empty.
pub fn union_rects(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
    rects.into_iter().reduce(|a, b| a.union(b))
}
