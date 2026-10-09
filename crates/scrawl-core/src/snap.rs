//! Smart guides: snapping moving geometry to nearby layers, guides and grids, and the distance
//! measurements shown while holding ⌥. Everything is in page space.

use kurbo::{Point, Rect, Vec2};

/// Positions closer than this are considered aligned when drawing guide lines.
const ALIGNED: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// A position to snap to along one axis, with the extent of whatever it came from on the other
/// axis (so the guide line can be drawn between the two things being aligned).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub pos: f64,
    pub span: (f64, f64),
}

/// Everything a drag can snap to.
#[derive(Clone, Debug, Default)]
pub struct Targets {
    pub x: Vec<Target>,
    pub y: Vec<Target>,
    /// Layer bounds, for equal-spacing snaps.
    pub rects: Vec<Rect>,
}

impl Targets {
    /// Adds a layer's edges and center, and uses it for equal spacing.
    pub fn add_rect(&mut self, r: Rect) {
        self.add_edges(r);
        self.rects.push(r);
    }

    /// Adds a container's edges and center (an artboard): aligned to, but not spaced against.
    pub fn add_edges(&mut self, r: Rect) {
        for x in [r.x0, r.center().x, r.x1] {
            self.x.push(Target { pos: x, span: (r.y0, r.y1) });
        }
        for y in [r.y0, r.center().y, r.y1] {
            self.y.push(Target { pos: y, span: (r.x0, r.x1) });
        }
    }

    /// Adds a guide or grid line: at `pos` along `axis` (`Axis::X` is a vertical line).
    pub fn add_line(&mut self, axis: Axis, pos: f64, span: (f64, f64)) {
        let t = Target { pos, span };
        match axis {
            Axis::X => self.x.push(t),
            Axis::Y => self.y.push(t),
        }
    }

    fn along(&self, axis: Axis) -> &[Target] {
        match axis {
            Axis::X => &self.x,
            Axis::Y => &self.y,
        }
    }
}

/// A guide line to draw: at `pos` along `axis`, spanning `from..to` on the other axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapLine {
    pub axis: Axis,
    pub pos: f64,
    pub from: f64,
    pub to: f64,
}

/// Equal gaps to highlight: intervals along `axis`, drawn at `at` on the other axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Gaps {
    pub axis: Axis,
    pub intervals: Vec<(f64, f64)>,
    pub at: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feedback {
    pub lines: Vec<SnapLine>,
    pub gaps: Vec<Gaps>,
}

fn coord(p: Point, axis: Axis) -> f64 {
    match axis {
        Axis::X => p.x,
        Axis::Y => p.y,
    }
}

/// `r`'s extent along `axis`.
fn range(r: Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.x0, r.x1),
        Axis::Y => (r.y0, r.y1),
    }
}

fn other(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    }
}

