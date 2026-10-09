use kurbo::{Affine, BezPath, Ellipse, Point, Rect, RoundedRect, Shape, Vec2};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Color, Frame, Style, geometry::union_rects};

/// Tolerance used when flattening analytic shapes to Béziers.
const PATH_TOLERANCE: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub Uuid);

impl LayerId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// A node in a page's layer tree.
///
/// A layer's [`Frame`] lives in its parent's coordinate space. Rotation and flips
/// are applied about the frame's center.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    pub frame: Frame,
    /// Degrees, clockwise on screen.
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub style: Style,
    #[serde(flatten)]
    pub kind: LayerKind,
}

fn default_true() -> bool {
    true
}

fn default_opacity() -> f64 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LayerKind {
    Artboard(Artboard),
    Group(Group),
    Rectangle(Rectangle),
    Oval,
    Path(PathShape),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artboard {
    #[serde(default)]
    pub children: Vec<Layer>,
    /// `None` renders the default white background.
    #[serde(default)]
    pub background: Option<Color>,
    #[serde(default = "default_true")]
    pub clip_content: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    #[serde(default)]
    pub children: Vec<Layer>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rectangle {
    #[serde(default)]
    pub corner_radius: f64,
}

/// A vector path whose points are normalized to the layer frame (`0..=1`), like
/// shape paths, so resizing the frame scales the path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathShape {
    pub points: Vec<CurvePoint>,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub point: Point,
    /// Outgoing control point (towards the next point), if curved.
    #[serde(default)]
    pub curve_from: Option<Point>,
    /// Incoming control point (from the previous point), if curved.
    #[serde(default)]
    pub curve_to: Option<Point>,
}

impl CurvePoint {
    pub fn corner(point: Point) -> Self {
        Self { point, curve_from: None, curve_to: None }
    }
}

impl LayerKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Artboard(_) => "Artboard",
            Self::Group(_) => "Group",
            Self::Rectangle(_) => "Rectangle",
            Self::Oval => "Oval",
            Self::Path(p) if p.points.len() == 2 && !p.closed => "Line",
            Self::Path(_) => "Path",
        }
    }
}

impl Layer {
    pub fn new(name: impl Into<String>, frame: Frame, kind: LayerKind) -> Self {
        Self {
            id: LayerId::new(),
            name: name.into(),
            visible: true,
            locked: false,
            frame,
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            opacity: 1.0,
            style: Style::default(),
            kind,
        }
    }

    pub fn artboard(name: impl Into<String>, frame: Frame) -> Self {
        Self::new(
            name,
            frame,
            LayerKind::Artboard(Artboard { children: vec![], background: None, clip_content: true }),
        )
    }

    pub fn rectangle(frame: Frame) -> Self {
        let mut l = Self::new("Rectangle", frame, LayerKind::Rectangle(Rectangle::default()));
        l.style = Style::default_shape();
        l
    }

    pub fn oval(frame: Frame) -> Self {
        let mut l = Self::new("Oval", frame, LayerKind::Oval);
        l.style = Style::default_shape();
        l
    }

    /// A straight line between two points in parent coordinates.
    pub fn line(a: Point, b: Point) -> Self {
        let frame = Frame::from_rect(Rect::from_points(a, b));
        let norm = |p: Point| {
            Point::new(
                if frame.width > 0.0 { (p.x - frame.x) / frame.width } else { 0.0 },
                if frame.height > 0.0 { (p.y - frame.y) / frame.height } else { 0.0 },
            )
        };
        let shape =
            PathShape { points: vec![CurvePoint::corner(norm(a)), CurvePoint::corner(norm(b))], closed: false };
        let mut l = Self::new("Line", frame, LayerKind::Path(shape));
        l.style = Style::default_line();
        l
    }

    pub fn group(children: Vec<Layer>, frame: Frame) -> Self {
        Self::new("Group", frame, LayerKind::Group(Group { children }))
    }

