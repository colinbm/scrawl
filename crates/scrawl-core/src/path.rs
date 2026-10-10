//! Editable vector paths.
//!
//! Points are stored normalized to the owning layer's frame (`0..=1` for the path's bounds), as
//! as is usual, so resizing a layer scales its path. Editing works on absolute local coordinates
//! and re-normalizes with [`PathShape::fit`].

use kurbo::{BezPath, ParamCurve, PathEl, PathSeg, Point, Rect, Shape, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathShape {
    pub subpaths: Vec<Subpath>,
    #[serde(default)]
    pub fill_rule: FillRule,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Subpath {
    pub points: Vec<CurvePoint>,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// How a point's two control handles relate when one is moved (point types).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PointMode {
    /// No handles: a sharp corner.
    #[default]
    Straight,
    /// Handles are collinear and the same length.
    Mirrored,
    /// Handles are collinear but may differ in length.
    Asymmetric,
    /// Handles move independently.
    Disconnected,
}

impl PointMode {
    pub const ALL: [Self; 4] = [Self::Straight, Self::Mirrored, Self::Asymmetric, Self::Disconnected];

    pub fn label(self) -> &'static str {
        match self {
            Self::Straight => "Straight",
            Self::Mirrored => "Mirrored",
            Self::Asymmetric => "Asymmetric",
            Self::Disconnected => "Disconnected",
        }
    }
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
    #[serde(default)]
    pub mode: PointMode,
    /// Rounds a sharp corner between two straight segments, in local units (not normalized).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub corner_radius: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// A rounded corner: where it leaves the incoming edge and joins the outgoing one, with the
/// arc's control points.
#[derive(Clone, Copy, Debug)]
struct Corner {
    entry: Point,
    c1: Point,
    c2: Point,
    exit: Point,
}

/// Identifies a point within a [`PathShape`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PointRef {
    pub subpath: usize,
    pub point: usize,
}

/// Which control handle of a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleSide {
    /// `curve_to`, towards the previous point.
    In,
    /// `curve_from`, towards the next point.
    Out,
}

impl CurvePoint {
    pub fn corner(point: Point) -> Self {
        Self { point, curve_from: None, curve_to: None, mode: PointMode::Straight, corner_radius: 0.0 }
    }

    /// A smooth point whose outgoing handle is at `out`; the incoming one is mirrored.
    pub fn smooth(point: Point, out: Point) -> Self {
        Self { point, curve_from: Some(out), curve_to: Some(point - (out - point)), mode: PointMode::Mirrored, corner_radius: 0.0 }
    }

    fn map(self, f: impl Fn(Point) -> Point) -> Self {
        Self { point: f(self.point), curve_from: self.curve_from.map(&f), curve_to: self.curve_to.map(&f), ..self }
    }

    pub fn translate(&mut self, d: Vec2) {
        *self = self.map(|p| p + d);
    }

    pub fn handle(&self, side: HandleSide) -> Option<Point> {
        match side {
            HandleSide::In => self.curve_to,
            HandleSide::Out => self.curve_from,
        }
    }

    /// Moves one handle, updating the other according to the point's mode.
    pub fn set_handle(&mut self, side: HandleSide, to: Point) {
        let (this, other) = match side {
            HandleSide::In => (&mut self.curve_to, &mut self.curve_from),
            HandleSide::Out => (&mut self.curve_from, &mut self.curve_to),
        };
        *this = Some(to);
        let v = to - self.point;
        match self.mode {
            PointMode::Mirrored => *other = Some(self.point - v),
            PointMode::Asymmetric => {
                let len = other.map_or(v.hypot(), |o| (o - self.point).hypot());
                if v.hypot() > 1e-9 {
                    *other = Some(self.point - v.normalize() * len);
                }
            }
            PointMode::Straight => self.mode = PointMode::Disconnected,
            PointMode::Disconnected => {}
        }
    }