fn overlaps(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Rects in the same row (or column) as `r`, sorted along `axis`.
fn row(r: Rect, rects: &[Rect], axis: Axis) -> Vec<Rect> {
    let across = range(r, other(axis));
    let mut row: Vec<Rect> = rects.iter().copied().filter(|q| overlaps(range(*q, other(axis)), across)).collect();
    row.sort_by(|a, b| range(*a, axis).0.total_cmp(&range(*b, axis).0));
    row
}

/// Gaps between consecutive, non-overlapping rects of a row.
fn row_gaps(row: &[Rect], axis: Axis) -> Vec<(f64, f64)> {
    let mut out = vec![];
    for (i, a) in row.iter().enumerate() {
        let a1 = range(*a, axis).1;
        // The nearest rect starting after `a` ends.
        if let Some(b) = row[i + 1..].iter().filter(|b| range(**b, axis).0 >= a1).min_by(|p, q| range(**p, axis).0.total_cmp(&range(**q, axis).0)) {
            out.push((a1, range(*b, axis).0));
        }
    }
    out
}

/// The nearest rects before and after `r` along `axis`, within its row.
fn neighbours(r: Rect, row: &[Rect], axis: Axis) -> (Option<Rect>, Option<Rect>) {
    let (r0, r1) = range(r, axis);
    let mid = (r0 + r1) / 2.0;
    let before = row.iter().copied().filter(|q| range(*q, axis).1 <= mid).max_by(|a, b| range(*a, axis).1.total_cmp(&range(*b, axis).1));
    let after = row.iter().copied().filter(|q| range(*q, axis).0 >= mid).min_by(|a, b| range(*a, axis).0.total_cmp(&range(*b, axis).0));
    (before, after)
}

/// The best offset along `axis` that aligns `r` with a target or spaces it evenly, if any is
/// within `threshold`.
fn snap_axis(r: Rect, targets: &Targets, threshold: f64, axis: Axis) -> Option<f64> {
    let (r0, r1) = range(r, axis);
    let mut best: Option<f64> = None;
    let consider = |best: &mut Option<f64>, d: f64| {
        if d.abs() <= threshold && best.is_none_or(|b| d.abs() < b.abs()) {
            *best = Some(d);
        }
    };
    for t in targets.along(axis) {
        for v in [r0, (r0 + r1) / 2.0, r1] {
            consider(&mut best, t.pos - v);
        }
    }
    // Alignment wins over spacing.
    if best.is_some() {
        return best;
    }
    // Equal spacing: centered between neighbours, or matching a gap already in the row. Spaced
    // positions are rounded to whole points.
    let row = row(r, &targets.rects, axis);
    let (before, after) = neighbours(r, &row, axis);
    if let (Some(b), Some(a)) = (before, after) {
        let free = range(a, axis).0 - range(b, axis).1;
        consider(&mut best, (range(b, axis).1 + (free - (r1 - r0)) / 2.0).round() - r0);
    }
    for (g0, g1) in row_gaps(&row, axis) {
        let g = g1 - g0;
        if let Some(b) = before {
            consider(&mut best, (range(b, axis).1 + g).round() - r0);
        }
        if let Some(a) = after {
            consider(&mut best, (range(a, axis).0 - g).round() - r1);
        }
    }
    best
}

/// Guide lines for every target `r` is aligned with.
fn lines_for(r: Rect, targets: &Targets, out: &mut Vec<SnapLine>) {
    for axis in [Axis::X, Axis::Y] {
        let (r0, r1) = range(r, axis);
        let (o0, o1) = range(r, other(axis));
        for t in targets.along(axis) {
            if ![r0, (r0 + r1) / 2.0, r1].iter().any(|v| (t.pos - v).abs() < ALIGNED) {
                continue;
            }
            let (from, to) = (t.span.0.min(o0), t.span.1.max(o1));
            match out.iter_mut().find(|l| l.axis == axis && (l.pos - t.pos).abs() < ALIGNED) {
                Some(l) => {
                    l.from = l.from.min(from);
                    l.to = l.to.max(to);
                }
                None => out.push(SnapLine { axis, pos: t.pos, from, to }),
            }
        }
    }
}

/// Gaps in `r`'s row equal to a gap between `r` and a neighbour.
fn gaps_for(r: Rect, targets: &Targets, out: &mut Vec<Gaps>) {
    for axis in [Axis::X, Axis::Y] {
        let row = row(r, &targets.rects, axis);
        let (before, after) = neighbours(r, &row, axis);
        let (r0, r1) = range(r, axis);
        let mine: Vec<(f64, f64)> =
            [before.map(|b| (range(b, axis).1, r0)), after.map(|a| (r1, range(a, axis).0))].into_iter().flatten().filter(|(a, b)| b > a).collect();
        let others = row_gaps(&row, axis);
        let mut intervals: Vec<(f64, f64)> = vec![];
        for &(m0, m1) in &mine {
            let equal: Vec<_> = mine.iter().chain(&others).copied().filter(|(a, b)| ((b - a) - (m1 - m0)).abs() < 0.5).collect();
            if equal.len() > 1 {
                for iv in equal {
                    if !intervals.contains(&iv) {
                        intervals.push(iv);
                    }
                }
            }
        }
        if !intervals.is_empty() {
            let (o0, o1) = range(r, other(axis));
            out.push(Gaps { axis, intervals, at: (o0 + o1) / 2.0 });
        }
    }
}

/// Snaps a rect being moved. `axes` limits which axes may snap (e.g. when ⇧ constrains the move).
/// Returns the offset to apply and the guides to show.
pub fn snap_rect(r: Rect, targets: &Targets, threshold: f64, axes: (bool, bool)) -> (Vec2, Feedback) {
    let dx = if axes.0 { snap_axis(r, targets, threshold, Axis::X).unwrap_or(0.0) } else { 0.0 };
    let dy = if axes.1 { snap_axis(r, targets, threshold, Axis::Y).unwrap_or(0.0) } else { 0.0 };
    let offset = Vec2::new(dx, dy);
    let moved = r + offset;
    let mut fb = Feedback::default();
    lines_for(moved, targets, &mut fb.lines);
    gaps_for(moved, targets, &mut fb.gaps);
    (offset, fb)
}

/// Snaps a point (a resize handle, a line end, a corner being drawn) on the given axes.
pub fn snap_point(p: Point, targets: &Targets, threshold: f64, axes: (bool, bool)) -> (Point, Feedback) {
    let mut q = p;
    let mut fb = Feedback::default();
    for (axis, on) in [(Axis::X, axes.0), (Axis::Y, axes.1)] {
        if !on {
            continue;
        }
        let v = coord(p, axis);
        let best = targets.along(axis).iter().filter(|t| (t.pos - v).abs() <= threshold).min_by(|a, b| (a.pos - v).abs().total_cmp(&(b.pos - v).abs()));
        let Some(t) = best else { continue };
        let o = coord(p, other(axis));
        match axis {
            Axis::X => q.x = t.pos,
            Axis::Y => q.y = t.pos,
        }
        // Draw every target at this position, as in `lines_for`.
        let (from, to) = targets
            .along(axis)
            .iter()
            .filter(|u| (u.pos - t.pos).abs() < ALIGNED)
            .fold((o, o), |(a, b), u| (a.min(u.span.0), b.max(u.span.1)));
        fb.lines.push(SnapLine { axis, pos: t.pos, from, to });
    }
    (q, fb)
}

/// One measurement: a line from `a` to `b` labelled with its length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub a: Point,
    pub b: Point,
}