    pub fn children(&self) -> Option<&Vec<Layer>> {
        match &self.kind {
            LayerKind::Artboard(a) => Some(&a.children),
            LayerKind::Group(g) => Some(&g.children),
            _ => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Layer>> {
        match &mut self.kind {
            LayerKind::Artboard(a) => Some(&mut a.children),
            LayerKind::Group(g) => Some(&mut g.children),
            _ => None,
        }
    }

    pub fn is_artboard(&self) -> bool {
        matches!(self.kind, LayerKind::Artboard(_))
    }

    pub fn is_group(&self) -> bool {
        matches!(self.kind, LayerKind::Group(_))
    }

    /// Maps this layer's local space into its parent's space.
    pub fn local_transform(&self) -> Affine {
        let f = self.frame;
        let c = Vec2::new(f.width / 2.0, f.height / 2.0);
        let sx = if self.flip_horizontal { -1.0 } else { 1.0 };
        let sy = if self.flip_vertical { -1.0 } else { 1.0 };
        Affine::translate(f.origin().to_vec2() + c)
            * Affine::rotate(self.rotation.to_radians())
            * Affine::scale_non_uniform(sx, sy)
            * Affine::translate(-c)
    }

    /// The layer's own geometry in local space. `None` for groups, which have none.
    pub fn local_path(&self) -> Option<BezPath> {
        let r = self.frame.local_rect();
        match &self.kind {
            LayerKind::Artboard(_) => Some(r.to_path(PATH_TOLERANCE)),
            LayerKind::Group(_) => None,
            LayerKind::Rectangle(rect) => {
                let max_r = r.width().min(r.height()) / 2.0;
                Some(RoundedRect::from_rect(r, rect.corner_radius.clamp(0.0, max_r)).to_path(PATH_TOLERANCE))
            }
            LayerKind::Oval => Some(Ellipse::from_rect(r).to_path(PATH_TOLERANCE)),
            LayerKind::Path(p) => Some(p.to_bez_path(r.width(), r.height())),
        }
    }

    /// Whether the geometry encloses an area that can be filled.
    pub fn is_closed(&self) -> bool {
        match &self.kind {
            LayerKind::Path(p) => p.closed,
            LayerKind::Group(_) => false,
            _ => true,
        }
    }

    /// Bounds of the rendered geometry (excluding borders) in the space `to_space` maps local
    /// coordinates into.
    pub fn bounds_in(&self, to_space: Affine) -> Option<Rect> {
        let t = to_space * self.local_transform();
        match self.children() {
            Some(children) if self.is_group() => union_rects(children.iter().filter_map(|c| c.bounds_in(t))),
            _ => self.local_path().map(|p| (t * p).bounding_box()),
        }
    }

    /// Moves the frame so the layer occupies `r`, given in the layer's *current* local space,
    /// while keeping its rotation and flips. Group children are scaled to fit; artboard
    /// children keep their positions relative to the artboard's top-left.
    pub fn reframe(&mut self, r: Rect) {
        let r = r.abs();
        let old = self.frame;
        let center_in_parent = self.local_transform() * r.center();
        let size = r.size();
        self.frame = Frame::new(
            center_in_parent.x - size.width / 2.0,
            center_in_parent.y - size.height / 2.0,
            size.width,
            size.height,
        );
        match &mut self.kind {
            LayerKind::Group(g) => {
                let sx = if old.width > 0.0 { size.width / old.width } else { 1.0 };
                let sy = if old.height > 0.0 { size.height / old.height } else { 1.0 };
                for c in &mut g.children {
                    c.frame.x = (c.frame.x - r.x0) * sx;
                    c.frame.y = (c.frame.y - r.y0) * sy;
                    c.frame.width *= sx;
                    c.frame.height *= sy;
                    if let LayerKind::Rectangle(rect) = &mut c.kind {
                        rect.corner_radius *= sx.min(sy);
                    }
                }
            }
            LayerKind::Artboard(a) => {
                for c in &mut a.children {
                    c.frame.translate(-r.origin().to_vec2());
                }
            }
            _ => {}
        }
    }

    /// Shrinks or grows a group's frame to tightly enclose its children, without moving them on
    /// screen. No-op for other layer types.
    pub fn fit_to_children(&mut self) {
        let transform = self.local_transform();
        let LayerKind::Group(g) = &mut self.kind else { return };
        let Some(r) = union_rects(g.children.iter().filter_map(|c| c.bounds_in(Affine::IDENTITY))) else {
            return;
        };
        let center_in_parent = transform * r.center();
        self.frame = Frame::new(
            center_in_parent.x - r.width() / 2.0,
            center_in_parent.y - r.height() / 2.0,
            r.width(),
            r.height(),
        );
        for c in &mut g.children {
            c.frame.translate(-r.origin().to_vec2());
        }
    }

    /// Deep copy with fresh ids for this layer and all descendants.
    pub fn duplicate(&self) -> Layer {
        let mut l = self.clone();
        l.regenerate_ids();
        l
    }

    fn regenerate_ids(&mut self) {
        self.id = LayerId::new();
        if let Some(children) = self.children_mut() {
            children.iter_mut().for_each(Layer::regenerate_ids);
        }
    }

    /// Depth-first, pre-order traversal of this layer and its descendants.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Layer)) {
        f(self);
        if let Some(children) = self.children() {
            children.iter().for_each(|c| c.walk(f));
        }
    }
}

impl PathShape {
    pub fn to_bez_path(&self, width: f64, height: f64) -> BezPath {
        let scale = |p: Point| Point::new(p.x * width, p.y * height);
        let mut path = BezPath::new();
        let Some(first) = self.points.first() else { return path };
        path.move_to(scale(first.point));
        let n = self.points.len();
        let segments = if self.closed { n } else { n.saturating_sub(1) };
        for i in 0..segments {
            let a = &self.points[i];
            let b = &self.points[(i + 1) % n];
            match (a.curve_from, b.curve_to) {
                (None, None) => path.line_to(scale(b.point)),
                (c1, c2) => path.curve_to(
                    scale(c1.unwrap_or(a.point)),
                    scale(c2.unwrap_or(b.point)),
                    scale(b.point),
                ),
            }
        }
        if self.closed {
            path.close_path();
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Point, b: Point) -> bool {
        (a - b).hypot() < 1e-9
    }

    #[test]
    fn reframe_keeps_opposite_edge_fixed_when_rotated() {
        let mut l = Layer::rectangle(Frame::new(10.0, 20.0, 100.0, 50.0));
        l.rotation = 30.0;
        let anchor_before = l.local_transform() * Point::new(0.0, 0.0);
        // Drag the right edge out by 40 in local space.
        l.reframe(Rect::new(0.0, 0.0, 140.0, 50.0));
        let anchor_after = l.local_transform() * Point::new(0.0, 0.0);
        assert!(approx(anchor_before, anchor_after));
        assert!((l.frame.width - 140.0).abs() < 1e-9);
    }

    #[test]
    fn fit_to_children_preserves_world_positions() {
        let child = Layer::rectangle(Frame::new(30.0, 40.0, 10.0, 10.0));
        let mut g = Layer::group(vec![child], Frame::new(100.0, 100.0, 200.0, 200.0));
        g.rotation = 45.0;
        let before = g.local_transform() * g.children().unwrap()[0].local_transform() * Point::ORIGIN;
        g.fit_to_children();
        let after = g.local_transform() * g.children().unwrap()[0].local_transform() * Point::ORIGIN;
        assert!(approx(before, after));
        assert!((g.frame.width - 10.0).abs() < 1e-9);
    }

    #[test]
    fn line_normalizes_points() {
        let l = Layer::line(Point::new(10.0, 10.0), Point::new(110.0, 10.0));
        assert_eq!(l.frame, Frame::new(10.0, 10.0, 100.0, 0.0));
        let LayerKind::Path(p) = &l.kind else { panic!() };
        assert_eq!(p.points[1].point, Point::new(1.0, 0.0));
        assert_eq!(l.kind.type_name(), "Line");
    }
}
