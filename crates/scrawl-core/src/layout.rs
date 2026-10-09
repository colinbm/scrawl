//! Arranging layers: stacks (groups that lay their children out in a row or column, like
//! stacks), distribute and tidy.
//!
//! A stack's flow follows the layer list from top to bottom, which is the reverse of child order
//! (the last child is the topmost layer).

use kurbo::{Affine, Rect, Vec2};
use serde::{Deserialize, Serialize};

use crate::document::Page;
use crate::ops::{move_by, normalize_selection};
use crate::snap::Axis;
use crate::{Layer, LayerId, LayerKind, geometry::union_rects};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StackAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    /// `Axis::X` lays children out left to right, `Axis::Y` top to bottom.
    pub direction: Axis,
    pub spacing: f64,
    #[serde(default)]
    pub align: StackAlign,
}

impl Default for Stack {
    fn default() -> Self {
        Self { direction: Axis::Y, spacing: 8.0, align: StackAlign::Start }
    }
}

fn along(r: Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.x0, r.x1),
        Axis::Y => (r.y0, r.y1),
    }
}

fn vec(axis: Axis, along: f64, across: f64) -> Vec2 {
    match axis {
        Axis::X => Vec2::new(along, across),
        Axis::Y => Vec2::new(across, along),
    }
}

fn cross(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    }
}

impl Layer {
    /// The group's stack settings, if it is a stack.
    pub fn stack(&self) -> Option<&Stack> {
        match &self.kind {
            LayerKind::Group(g) => g.stack.as_ref(),
            _ => None,
        }
    }

    /// Lays out a stack group's visible children and refits the group. Returns whether anything
    /// moved.
    fn layout_stack(&mut self) -> bool {
        let LayerKind::Group(g) = &mut self.kind else { return false };
        let Some(stack) = g.stack else { return false };
        let dir = stack.direction;
        let bounds: Vec<Option<Rect>> =
            g.children.iter().map(|c| if c.visible { c.bounds_in(Affine::IDENTITY) } else { None }).collect();
        let thickness = bounds.iter().flatten().map(|r| along(*r, cross(dir))).map(|(a, b)| b - a).fold(0.0, f64::max);
        let mut cursor = 0.0;
        let mut changed = false;
        for (c, b) in g.children.iter_mut().zip(&bounds).rev() {
            let Some(b) = b else { continue };
            let (a0, a1) = along(*b, dir);
            let (c0, c1) = along(*b, cross(dir));
            let across = match stack.align {
                StackAlign::Start => 0.0,
                StackAlign::Center => (thickness - (c1 - c0)) / 2.0,
                StackAlign::End => thickness - (c1 - c0),
            };
            let d = vec(dir, cursor - a0, across - c0);
            if d.hypot() > 1e-9 {
                c.frame.translate(d);
                changed = true;
            }
            cursor += a1 - a0 + stack.spacing;
        }
        // Size to the content, keeping the top-left where it is.
        let content = union_rects(g.children.iter().filter(|c| c.visible).filter_map(|c| c.bounds_in(Affine::IDENTITY)));
        if let Some(u) = content
            && (self.frame.width, self.frame.height) != (u.x1, u.y1)
        {
            self.frame.width = u.x1;
            self.frame.height = u.y1;
            changed = true;
        }
        changed
    }

    /// Lays out every stack in this subtree, innermost first, skipping stacks that hold a layer
    /// in `skip` (being dragged). Returns whether anything changed.
    pub fn apply_stacks(&mut self, skip: &[LayerId]) -> bool {
        let mut changed = false;
        if let Some(children) = self.children_mut() {
            for c in children.iter_mut() {
                changed |= c.apply_stacks(skip);
            }
        }
        let holds_skipped = self.children().is_some_and(|cs| cs.iter().any(|c| skip.contains(&c.id)));
        if self.stack().is_some() && !holds_skipped {
            changed |= self.layout_stack();
        }
        changed
    }
}

/// Lays out every stack on the page (see [`Layer::apply_stacks`]).
pub fn apply_stacks(page: &mut Page, skip: &[LayerId]) -> bool {
    let mut changed = false;
    for l in &mut page.layers {
        changed |= l.apply_stacks(skip);
    }
    changed
}

/// Re-orders a stack's children to match where they are on the canvas, so dropping a dragged
/// child puts it in the flow where it was dropped.
pub fn order_stack_by_position(page: &mut Page, id: LayerId) {
    let Some(layer) = page.find_mut(id) else { return };
    let Some(dir) = layer.stack().map(|s| s.direction) else { return };
    let Some(children) = layer.children_mut() else { return };
    let center = |c: &Layer| c.bounds_in(Affine::IDENTITY).map_or(0.0, |b| along(b, dir).0 + along(b, dir).1);
    // Flow runs from the last child, so sort descending.
    children.sort_by(|a, b| center(b).total_cmp(&center(a)));
}