    /// Changes the mode, synthesizing or dropping handles. `prev` and `next` are the neighboring
    /// points, used to orient new handles along the tangent.
    pub fn set_mode(&mut self, mode: PointMode, prev: Option<Point>, next: Option<Point>) {
        self.mode = mode;
        match mode {
            PointMode::Straight => {
                self.curve_from = None;
                self.curve_to = None;
            }
            PointMode::Mirrored | PointMode::Asymmetric | PointMode::Disconnected => {
                if self.curve_from.is_none() && self.curve_to.is_none() {
                    let p = self.point;
                    let (a, b) = (prev.unwrap_or(p), next.unwrap_or(p));
                    let tangent = if (b - a).hypot() > 1e-9 { (b - a).normalize() } else { Vec2::new(1.0, 0.0) };
                    let len_in = (p - a).hypot() / 3.0;
                    let len_out = (b - p).hypot() / 3.0;
                    let len = if mode == PointMode::Mirrored { (len_in + len_out) / 2.0 } else { 0.0 };
                    let (li, lo) = if mode == PointMode::Mirrored { (len, len) } else { (len_in, len_out) };
                    self.curve_to = Some(p - tangent * li.max(1.0));
                    self.curve_from = Some(p + tangent * lo.max(1.0));
                } else if mode == PointMode::Mirrored
                    && let Some(out) = self.curve_from.or(self.curve_to.map(|t| self.point - (t - self.point)))
                {
                    self.curve_from = Some(out);
                    self.curve_to = Some(self.point - (out - self.point));
                }
            }
        }
    }
}

impl Subpath {
    /// Number of segments.
    pub fn segment_count(&self) -> usize {
        let n = self.points.len();
        if self.closed { n } else { n.saturating_sub(1) }
    }

    /// The segment starting at point `i`.
    pub fn segment(&self, i: usize) -> PathSeg {
        let n = self.points.len();
        let a = &self.points[i];
        let b = &self.points[(i + 1) % n];
        match (a.curve_from, b.curve_to) {
            (None, None) => PathSeg::Line(kurbo::Line::new(a.point, b.point)),
            (c1, c2) => PathSeg::Cubic(kurbo::CubicBez::new(
                a.point,
                c1.unwrap_or(a.point),
                c2.unwrap_or(b.point),
                b.point,
            )),
        }
    }

    pub fn neighbors(&self, i: usize) -> (Option<Point>, Option<Point>) {
        let n = self.points.len();
        let prev = if i > 0 { Some(i - 1) } else if self.closed && n > 1 { Some(n - 1) } else { None };
        let next = if i + 1 < n { Some(i + 1) } else if self.closed && n > 1 { Some(0) } else { None };
        (prev.map(|j| self.points[j].point), next.map(|j| self.points[j].point))
    }

    /// Splits segment `i` at parameter `t`, inserting a point and returning its index.
    pub fn split_segment(&mut self, i: usize, t: f64) -> usize {
        let n = self.points.len();
        let j = (i + 1) % n;
        let seg = self.segment(i);
        let at = seg.eval(t);
        let new = match seg {
            PathSeg::Cubic(c) => {
                let (l, r) = (c.subsegment(0.0..t), c.subsegment(t..1.0));
                self.points[i].curve_from = Some(l.p1);
                self.points[j].curve_to = Some(r.p2);
                CurvePoint { point: at, curve_to: Some(l.p2), curve_from: Some(r.p1), mode: PointMode::Asymmetric, corner_radius: 0.0 }
            }
            _ => CurvePoint::corner(at),
        };
        self.points.insert(i + 1, new);
        i + 1
    }

    /// The rounding of point `i`, if it has a radius and sits between two straight segments.
    /// The radius shrinks so the arc fits in half of each edge.
    fn corner(&self, i: usize) -> Option<Corner> {
        let n = self.points.len();
        let p = &self.points[i];
        if p.corner_radius <= 0.0 || p.curve_from.is_some() || p.curve_to.is_some() || n < 3 {
            return None;
        }
        if !self.closed && (i == 0 || i == n - 1) {
            return None;
        }
        let prev = &self.points[(i + n - 1) % n];
        let next = &self.points[(i + 1) % n];
        if prev.curve_from.is_some() || next.curve_to.is_some() {
            return None;
        }
        let (a, b, c) = (prev.point, p.point, next.point);
        let (la, lc) = ((a - b).hypot(), (c - b).hypot());
        if la < 1e-9 || lc < 1e-9 {
            return None;
        }
        let (u, v) = ((a - b) / la, (c - b) / lc);
        let theta = u.dot(v).clamp(-1.0, 1.0).acos();
        if theta < 1e-6 || (std::f64::consts::PI - theta) < 1e-6 {
            return None;
        }
        let half = (theta / 2.0).tan();
        let d = (p.corner_radius / half).min(la / 2.0).min(lc / 2.0);
        let r = d * half;
        let sweep = std::f64::consts::PI - theta;
        let h = 4.0 / 3.0 * (sweep / 4.0).tan() * r;
        let (entry, exit) = (b + u * d, b + v * d);
        Some(Corner { entry, c1: entry - u * h, c2: exit - v * h, exit })
    }

