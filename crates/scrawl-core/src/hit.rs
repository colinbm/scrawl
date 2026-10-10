//! Hit testing in page space.

use kurbo::{Affine, ParamCurveNearest, Point, Rect, Shape};

use crate::{FillRule, Layer, LayerId, Page};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A selectable layer.
    Layer(LayerId),
    /// Empty space inside an artboard. This doesn't select the artboard (that's done
    /// via its title), but drawing and marquee selection treat it as "inside" the artboard.
    ArtboardBackground(LayerId),
    Nothing,
}

/// Finds what's under `point`.
///
/// Clicking inside a group selects the group, unless `deep` is set (Cmd-click) or the
/// group is in `entered` (the user has drilled into it), in which case its children are tested.
/// `tolerance` is in page units and widens thin strokes and lines.
pub fn hit_test(page: &Page, point: Point, tolerance: f64, deep: bool, entered: &[LayerId]) -> Hit {
    for layer in page.layers.iter().rev() {
        if !layer.visible {
            continue;
        }
        if layer.is_artboard() {
            let t = layer.local_transform();
            if !layer.frame.to_rect().contains(point) {
                continue;
            }
            for child in layer.children().into_iter().flatten().rev() {
                if let Some(id) = hit_layer(child, t, point, tolerance, deep, entered) {
                    return Hit::Layer(id);
                }
            }
            return if layer.locked { Hit::Nothing } else { Hit::ArtboardBackground(layer.id) };
        }
        if let Some(id) = hit_layer(layer, Affine::IDENTITY, point, tolerance, deep, entered) {
            return Hit::Layer(id);
        }
    }
    Hit::Nothing
}

fn hit_layer(
    layer: &Layer,
    parent: Affine,
    point: Point,
    tolerance: f64,
    deep: bool,
    entered: &[LayerId],
) -> Option<LayerId> {
    if !layer.visible || layer.locked {
        return None;
    }
    let t = parent * layer.local_transform();
    // A shape group is picked by its combined outline, unless the user has drilled into it.
    let shape_leaf = layer.is_shape_group() && !deep && !entered.contains(&layer.id);
    if let Some(children) = layer.children().filter(|_| !shape_leaf) {
        let mut hits = children.iter().rev().filter_map(|c| hit_layer(c, t, point, tolerance, deep, entered));
        let child = hits.next()?;
        return Some(if deep || entered.contains(&layer.id) { child } else { layer.id });
    }
    let local = t.inverse() * point;
    let path = layer.local_path()?;
    // Borders widen the hit area.
    let tol = tolerance + layer.style.border_outset();
    let winding = path.winding(local);
    let inside = match layer.fill_rule() {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    };
    if layer.is_closed() && inside {
        return Some(layer.id);
    }
    path.segments()
        .any(|seg| seg.nearest(local, 1e-3).distance_sq <= tol * tol)
        .then_some(layer.id)
}

/// Layers whose bounds intersect `rect` (page space), for marquee selection.
///
/// Candidates are root layers and direct children of artboards. Artboards themselves are only
/// selected when the marquee fully encloses them.
pub fn layers_in_rect(page: &Page, rect: Rect) -> Vec<LayerId> {
    let mut out = vec![];
    let intersects = |b: Rect| b.intersect(rect).area() > 0.0 || rect.contains(b.center());
    for layer in &page.layers {
        if !layer.visible || layer.locked {
            continue;
        }
        let Some(b) = layer.bounds_in(Affine::IDENTITY) else { continue };
        if layer.is_artboard() {
            if rect.contains(b.origin()) && rect.contains(Point::new(b.x1, b.y1)) {
                out.push(layer.id);
                continue;
            }
            let t = layer.local_transform();
            for child in layer.children().into_iter().flatten() {
                if child.visible && !child.locked && child.bounds_in(t).is_some_and(intersects) {
                    out.push(child.id);
                }
            }
        } else if intersects(b) {
            out.push(layer.id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, ops};

    #[test]
    fn groups_select_as_a_unit_unless_deep() {
        let mut page = Page::new("P");
        let ab = ops::insert_drawn(&mut page, Layer::artboard("A", Frame::new(0.0, 0.0, 500.0, 500.0)));
        let a = ops::insert_drawn(&mut page, Layer::rectangle(Frame::new(10.0, 10.0, 50.0, 50.0)));
        let b = ops::insert_drawn(&mut page, Layer::oval(Frame::new(100.0, 10.0, 50.0, 50.0)));
        let g = ops::group(&mut page, &[a, b]).unwrap();
        let p = Point::new(20.0, 20.0);
        assert_eq!(hit_test(&page, p, 0.0, false, &[]), Hit::Layer(g));
        assert_eq!(hit_test(&page, p, 0.0, true, &[]), Hit::Layer(a));
        assert_eq!(hit_test(&page, p, 0.0, false, &[g]), Hit::Layer(a));
        // The oval's bounding-box corner is outside the ellipse.
        assert_eq!(hit_test(&page, Point::new(101.0, 11.0), 0.0, true, &[]), Hit::ArtboardBackground(ab));
        assert_eq!(hit_test(&page, Point::new(600.0, 0.0), 0.0, false, &[]), Hit::Nothing);
    }

    #[test]
    fn lines_hit_within_tolerance() {
        let mut page = Page::new("P");
        let l = ops::insert_drawn(&mut page, Layer::line(Point::new(0.0, 0.0), Point::new(100.0, 0.0)));
        assert_eq!(hit_test(&page, Point::new(50.0, 3.0), 3.0, false, &[]), Hit::Layer(l));
        assert_eq!(hit_test(&page, Point::new(50.0, 6.0), 3.0, false, &[]), Hit::Nothing);
    }

    #[test]
    fn marquee() {
        let mut page = Page::new("P");
        let ab = ops::insert_drawn(&mut page, Layer::artboard("A", Frame::new(0.0, 0.0, 100.0, 100.0)));
        let r = ops::insert_drawn(&mut page, Layer::rectangle(Frame::new(10.0, 10.0, 10.0, 10.0)));
        assert_eq!(layers_in_rect(&page, Rect::new(5.0, 5.0, 12.0, 12.0)), vec![r]);
        assert_eq!(layers_in_rect(&page, Rect::new(-1.0, -1.0, 101.0, 101.0)), vec![ab]);
    }
}
