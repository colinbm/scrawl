use kurbo::{Affine, BezPath, Ellipse, Point, Rect, RoundedRect, RoundedRectRadii, Shape, Vec2};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::path::{CurvePoint, PathShape};
use crate::style::BlendMode;
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
    pub blend_mode: BlendMode,
    /// Clips the layers above it within the same parent ("Use as Mask").
    #[serde(default)]
    pub is_mask: bool,
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
    Polygon(Polygon),
    Star(Star),
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
    /// Per-corner radii (top-left, top-right, bottom-right, bottom-left), overriding
    /// `corner_radius` when set.
    #[serde(default)]
    pub corner_radii: Option<[f64; 4]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub sides: u32,
}

impl Default for Polygon {
    fn default() -> Self {
        Self { sides: 5 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Star {
    pub points: u32,
    /// Inner radius as a fraction of the outer radius.
    pub inner_ratio: f64,
}

impl Default for Star {
    fn default() -> Self {
        Self { points: 5, inner_ratio: 0.5 }
    }
}

/// A single open segment between two corner points.
fn is_line(p: &PathShape) -> bool {
    matches!(&p.subpaths[..], [sp] if !sp.closed && sp.points.len() == 2 && sp.points.iter().all(|q| q.curve_from.is_none() && q.curve_to.is_none()))
}

impl LayerKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Artboard(_) => "Artboard",
            Self::Group(_) => "Group",
            Self::Rectangle(_) => "Rectangle",
            Self::Oval => "Oval",
            Self::Polygon(p) if p.sides == 3 => "Triangle",
            Self::Polygon(_) => "Polygon",
            Self::Star(_) => "Star",
            Self::Path(p) if is_line(p) => "Line",
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
            blend_mode: BlendMode::Normal,
            is_mask: false,
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
        let mut l = Self::path(&PathShape::single(vec![CurvePoint::corner(a), CurvePoint::corner(b)], false));
        l.name = "Line".into();
        l
    }

    /// A path layer from a shape in parent coordinates. Closed paths get the default shape
    /// style, open ones the line style.
    pub fn path(shape: &PathShape) -> Self {
        let (bounds, norm) = shape.fit();
        let closed = norm.is_closed();
        let mut l = Self::new("Path", Frame::from_rect(bounds), LayerKind::Path(norm));
        l.style = if closed { Style::default_shape() } else { Style::default_line() };
        l
    }

    pub fn polygon(frame: Frame, sides: u32) -> Self {
        let mut l = Self::new(if sides == 3 { "Triangle" } else { "Polygon" }, frame, LayerKind::Polygon(Polygon { sides }));
        l.style = Style::default_shape();
        l
    }

    pub fn star(frame: Frame) -> Self {
        let mut l = Self::new("Star", frame, LayerKind::Star(Star::default()));
        l.style = Style::default_shape();
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
                let c = |v: f64| v.clamp(0.0, max_r);
                let [tl, tr, br, bl] = rect.corner_radii.unwrap_or([rect.corner_radius; 4]);
                let radii = RoundedRectRadii::new(c(tl), c(tr), c(br), c(bl));
                Some(RoundedRect::from_rect(r, radii).to_path(PATH_TOLERANCE))
            }
            LayerKind::Oval => {
                let mut path = Ellipse::from_rect(r).to_path(PATH_TOLERANCE);
                // kurbo leaves ellipses unclosed, which would show as a seam in strokes.
                path.close_path();
                Some(path)
            }
            LayerKind::Polygon(p) => Some(fitted_polygon(r, &regular_vertices(p.sides.max(3), &[1.0]))),
            LayerKind::Star(st) => {
                Some(fitted_polygon(r, &regular_vertices(st.points.max(3) * 2, &[1.0, st.inner_ratio.clamp(0.01, 1.0)])))
            }
            LayerKind::Path(p) => Some(p.to_bez_path(r.width(), r.height())),
        }
    }

    pub fn fill_rule(&self) -> crate::FillRule {
        match &self.kind {
            LayerKind::Path(p) => p.fill_rule,
            _ => crate::FillRule::NonZero,
        }
    }

    /// Whether the geometry encloses an area that can be filled.
    pub fn is_closed(&self) -> bool {
        match &self.kind {
            LayerKind::Path(p) => p.is_closed(),
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

    /// The geometry as an editable path in absolute local coordinates. `None` for containers.
    pub fn editable_path(&self) -> Option<PathShape> {
        match &self.kind {
            LayerKind::Path(p) => Some(p.scaled(self.frame.width, self.frame.height)),
            LayerKind::Artboard(_) | LayerKind::Group(_) => None,
            _ => Some(PathShape::from_bez_path(&self.local_path()?)),
        }
    }

    /// Replaces the geometry with `shape`, given in absolute local coordinates. The frame is
    /// refit to the path's tight bounds without moving anything on screen.
    pub fn set_local_path(&mut self, shape: &PathShape) {
        let (bounds, norm) = shape.fit();
        self.kind = LayerKind::Path(PathShape::default());
        self.reframe(bounds);
        self.kind = LayerKind::Path(norm);
    }

    /// Converts a shape (rectangle, oval, polygon…) into an equivalent editable path in place.
    /// Returns false for containers.
    pub fn convert_to_path(&mut self) -> bool {
        if matches!(self.kind, LayerKind::Path(_)) {
            return true;
        }
        let Some(shape) = self.editable_path() else { return false };
        self.set_local_path(&shape);
        true
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

/// Vertices of a regular polygon on the unit circle, starting at the top, cycling through
/// `radii` (for stars).
fn regular_vertices(n: u32, radii: &[f64]) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::TAU / n as f64;
            let r = radii[i as usize % radii.len()];
            Point::new(r * a.cos(), r * a.sin())
        })
        .collect()
}