    /// Removes segment `i`: a closed subpath opens there; an open one splits in two. Returns the
    /// pieces with at least two points.
    pub fn cut(&self, i: usize) -> Vec<Subpath> {
        let n = self.points.len();
        if self.closed {
            let points = (0..n).map(|k| self.points[(i + 1 + k) % n]).collect();
            return vec![Subpath { points, closed: false }];
        }
        let (a, b) = self.points.split_at((i + 1).min(n));
        [a, b].into_iter().filter(|p| p.len() >= 2).map(|p| Subpath { points: p.to_vec(), closed: false }).collect()
    }

    pub fn reverse(&mut self) {
        self.points.reverse();
        for p in &mut self.points {
            std::mem::swap(&mut p.curve_from, &mut p.curve_to);
        }
    }

    fn map(&self, f: impl Fn(Point) -> Point + Copy) -> Self {
        Self { points: self.points.iter().map(|p| p.map(f)).collect(), closed: self.closed }
    }
}

impl PathShape {
    pub fn single(points: Vec<CurvePoint>, closed: bool) -> Self {
        Self { subpaths: vec![Subpath { points, closed }], fill_rule: FillRule::NonZero }
    }

    pub fn is_closed(&self) -> bool {
        self.subpaths.iter().any(|s| s.closed)
    }

    pub fn point(&self, r: PointRef) -> Option<&CurvePoint> {
        self.subpaths.get(r.subpath)?.points.get(r.point)
    }

    pub fn point_mut(&mut self, r: PointRef) -> Option<&mut CurvePoint> {
        self.subpaths.get_mut(r.subpath)?.points.get_mut(r.point)
    }

