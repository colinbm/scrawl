//! Resizing constraints: how a layer responds when its group (or an artboard with "Adjust content
//! on resize") changes size. With no constraints, layers scale proportionally.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resizing {
    /// Keep the distance to the parent's left edge.
    #[serde(default)]
    pub left: bool,
    #[serde(default)]
    pub right: bool,
    #[serde(default)]
    pub top: bool,
    #[serde(default)]
    pub bottom: bool,
    #[serde(default)]
    pub fix_width: bool,
    #[serde(default)]
    pub fix_height: bool,
}

impl Resizing {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// A child's new position and size along one axis, given its old ones and the parent's old
    /// and new size (positions are relative to the parent's leading edge).
    pub fn axis(pos: f64, size: f64, old: f64, new: f64, pin_start: bool, pin_end: bool, fix: bool) -> (f64, f64) {
        let s = if old > 0.0 { new / old } else { 1.0 };
        let scaled = if fix { size } else { size * s };
        match (pin_start, pin_end) {
            // Pinned to both edges: stretch with the parent (unless the size is fixed too, in
            // which case the leading pin wins).
            (true, true) if !fix => (pos, (size + new - old).max(0.0)),
            (true, _) => (pos, scaled),
            (false, true) => (new - (old - pos - size) - scaled, scaled),
            // Free: the center keeps its relative position.
            (false, false) => ((pos + size / 2.0) * s - scaled / 2.0, scaled),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, Layer, LayerKind};
    use kurbo::Rect;

    fn frames(l: &Layer) -> Vec<(f64, f64, f64, f64)> {
        l.children().unwrap().iter().map(|c| (c.frame.x, c.frame.y, c.frame.width, c.frame.height)).collect()
    }

    #[test]
    fn group_scales_children_from_any_handle() {
        let a = Layer::rectangle(Frame::new(0.0, 0.0, 50.0, 100.0));
        let b = Layer::rectangle(Frame::new(50.0, 0.0, 50.0, 100.0));
        let mut g = Layer::group(vec![a, b], Frame::new(0.0, 0.0, 100.0, 100.0));
        // Left handle dragged to x = 50.
        g.reframe(Rect::new(50.0, 0.0, 100.0, 100.0));
        assert_eq!((g.frame.x, g.frame.width), (50.0, 50.0));
        assert_eq!(frames(&g), vec![(0.0, 0.0, 25.0, 100.0), (25.0, 0.0, 25.0, 100.0)]);
    }

    #[test]
    fn group_children_follow_constraints() {
        let mut bg = Layer::rectangle(Frame::new(0.0, 0.0, 200.0, 100.0));
        bg.resizing = Resizing { left: true, right: true, top: true, bottom: true, ..Default::default() };
        let mut icon = Layer::rectangle(Frame::new(10.0, 10.0, 20.0, 20.0));
        icon.resizing = Resizing { left: true, top: true, fix_width: true, fix_height: true, ..Default::default() };
        let mut badge = Layer::rectangle(Frame::new(170.0, 10.0, 20.0, 20.0));
        badge.resizing = Resizing { right: true, top: true, fix_width: true, fix_height: true, ..Default::default() };
        let mut g = Layer::group(vec![bg, icon, badge], Frame::new(0.0, 0.0, 200.0, 100.0));
        g.reframe(Rect::new(0.0, 0.0, 300.0, 100.0));
        assert_eq!(frames(&g), vec![(0.0, 0.0, 300.0, 100.0), (10.0, 10.0, 20.0, 20.0), (270.0, 10.0, 20.0, 20.0)]);
    }

    #[test]
    fn nested_groups_resize_their_contents() {
        let inner_child = Layer::rectangle(Frame::new(0.0, 0.0, 50.0, 50.0));
        let inner = Layer::group(vec![inner_child], Frame::new(0.0, 0.0, 50.0, 50.0));
        let mut g = Layer::group(vec![inner], Frame::new(0.0, 0.0, 50.0, 50.0));
        g.reframe(Rect::new(0.0, 0.0, 100.0, 100.0));
        let inner = &g.children().unwrap()[0];
        assert_eq!(frames(inner), vec![(0.0, 0.0, 100.0, 100.0)]);
    }

    #[test]
    fn artboards_keep_or_adjust_content() {
        let mut child = Layer::rectangle(Frame::new(300.0, 10.0, 50.0, 50.0));
        child.resizing = Resizing { right: true, fix_width: true, ..Default::default() };
        let mut ab = Layer::artboard("A", Frame::new(0.0, 0.0, 400.0, 400.0));
        ab.children_mut().unwrap().push(child);
        let mut adjusting = ab.clone();
        ab.reframe(Rect::new(0.0, 0.0, 300.0, 400.0));
        assert_eq!(frames(&ab)[0].0, 300.0);
        if let LayerKind::Artboard(a) = &mut adjusting.kind {
            a.resizes_content = true;
        }
        adjusting.reframe(Rect::new(0.0, 0.0, 300.0, 400.0));
        assert_eq!(frames(&adjusting)[0].0, 200.0);
    }

    #[test]
    fn free_layers_scale_proportionally() {
        assert_eq!(Resizing::axis(50.0, 50.0, 100.0, 200.0, false, false, false), (100.0, 100.0));
    }

    #[test]
    fn fixed_size_keeps_relative_center() {
        assert_eq!(Resizing::axis(40.0, 20.0, 100.0, 200.0, false, false, true), (90.0, 20.0));
    }

    #[test]
    fn pins_keep_margins() {
        // Left pin: same left margin, scaled width.
        assert_eq!(Resizing::axis(10.0, 20.0, 100.0, 200.0, true, false, false), (10.0, 40.0));
        // Right pin with fixed width: same right margin (70).
        assert_eq!(Resizing::axis(10.0, 20.0, 100.0, 200.0, false, true, true), (110.0, 20.0));
        // Both pins: stretch.
        assert_eq!(Resizing::axis(10.0, 20.0, 100.0, 200.0, true, true, false), (10.0, 120.0));
        assert_eq!(Resizing::axis(10.0, 20.0, 100.0, 50.0, true, true, false), (10.0, 0.0));
    }

    #[test]
    fn rotated_children_pin_by_their_rotated_bounds() {
        // A 40×10 bar turned 90° looks 10×40; pinned to the right edge with a fixed size.
        let mut bar = Layer::rectangle(Frame::new(150.0, 30.0, 40.0, 10.0));
        bar.rotation = 90.0;
        bar.resizing = Resizing { right: true, fix_width: true, fix_height: true, top: true, ..Default::default() };
        let gap_before = 200.0 - bar.bounds_in(kurbo::Affine::IDENTITY).unwrap().x1;
        let spacer = Layer::rectangle(Frame::new(0.0, 0.0, 200.0, 100.0));
        let mut g = Layer::group(vec![spacer, bar], Frame::new(0.0, 0.0, 200.0, 100.0));
        g.reframe(Rect::new(0.0, 0.0, 300.0, 100.0));
        let bar = &g.children().unwrap()[1];
        let b = bar.bounds_in(kurbo::Affine::IDENTITY).unwrap();
        assert!((300.0 - b.x1 - gap_before).abs() < 1e-6, "keeps its gap to the right edge: {b:?}");
        assert!((b.width() - 10.0).abs() < 1e-6 && (b.height() - 40.0).abs() < 1e-6, "and its size: {b:?}");
        assert_eq!(bar.rotation, 90.0);
        // Stretching across scales it along its on-screen width.
        let mut stretchy = Layer::rectangle(Frame::new(80.0, 30.0, 40.0, 10.0));
        stretchy.rotation = 90.0;
        stretchy.resizing = Resizing { left: true, right: true, top: true, fix_height: true, ..Default::default() };
        let spacer = Layer::rectangle(Frame::new(0.0, 0.0, 200.0, 100.0));
        let mut g = Layer::group(vec![spacer, stretchy], Frame::new(0.0, 0.0, 200.0, 100.0));
        g.reframe(Rect::new(0.0, 0.0, 400.0, 100.0));
        let b = g.children().unwrap()[1].bounds_in(kurbo::Affine::IDENTITY).unwrap();
        assert!((b.width() - 210.0).abs() < 1e-6, "pinned both sides: 10 + 200 wide: {b:?}");
    }
}