/// Groups the selection into a stack, choosing the direction and spacing from how the layers are
/// currently arranged. Returns the stack's id.
pub fn make_stack(page: &mut Page, ids: &[LayerId]) -> Option<LayerId> {
    let ids = normalize_selection(page, ids);
    let rects: Vec<Rect> = ids.iter().filter_map(|&id| page.world_bounds(id)).collect();
    let spread = |axis| {
        let cs: Vec<f64> = rects.iter().map(|r| along(*r, axis)).map(|(a, b)| (a + b) / 2.0).collect();
        cs.iter().copied().fold(f64::MIN, f64::max) - cs.iter().copied().fold(f64::MAX, f64::min)
    };
    let direction = if spread(Axis::X) > spread(Axis::Y) { Axis::X } else { Axis::Y };
    let spacing = average_gap(&rects, direction).unwrap_or(8.0);
    let gid = crate::ops::group(page, &ids)?;
    if let Some(LayerKind::Group(g)) = page.find_mut(gid).map(|l| &mut l.kind) {
        g.stack = Some(Stack { direction, spacing, align: StackAlign::Start });
    }
    order_stack_by_position(page, gid);
    if let Some(l) = page.find_mut(gid) {
        l.layout_stack();
    }
    Some(gid)
}

/// The rounded average of the positive gaps between consecutive rects along `axis`.
fn average_gap(rects: &[Rect], axis: Axis) -> Option<f64> {
    let mut sorted: Vec<(f64, f64)> = rects.iter().map(|r| along(*r, axis)).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let gaps: Vec<f64> = sorted.windows(2).map(|w| w[1].0 - w[0].1).filter(|g| *g > 0.0).collect();
    (!gaps.is_empty()).then(|| (gaps.iter().sum::<f64>() / gaps.len() as f64).round())
}

/// Spaces three or more layers evenly between the first and last along `axis`.
pub fn distribute(page: &mut Page, ids: &[LayerId], axis: Axis) {
    let ids = normalize_selection(page, ids);
    let mut items: Vec<(LayerId, Rect)> = ids.iter().filter_map(|&id| Some((id, page.world_bounds(id)?))).collect();
    if items.len() < 3 {
        return;
    }
    items.sort_by(|a, b| along(a.1, axis).0.total_cmp(&along(b.1, axis).0));
    let start = along(items[0].1, axis).0;
    let end = items.iter().map(|(_, r)| along(*r, axis).1).fold(f64::MIN, f64::max);
    let total: f64 = items.iter().map(|(_, r)| along(*r, axis)).map(|(a, b)| b - a).sum();
    let gap = (end - start - total) / (items.len() - 1) as f64;
    let mut cursor = start;
    for (id, r) in items {
        let (a, b) = along(r, axis);
        move_by(page, &[id], vec(axis, cursor - a, 0.0));
        cursor += b - a + gap;
    }
}