    pub fn point_refs(&self) -> impl Iterator<Item = PointRef> + '_ {
        self.subpaths
            .iter()
            .enumerate()
            .flat_map(|(s, sp)| (0..sp.points.len()).map(move |p| PointRef { subpath: s, point: p }))
    }

    /// The path scaled from normalized to local coordinates.
    pub fn to_bez_path(&self, width: f64, height: f64) -> BezPath {
        self.scaled(width, height).to_absolute_bez_path()
    }

    /// Converts normalized coordinates into absolute local ones.
    pub fn scaled(&self, width: f64, height: f64) -> Self {
        let f = move |p: Point| Point::new(p.x * width, p.y * height);
        Self { subpaths: self.subpaths.iter().map(|s| s.map(f)).collect(), fill_rule: self.fill_rule }
    }

    /// Treats the stored coordinates as absolute and builds a Bézier path, rounding corners
    /// that have a radius.
    pub fn to_absolute_bez_path(&self) -> BezPath {
        let mut path = BezPath::new();
        for sp in &self.subpaths {
            if sp.points.is_empty() {
                continue;
            }
            let n = sp.points.len();
            let corners: Vec<Option<Corner>> = (0..n).map(|i| sp.corner(i)).collect();
            path.move_to(corners[0].map_or(sp.points[0].point, |c| c.exit));
            for i in 0..sp.segment_count() {
                let j = (i + 1) % n;
                match sp.segment(i) {
                    PathSeg::Line(l) => path.line_to(corners[j].map_or(l.p1, |c| c.entry)),
                    PathSeg::Cubic(c) => path.curve_to(c.p1, c.p2, c.p3),
                    PathSeg::Quad(q) => path.quad_to(q.p1, q.p2),
                }
                if let Some(c) = corners[j] {
                    path.curve_to(c.c1, c.c2, c.exit);
                }
            }
            if sp.closed {
                path.close_path();
            }
        }
        path
    }

    /// Given a shape in absolute coordinates, returns its tight bounds and the shape normalized
    /// to them.
    pub fn fit(&self) -> (Rect, PathShape) {
        // On-curve points are included explicitly: a lone point has no segments, and kurbo
        // reports an empty path's bounds as the origin.
        let mut points = self.subpaths.iter().flat_map(|s| s.points.iter().map(|p| p.point));
        let Some(first) = points.next() else { return (Rect::ZERO, self.clone()) };
        let mut bounds = points.fold(Rect::from_points(first, first), |r, p| r.union_pt(p));
        let path = self.to_absolute_bez_path();
        if path.segments().next().is_some() {
            bounds = bounds.union(path.bounding_box());
        }
        let (w, h) = (bounds.width(), bounds.height());
        let f = move |p: Point| {
            Point::new(
                if w > 0.0 { (p.x - bounds.x0) / w } else { 0.0 },
                if h > 0.0 { (p.y - bounds.y0) / h } else { 0.0 },
            )
        };
        let shape = Self { subpaths: self.subpaths.iter().map(|s| s.map(f)).collect(), fill_rule: self.fill_rule };
        (bounds, shape)
    }

    /// Converts a Bézier path into an editable shape (absolute coordinates).
    pub fn from_bez_path(path: &BezPath) -> Self {
        let mut subpaths: Vec<Subpath> = vec![];
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => subpaths.push(Subpath { points: vec![CurvePoint::corner(p)], closed: false }),
                PathEl::LineTo(p) => {
                    if let Some(sp) = subpaths.last_mut() {
                        sp.points.push(CurvePoint::corner(p));
                    }
                }
                PathEl::QuadTo(c, p) => {
                    if let Some(sp) = subpaths.last_mut()
                        && let Some(last) = sp.points.last_mut()
                    {
                        let p0 = last.point;
                        last.curve_from = Some(p0 + (c - p0) * (2.0 / 3.0));
                        let mut np = CurvePoint::corner(p);
                        np.curve_to = Some(p + (c - p) * (2.0 / 3.0));
                        sp.points.push(np);
                    }
                }
                PathEl::CurveTo(c1, c2, p) => {
                    if let Some(sp) = subpaths.last_mut()
                        && let Some(last) = sp.points.last_mut()
                    {
                        last.curve_from = Some(c1);
                        let mut np = CurvePoint::corner(p);
                        np.curve_to = Some(c2);
                        sp.points.push(np);
                    }
                }
                PathEl::ClosePath => {
                    if let Some(sp) = subpaths.last_mut() {
                        sp.closed = true;
                    }
                }
            }
        }
        subpaths.retain(|s| !s.points.is_empty());
        for sp in &mut subpaths {
            // A final point that returns to the start is an implicit close (kurbo's ellipses end
            // this way without a ClosePath).
            if sp.points.len() > 2 {
                let (first, last) = (sp.points[0], *sp.points.last().unwrap());
                if (first.point - last.point).hypot() < 1e-6 {
                    sp.points[0].curve_to = last.curve_to;
                    sp.points.pop();
                    sp.closed = true;
                }
            }
            for p in &mut sp.points {
                p.mode = infer_mode(p);
            }
        }
        Self { subpaths, fill_rule: FillRule::NonZero }
    }
}