/// Closes `pts` into a path stretched to fill `r`, like shape layers.
fn fitted_polygon(r: Rect, pts: &[Point]) -> BezPath {
    let b = pts.iter().fold(Rect::from_points(pts[0], pts[0]), |b, &p| b.union_pt(p));
    let map = |p: Point| {
        Point::new(r.x0 + (p.x - b.x0) / b.width() * r.width(), r.y0 + (p.y - b.y0) / b.height() * r.height())
    };
    let mut path = BezPath::new();
    path.move_to(map(pts[0]));
    pts[1..].iter().for_each(|&p| path.line_to(map(p)));
    path.close_path();
    path
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
        assert_eq!(p.subpaths[0].points[1].point, Point::new(1.0, 0.0));
        assert_eq!(l.kind.type_name(), "Line");
    }

    #[test]
    fn convert_to_path_keeps_geometry() {
        let mut l = Layer::oval(Frame::new(10.0, 20.0, 100.0, 50.0));
        l.rotation = 30.0;
        let before = l.bounds_in(Affine::IDENTITY).unwrap();
        assert!(l.convert_to_path());
        assert_eq!(l.kind.type_name(), "Path");
        let after = l.bounds_in(Affine::IDENTITY).unwrap();
        assert!((before.center() - after.center()).hypot() < 1e-6);
        assert!((before.area() - after.area()).abs() < 1e-3);
    }

    #[test]
    fn polygons_fill_their_frame() {
        let l = Layer::polygon(Frame::new(0.0, 0.0, 100.0, 80.0), 3);
        assert_eq!(l.bounds_in(Affine::IDENTITY).unwrap(), Rect::new(0.0, 0.0, 100.0, 80.0));
        let s = Layer::star(Frame::new(0.0, 0.0, 100.0, 100.0));
        let b = s.bounds_in(Affine::IDENTITY).unwrap();
        assert!((b.width() - 100.0).abs() < 1e-9 && (b.height() - 100.0).abs() < 1e-9);
    }
}