/// Arranges layers into tidy rows with uniform spacing, keeping their reading order. With no
/// `spacing`, uses the average gap already between them.
pub fn tidy(page: &mut Page, ids: &[LayerId], spacing: Option<f64>) {
    let ids = normalize_selection(page, ids);
    let mut items: Vec<(LayerId, Rect)> = ids.iter().filter_map(|&id| Some((id, page.world_bounds(id)?))).collect();
    let Some(bounds) = union_rects(items.iter().map(|(_, r)| *r)) else { return };
    // Rows: a layer joins the current row if its center is above the row's bottom.
    items.sort_by(|a, b| a.1.center().y.total_cmp(&b.1.center().y));
    let mut rows: Vec<Vec<(LayerId, Rect)>> = vec![];
    for it in items {
        match rows.last_mut() {
            Some(row) if it.1.center().y < row.iter().map(|(_, r)| r.y1).fold(f64::MIN, f64::max) => row.push(it),
            _ => rows.push(vec![it]),
        }
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.1.x0.total_cmp(&b.1.x0));
    }
    let spacing = spacing
        .or_else(|| {
            let gaps: Vec<f64> = rows.iter().filter_map(|r| average_gap(&r.iter().map(|(_, b)| *b).collect::<Vec<_>>(), Axis::X)).collect();
            (!gaps.is_empty()).then(|| (gaps.iter().sum::<f64>() / gaps.len() as f64).round())
        })
        .unwrap_or(10.0);
    let mut y = bounds.y0;
    for row in rows {
        let mut x = bounds.x0;
        let height = row.iter().map(|(_, r)| r.height()).fold(0.0, f64::max);
        for (id, r) in row {
            move_by(page, &[id], Vec2::new(x - r.x0, y - r.y0));
            x += r.width() + spacing;
        }
        y += height + spacing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Frame, ops};

    fn page_with(rects: &[(f64, f64, f64, f64)]) -> (Document, Vec<LayerId>) {
        let mut doc = Document::default();
        let ids = rects.iter().map(|&(x, y, w, h)| ops::insert_drawn(&mut doc.pages[0], Layer::rectangle(Frame::new(x, y, w, h)))).collect();
        (doc, ids)
    }

    fn origins(page: &Page, ids: &[LayerId]) -> Vec<(f64, f64)> {
        ids.iter().map(|&id| page.world_bounds(id).unwrap().origin()).map(|p| (p.x, p.y)).collect()
    }

    #[test]
    fn distributes_evenly() {
        let (mut doc, ids) = page_with(&[(0.0, 0.0, 10.0, 10.0), (15.0, 0.0, 20.0, 10.0), (90.0, 0.0, 10.0, 10.0)]);
        distribute(&mut doc.pages[0], &ids, Axis::X);
        // 100 wide, 40 of layers, two gaps of 30.
        assert_eq!(origins(&doc.pages[0], &ids), vec![(0.0, 0.0), (40.0, 0.0), (90.0, 0.0)]);
    }

    #[test]
    fn tidy_makes_rows_with_uniform_spacing() {
        let (mut doc, ids) =
            page_with(&[(0.0, 0.0, 10.0, 10.0), (17.0, 3.0, 10.0, 10.0), (30.0, -2.0, 10.0, 10.0), (4.0, 30.0, 10.0, 20.0)]);
        tidy(&mut doc.pages[0], &ids, Some(5.0));
        assert_eq!(origins(&doc.pages[0], &ids), vec![(0.0, -2.0), (15.0, -2.0), (30.0, -2.0), (0.0, 13.0)]);
    }

    #[test]
    fn stack_lays_out_in_layer_list_order() {
        let (mut doc, ids) = page_with(&[(0.0, 100.0, 50.0, 10.0), (0.0, 0.0, 30.0, 20.0)]);
        let page = &mut doc.pages[0];
        // The second rect is higher on the canvas, so it comes first.
        let gid = make_stack(page, &ids).unwrap();
        let stack = *page.find(gid).unwrap().stack().unwrap();
        assert_eq!(stack.direction, Axis::Y);
        assert_eq!(stack.spacing, 80.0);
        assert_eq!(origins(page, &ids), vec![(0.0, 100.0), (0.0, 0.0)]);

        // Tighten the spacing and center the items.
        if let LayerKind::Group(g) = &mut page.find_mut(gid).unwrap().kind {
            g.stack = Some(Stack { spacing: 4.0, align: StackAlign::Center, ..stack });
        }
        assert!(apply_stacks(page, &[]));
        assert_eq!(origins(page, &ids), vec![(0.0, 24.0), (10.0, 0.0)]);
        assert_eq!(page.find(gid).unwrap().frame.height, 34.0);
        // Stable once laid out.
        assert!(!apply_stacks(page, &[]));
    }

    #[test]
    fn dragged_children_pause_their_stack() {
        let (mut doc, ids) = page_with(&[(0.0, 0.0, 10.0, 10.0), (0.0, 20.0, 10.0, 10.0)]);
        let page = &mut doc.pages[0];
        make_stack(page, &ids).unwrap();
        move_by(page, &[ids[0]], Vec2::new(0.0, 50.0));
        assert!(!apply_stacks(page, &[ids[0]]));
        assert!(apply_stacks(page, &[]));
    }

    #[test]
    fn dropping_reorders_the_stack() {
        let (mut doc, ids) = page_with(&[(0.0, 0.0, 10.0, 10.0), (0.0, 20.0, 10.0, 10.0), (0.0, 40.0, 10.0, 10.0)]);
        let page = &mut doc.pages[0];
        let gid = make_stack(page, &ids).unwrap();
        // Drag the first item below the last, then drop.
        move_by(page, &[ids[0]], Vec2::new(0.0, 45.0));
        order_stack_by_position(page, gid);
        apply_stacks(page, &[]);
        assert_eq!(origins(page, &ids), vec![(0.0, 40.0), (0.0, 0.0), (0.0, 20.0)]);
    }
}