fn infer_mode(p: &CurvePoint) -> PointMode {
    match (p.curve_to, p.curve_from) {
        (None, None) => PointMode::Straight,
        (Some(a), Some(b)) => {
            let (va, vb) = (a - p.point, b - p.point);
            let collinear = va.hypot() > 1e-9 && vb.hypot() > 1e-9 && va.normalize().dot(vb.normalize()) < -0.9999;
            if !collinear {
                PointMode::Disconnected
            } else if (va.hypot() - vb.hypot()).abs() < 1e-6 {
                PointMode::Mirrored
            } else {
                PointMode::Asymmetric
            }
        }
        _ => PointMode::Disconnected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Ellipse, Shape};

    #[test]
    fn bez_round_trip_through_shape() {
        let ellipse = Ellipse::new((50.0, 30.0), (40.0, 20.0), 0.0).to_path(0.01);
        let shape = PathShape::from_bez_path(&ellipse);
        assert_eq!(shape.subpaths.len(), 1);
        assert!(shape.subpaths[0].closed);
        assert!(shape.subpaths[0].points.iter().all(|p| p.mode == PointMode::Mirrored));
        let back = shape.to_absolute_bez_path();
        assert!((back.area() - ellipse.area()).abs() < 1e-6);
    }

    #[test]
    fn fit_normalizes_to_tight_bounds() {
        let shape = PathShape::single(
            vec![CurvePoint::corner(Point::new(10.0, 20.0)), CurvePoint::corner(Point::new(30.0, 60.0))],
            false,
        );
        let (bounds, norm) = shape.fit();
        assert_eq!(bounds, Rect::new(10.0, 20.0, 30.0, 60.0));
        assert_eq!(norm.subpaths[0].points[1].point, Point::new(1.0, 1.0));
        assert_eq!(norm.scaled(20.0, 40.0).subpaths[0].points[1].point, Point::new(20.0, 40.0));
        let lone = PathShape::single(vec![CurvePoint::corner(Point::new(7.0, 9.0))], false);
        assert_eq!(lone.fit().0, Rect::new(7.0, 9.0, 7.0, 9.0));
    }

    #[test]
    fn split_preserves_curve() {
        let mut sp = Subpath {
            points: vec![CurvePoint::smooth(Point::new(0.0, 0.0), Point::new(10.0, 20.0)), CurvePoint::corner(Point::new(50.0, 0.0))],
            closed: false,
        };
        let before = sp.segment(0);
        let i = sp.split_segment(0, 0.3);
        assert_eq!(i, 1);
        assert!((sp.points[1].point - before.eval(0.3)).hypot() < 1e-9);
        assert!((sp.segment(1).eval(0.5) - before.eval(0.65)).hypot() < 1e-9);
    }

    fn square(radius_at: &[(usize, f64)]) -> PathShape {
        let mut points: Vec<CurvePoint> =
            [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)].iter().map(|&(x, y)| CurvePoint::corner(Point::new(x, y))).collect();
        for &(i, r) in radius_at {
            points[i].corner_radius = r;
        }
        PathShape::single(points, true)
    }

    #[test]
    fn corner_radius_rounds_one_corner() {
        let plain = square(&[]).to_absolute_bez_path().area().abs();
        let rounded = square(&[(2, 10.0)]).to_absolute_bez_path().area().abs();
        // A 10-radius corner trims r² − πr²/4.
        let trimmed = 100.0 - std::f64::consts::PI * 25.0;
        assert!((plain - rounded - trimmed).abs() < 0.1, "{}", plain - rounded);
        // Radii too big for the edges shrink to fit half of each.
        let all = square(&[(0, 500.0), (1, 500.0), (2, 500.0), (3, 500.0)]).to_absolute_bez_path();
        let circle = std::f64::consts::PI * 50.0 * 50.0;
        assert!((all.area().abs() - circle).abs() < 5.0);
        // The radius survives normalizing and scaling, staying in local units.
        let (_, norm) = square(&[(2, 10.0)]).fit();
        let again = norm.to_bez_path(100.0, 100.0).area().abs();
        assert!((again - rounded).abs() < 1e-6);
    }

    #[test]
    fn cutting_segments() {
        let closed = &square(&[]).subpaths[0];
        let open = closed.cut(1);
        assert_eq!(open.len(), 1);
        assert!(!open[0].closed);
        assert_eq!(open[0].points[0].point, Point::new(100.0, 100.0), "opens where the cut was");
        assert_eq!(open[0].points.len(), 4);
        let pieces = open[0].cut(1);
        assert_eq!(pieces.iter().map(|p| p.points.len()).collect::<Vec<_>>(), [2, 2]);
        // Cutting an end segment drops the lone point.
        assert_eq!(open[0].cut(0).iter().map(|p| p.points.len()).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn mirrored_handles_follow() {
        let mut p = CurvePoint::smooth(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        p.set_handle(HandleSide::In, Point::new(0.0, -5.0));
        assert_eq!(p.curve_from, Some(Point::new(0.0, 5.0)));
        p.mode = PointMode::Asymmetric;
        p.set_handle(HandleSide::Out, Point::new(20.0, 0.0));
        assert!((p.curve_to.unwrap() - Point::new(-5.0, 0.0)).hypot() < 1e-9);
    }
}