impl Measure {
    pub fn length(&self) -> f64 {
        (self.b - self.a).hypot()
    }
}

/// Distances between the selection `s` and another layer or container `o`, as shown when
/// holding ⌥: the gap between them on each axis where they're apart, or the distance between
/// matching edges where they overlap. Also returns dashed extension lines from `o` to where a
/// measurement is drawn, when that's outside `o`.
pub fn measure(s: Rect, o: Rect) -> (Vec<Measure>, Vec<(Point, Point)>) {
    let mut out = vec![];
    let mut ext = vec![];
    for axis in [Axis::X, Axis::Y] {
        let (s0, s1) = range(s, axis);
        let (o0, o1) = range(o, axis);
        let (c0, c1) = range(s, other(axis));
        let at = (c0 + c1) / 2.0;
        let pt = |along: f64, across: f64| match axis {
            Axis::X => Point::new(along, across),
            Axis::Y => Point::new(across, along),
        };
        let segs: Vec<(f64, f64)> = if s1 <= o0 {
            vec![(s1, o0)]
        } else if o1 <= s0 {
            vec![(o1, s0)]
        } else {
            [(o0.min(s0), o0.max(s0)), (s1.min(o1), s1.max(o1))].into_iter().filter(|(a, b)| b - a > ALIGNED).collect()
        };
        let (oc0, oc1) = range(o, other(axis));
        for (a, b) in segs {
            out.push(Measure { a: pt(a, at), b: pt(b, at) });
            // If the measurement line misses `o`, extend o's edge to meet it.
            if at < oc0 || at > oc1 {
                let edge = if (a - o0).abs() < ALIGNED || (a - o1).abs() < ALIGNED { a } else { b };
                let near = if at < oc0 { oc0 } else { oc1 };
                ext.push((pt(edge, near), pt(edge, at)));
            }
        }
    }
    (out, ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(rects: &[Rect]) -> Targets {
        let mut t = Targets::default();
        rects.iter().for_each(|&r| t.add_rect(r));
        t
    }

    #[test]
    fn snaps_edge_to_nearby_edge() {
        let t = targets(&[Rect::new(0.0, 0.0, 100.0, 100.0)]);
        let (off, fb) = snap_rect(Rect::new(103.0, 200.0, 150.0, 250.0), &t, 5.0, (true, true));
        assert_eq!(off, Vec2::new(-3.0, 0.0));
        assert!(fb.lines.iter().any(|l| l.axis == Axis::X && l.pos == 100.0 && l.from == 0.0 && l.to == 250.0));
    }

    #[test]
    fn snaps_centers_and_respects_threshold() {
        let t = targets(&[Rect::new(0.0, 0.0, 100.0, 100.0)]);
        let (off, _) = snap_rect(Rect::new(300.0, 42.0, 320.0, 62.0), &t, 5.0, (true, true));
        assert_eq!(off, Vec2::new(0.0, -2.0));
        let (off, fb) = snap_rect(Rect::new(300.0, 20.0, 320.0, 30.0), &t, 5.0, (true, true));
        assert_eq!(off, Vec2::ZERO);
        assert!(fb.lines.is_empty());
    }

    #[test]
    fn locked_axis_does_not_snap() {
        let t = targets(&[Rect::new(0.0, 0.0, 100.0, 100.0)]);
        let (off, _) = snap_rect(Rect::new(103.0, 2.0, 150.0, 47.0), &t, 5.0, (false, true));
        assert_eq!(off, Vec2::new(0.0, -2.0));
    }

    #[test]
    fn snaps_to_equal_spacing() {
        // A and B are 20 apart; a third rect to the right of B snaps to the same gap.
        let t = targets(&[Rect::new(0.0, 0.0, 50.0, 50.0), Rect::new(70.0, 0.0, 120.0, 50.0)]);
        let (off, fb) = snap_rect(Rect::new(143.0, 10.0, 173.0, 30.0), &t, 5.0, (true, false));
        assert_eq!(off.x, -3.0);
        let gaps = fb.gaps.iter().find(|g| g.axis == Axis::X).expect("equal gaps shown");
        assert_eq!(gaps.intervals.len(), 2);
    }

    #[test]
    fn centers_between_neighbours() {
        let t = targets(&[Rect::new(0.0, 0.0, 50.0, 50.0), Rect::new(150.0, 0.0, 200.0, 50.0)]);
        let (off, fb) = snap_rect(Rect::new(73.0, 0.0, 123.0, 50.0), &t, 5.0, (true, false));
        assert_eq!(off.x, 2.0);
        assert!(fb.gaps.iter().any(|g| g.intervals == vec![(50.0, 75.0), (125.0, 150.0)]));
    }

    #[test]
    fn alignment_beats_spacing() {
        // Centering between the neighbours would need 2, aligning with the tall rect's edge 3.
        let mut t = targets(&[Rect::new(0.0, 0.0, 50.0, 50.0), Rect::new(150.0, 0.0, 200.0, 50.0)]);
        t.add_rect(Rect::new(70.0, 300.0, 90.0, 400.0));
        let (off, _) = snap_rect(Rect::new(73.0, 0.0, 123.0, 50.0), &t, 5.0, (true, false));
        assert_eq!(off.x, -3.0);
    }

    #[test]
    fn point_snaps_per_axis() {
        let mut t = targets(&[Rect::new(0.0, 0.0, 100.0, 100.0)]);
        t.add_line(Axis::Y, 300.0, (f64::NEG_INFINITY, f64::INFINITY));
        let (q, fb) = snap_point(Point::new(98.0, 302.0), &t, 5.0, (true, true));
        assert_eq!(q, Point::new(100.0, 300.0));
        assert_eq!(fb.lines.len(), 2);
        let (q, _) = snap_point(Point::new(98.0, 302.0), &t, 5.0, (false, true));
        assert_eq!(q, Point::new(98.0, 300.0));
    }

    #[test]
    fn measures_gap_between_separate_rects() {
        let (m, ext) = measure(Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(30.0, 0.0, 40.0, 10.0));
        assert_eq!(m, vec![Measure { a: Point::new(10.0, 5.0), b: Point::new(30.0, 5.0) }]);
        assert!(ext.is_empty());
    }

    #[test]
    fn measures_inset_inside_container() {
        let (m, _) = measure(Rect::new(10.0, 20.0, 90.0, 60.0), Rect::new(0.0, 0.0, 100.0, 100.0));
        let lengths: Vec<f64> = m.iter().map(Measure::length).collect();
        assert_eq!(lengths, vec![10.0, 10.0, 20.0, 40.0]);
    }

    #[test]
    fn extends_edge_when_measurement_misses_target() {
        // Target is to the right and well below: the horizontal measurement runs at the
        // selection's center, so the target's left edge is extended up to meet it.
        let (m, ext) = measure(Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(30.0, 50.0, 40.0, 60.0));
        assert_eq!(m.len(), 2);
        assert!(ext.contains(&(Point::new(30.0, 50.0), Point::new(30.0, 5.0))));
    }
}
