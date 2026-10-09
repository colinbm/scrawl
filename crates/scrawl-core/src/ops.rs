//! Editing operations on a page. Each takes the ids it acts on and leaves the tree valid.

use kurbo::{Affine, Point, Vec2};

use crate::{
    Frame, Layer, LayerId, LayerKind, Page,
    document::Location,
};

/// Drops ids that are missing, or whose ancestor is also in the list (acting on the ancestor
/// already covers them). Preserves order.
pub fn normalize_selection(page: &Page, ids: &[LayerId]) -> Vec<LayerId> {
    ids.iter()
        .copied()
        .filter(|&id| {
            page.ancestors(id).is_some_and(|anc| !anc.iter().any(|a| ids.contains(a)))
        })
        .fold(vec![], |mut acc, id| {
            if !acc.contains(&id) {
                acc.push(id);
            }
            acc
        })
}

/// Converts a page-space delta into the frame space of `id`.
fn delta_in_parent(page: &Page, id: LayerId, d: Vec2) -> Option<Vec2> {
    let inv = page.parent_transform(id)?.inverse();
    Some(inv * Point::new(d.x, d.y) - inv * Point::ORIGIN)
}

/// Translates layers by `d` in page space.
pub fn move_by(page: &mut Page, ids: &[LayerId], d: Vec2) {
    for id in normalize_selection(page, ids) {
        if let Some(local) = delta_in_parent(page, id, d)
            && let Some(l) = page.find_mut(id)
        {
            l.frame.translate(local);
        }
        refit_parent(page, id);
    }
}

fn refit_parent(page: &mut Page, id: LayerId) {
    if let Some(Some(parent)) = page.parent(id) {
        page.refit_ancestor_groups(parent);
    }
}

pub fn delete(page: &mut Page, ids: &[LayerId]) {
    for id in normalize_selection(page, ids) {
        let parent = page.parent(id).flatten();
        page.remove(id);
        if let Some(parent) = parent {
            remove_empty_groups(page, parent);
        }
    }
}

/// Removes `id` if it's an empty group, walking up through now-empty ancestors; otherwise
/// re-fits the remaining groups.
fn remove_empty_groups(page: &mut Page, id: LayerId) {
    let empty_group = page.find(id).is_some_and(|l| l.is_group() && l.children().is_some_and(Vec::is_empty));
    if empty_group {
        let parent = page.parent(id).flatten();
        page.remove(id);
        if let Some(p) = parent {
            remove_empty_groups(page, p);
        }
    } else {
        page.refit_ancestor_groups(id);
    }
}

/// Duplicates each layer directly above itself. Returns the new ids.
pub fn duplicate(page: &mut Page, ids: &[LayerId]) -> Vec<LayerId> {
    let mut out = vec![];
    for id in normalize_selection(page, ids) {
        let (Some(loc), Some(layer)) = (page.location(id), page.find(id)) else { continue };
        let mut copy = layer.duplicate();
        copy.name = layer.name.clone();
        out.push(copy.id);
        let _ = page.insert(Location { index: loc.index + 1, ..loc }, copy);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reorder {
    Forward,
    Backward,
    ToFront,
    ToBack,
}

/// Changes z-order among siblings.
pub fn reorder(page: &mut Page, ids: &[LayerId], how: Reorder) {
    let mut ids = normalize_selection(page, ids);
    // Process in an order where moving one layer can't jump over another selected one.
    ids.sort_by_key(|&id| page.location(id).map(|l| l.index).unwrap_or(0));
    if matches!(how, Reorder::Forward | Reorder::ToFront) {
        ids.reverse();
    }
    // Selected layers never pass each other: each one's movement is bounded by where the
    // previously placed one landed, per parent.
    let mut bounds: Vec<(Option<LayerId>, usize)> = vec![];
    for id in ids {
        let Some(loc) = page.location(id) else { continue };
        let Some(siblings) = page.siblings_mut(loc.parent) else { continue };
        let limit = match bounds.iter().find(|(p, _)| *p == loc.parent) {
            Some(&(_, limit)) => limit,
            None if matches!(how, Reorder::Forward | Reorder::ToFront) => siblings.len() - 1,
            None => 0,
        };
        let target = match how {
            Reorder::Forward => (loc.index + 1).min(limit),
            Reorder::Backward => loc.index.saturating_sub(1).max(limit),
            Reorder::ToFront | Reorder::ToBack => limit,
        };
        let next = match how {
            Reorder::Forward | Reorder::ToFront => target.saturating_sub(1),
            Reorder::Backward | Reorder::ToBack => target + 1,
        };
        bounds.retain(|(p, _)| *p != loc.parent);
        bounds.push((loc.parent, next));
        let layer = siblings.remove(loc.index);
        siblings.insert(target, layer);
    }
}

/// Wraps layers sharing the first layer's parent into a new group placed where the topmost one
/// was. Layers with a different parent are ignored. Returns the group id.
pub fn group(page: &mut Page, ids: &[LayerId]) -> Option<LayerId> {
    let ids = normalize_selection(page, ids);
    let parent = page.parent(*ids.first()?)?;
    let mut members: Vec<(usize, LayerId)> = ids
        .iter()
        .filter(|&&id| page.parent(id) == Some(parent))
        .filter_map(|&id| Some((page.location(id)?.index, id)))
        .collect();
    members.sort();
    let top_index = members.last()?.0;
    let insert_at = top_index + 1 - members.len();

    let mut children: Vec<Layer> = members.iter().filter_map(|&(_, id)| Some(page.remove(id)?.1)).collect();
    // Express children relative to a group at the parent origin, then fit it.
    let bounds = crate::geometry::union_rects(children.iter().filter_map(|c| c.bounds_in(Affine::IDENTITY)))?;
    for c in &mut children {
        c.frame.translate(-bounds.origin().to_vec2());
    }
    let group = Layer::group(children, Frame::from_rect(bounds));
    let gid = group.id;
    page.insert(Location { parent, index: insert_at }, group).ok()?;
    if let Some(p) = parent {
        page.refit_ancestor_groups(p);
    }
    Some(gid)
}

/// Dissolves a group, moving its children into its parent at the same on-screen position.
/// Returns the released children's ids.
pub fn ungroup(page: &mut Page, id: LayerId) -> Vec<LayerId> {
    if !page.find(id).is_some_and(Layer::is_group) {
        return vec![];
    }
    let Some((loc, group)) = page.remove(id) else { return vec![] };
    let gt = group.local_transform();
    let LayerKind::Group(g) = group.kind else { unreachable!() };
    let mut out = vec![];
    for (i, mut child) in g.children.into_iter().enumerate() {
        let center = gt * child.local_transform() * child.frame.local_rect().center();
        child.rotation += group.rotation;
        child.flip_horizontal ^= group.flip_horizontal;
        child.flip_vertical ^= group.flip_vertical;
        child.opacity *= group.opacity;
        child.frame.x = center.x - child.frame.width / 2.0;
        child.frame.y = center.y - child.frame.height / 2.0;
        out.push(child.id);
        let _ = page.insert(Location { index: loc.index + i, ..loc }, child);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

/// Aligns layers to the edge/center of their combined bounds. A single layer inside an
/// artboard is aligned to the artboard instead.
pub fn align(page: &mut Page, ids: &[LayerId], how: Align) {
    let ids = normalize_selection(page, ids);
    let target = if let [only] = ids[..] {
        let Some(Some(parent)) = page.parent(only) else { return };
        page.world_bounds(parent)
    } else {
        page.selection_bounds(&ids)
    };
    let Some(target) = target else { return };
    for id in ids {
        let Some(b) = page.world_bounds(id) else { continue };
        let d = match how {
            Align::Left => Vec2::new(target.x0 - b.x0, 0.0),
            Align::HCenter => Vec2::new(target.center().x - b.center().x, 0.0),
            Align::Right => Vec2::new(target.x1 - b.x1, 0.0),
            Align::Top => Vec2::new(0.0, target.y0 - b.y0),
            Align::VCenter => Vec2::new(0.0, target.center().y - b.center().y),
            Align::Bottom => Vec2::new(0.0, target.y1 - b.y1),
        };
        move_by(page, &[id], d);
    }
}

/// Inserts a newly drawn layer whose frame is in page space. Non-artboard layers drawn inside an
/// artboard become its children, with their frame converted to artboard space.
pub fn insert_drawn(page: &mut Page, mut layer: Layer) -> LayerId {
    let id = layer.id;
    let parent = if layer.is_artboard() { None } else { page.artboard_at(layer.frame.center()) };
    if let Some(ab) = parent.and_then(|p| page.find(p)) {
        layer.frame.translate(-ab.frame.origin().to_vec2());
    }
    let index = page.siblings(parent).map_or(0, Vec::len);
    let _ = page.insert(Location { parent, index }, layer);
    id
}

/// Adds a layer, whose frame is in page space, on top of the page root.
pub fn insert_root(page: &mut Page, layer: Layer) -> LayerId {
    let id = layer.id;
    page.layers.push(layer);
    id
}

/// Rotation and flips accumulated from the page root down to (and including) `parent`.
fn accumulated_orientation(page: &Page, parent: Option<LayerId>) -> (f64, bool, bool) {
    let Some(parent) = parent else { return (0.0, false, false) };
    let chain = page.ancestors(parent).unwrap_or_default().into_iter().chain([parent]);
    chain.filter_map(|id| page.find(id)).fold((0.0, false, false), |(r, h, v), l| {
        (r + l.rotation, h ^ l.flip_horizontal, v ^ l.flip_vertical)
    })
}

/// Re-expresses `layer`, currently positioned by `from` (its parent space → page), in the space
/// of `to_parent`, keeping its on-screen center, rotation and flips.
fn rebase(page: &Page, mut layer: Layer, from: Option<LayerId>, to_parent: Option<LayerId>) -> Option<Layer> {
    let center = page.space_transform(from)? * layer.local_transform() * layer.frame.local_rect().center();
    let c = page.space_transform(to_parent)?.inverse() * center;
    let (r0, h0, v0) = accumulated_orientation(page, from);
    let (r1, h1, v1) = accumulated_orientation(page, to_parent);
    layer.rotation += r0 - r1;
    layer.flip_horizontal ^= h0 ^ h1;
    layer.flip_vertical ^= v0 ^ v1;
    layer.frame.x = c.x - layer.frame.width / 2.0;
    layer.frame.y = c.y - layer.frame.height / 2.0;
    Some(layer)
}

/// A copy of layer `id` with its frame expressed in page space, e.g. for the clipboard.
pub fn page_space_copy(page: &Page, id: LayerId) -> Option<Layer> {
    let parent = page.parent(id)?;
    rebase(page, page.find(id)?.clone(), parent, None)
}

/// Moves a layer to `index` within `parent`'s children (`None` = page root), keeping its
/// on-screen position. `index` refers to positions *before* the layer is removed. Returns false
/// if the move is invalid (into itself, an artboard into a container, a non-container parent).
pub fn move_to(page: &mut Page, id: LayerId, parent: Option<LayerId>, index: usize) -> bool {
    let Some(layer) = page.find(id) else { return false };
    if let Some(p) = parent {
        let into_self = p == id || page.ancestors(p).is_none_or(|a| a.contains(&id));
        let container = page.find(p).is_some_and(|l| l.children().is_some());
        if into_self || !container || layer.is_artboard() {
            return false;
        }
    }
    let Some(old) = page.location(id) else { return false };
    let Some(moved) = rebase(page, layer.clone(), old.parent, parent) else { return false };
    let index = if old.parent == parent && old.index < index { index - 1 } else { index };
    page.remove(id);
    if page.insert(Location { parent, index }, moved).is_err() {
        return false;
    }
    if let Some(p) = old.parent {
        remove_empty_groups(page, p);
    }
    page.refit_ancestor_groups(id);
    true
}

/// Moves layers into the artboard under their center (or out to the page if they're dragged
/// off), keeping their on-screen position. The usual drag-into-artboard behavior. Only
/// direct children of the page or of an artboard are affected.
pub fn reparent_to_artboards(page: &mut Page, ids: &[LayerId]) {
    for id in normalize_selection(page, ids) {
        let Some(layer) = page.find(id) else { continue };
        let Some(current) = page.parent(id) else { continue };
        if layer.is_artboard() || current.is_some_and(|p| !page.find(p).is_some_and(Layer::is_artboard)) {
            continue;
        }
        let Some(center) = page.world_bounds(id).map(|b| b.center()) else { continue };
        let target = page.artboard_at(center);
        if target != current {
            let index = page.siblings(target).map_or(0, Vec::len);
            move_to(page, id, target, index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect;

    fn page_with_rects(n: usize) -> (Page, Vec<LayerId>) {
        let mut page = Page::new("P");
        let ids = (0..n)
            .map(|i| insert_drawn(&mut page, Layer::rectangle(Frame::new(i as f64 * 20.0, 0.0, 10.0, 10.0))))
            .collect();
        (page, ids)
    }

    fn order(page: &Page) -> Vec<LayerId> {
        page.layers.iter().map(|l| l.id).collect()
    }

    #[test]
    fn group_then_ungroup_preserves_positions() {
        let (mut page, ids) = page_with_rects(3);
        let before: Vec<_> = ids.iter().map(|&id| page.world_bounds(id).unwrap()).collect();
        let g = group(&mut page, &ids[..2]).unwrap();
        assert_eq!(page.world_bounds(g).unwrap(), Rect::new(0.0, 0.0, 30.0, 10.0));
        assert_eq!(order(&page), vec![g, ids[2]]);
        for (id, b) in ids.iter().zip(&before) {
            assert_eq!(page.world_bounds(*id).unwrap(), *b);
        }
        page.find_mut(g).unwrap().rotation = 90.0;
        let rotated: Vec<_> = ids[..2].iter().map(|&id| page.world_bounds(id).unwrap()).collect();
        let released = ungroup(&mut page, g);
        assert_eq!(released, ids[..2]);
        for (id, b) in ids.iter().zip(&rotated) {
            let got = page.world_bounds(*id).unwrap();
            assert!((got.center() - b.center()).hypot() < 1e-9 && (got.area() - b.area()).abs() < 1e-9);
        }
    }

    #[test]
    fn reorder_moves_within_siblings() {
        let (mut page, ids) = page_with_rects(3);
        reorder(&mut page, &[ids[0]], Reorder::ToFront);
        assert_eq!(order(&page), vec![ids[1], ids[2], ids[0]]);
        reorder(&mut page, &[ids[0]], Reorder::Backward);
        assert_eq!(order(&page), vec![ids[1], ids[0], ids[2]]);
        reorder(&mut page, &[ids[1], ids[0]], Reorder::Forward);
        assert_eq!(order(&page), vec![ids[2], ids[1], ids[0]]);
        // Already at the top together: nothing moves, and they don't swap.
        reorder(&mut page, &[ids[1], ids[0]], Reorder::Forward);
        assert_eq!(order(&page), vec![ids[2], ids[1], ids[0]]);
        reorder(&mut page, &[ids[0], ids[1]], Reorder::ToBack);
        assert_eq!(order(&page), vec![ids[1], ids[0], ids[2]]);
    }

    #[test]
    fn drawn_layers_land_in_artboards() {
        let mut page = Page::new("P");
        let ab = insert_drawn(&mut page, Layer::artboard("A", Frame::new(100.0, 100.0, 200.0, 200.0)));
        let r = insert_drawn(&mut page, Layer::rectangle(Frame::new(150.0, 150.0, 10.0, 10.0)));
        assert_eq!(page.parent(r), Some(Some(ab)));
        assert_eq!(page.find(r).unwrap().frame.origin(), Point::new(50.0, 50.0));
        // Drag it off the artboard and it moves to the page root.
        move_by(&mut page, &[r], Vec2::new(-200.0, 0.0));
        reparent_to_artboards(&mut page, &[r]);
        assert_eq!(page.parent(r), Some(None));
        assert_eq!(page.world_bounds(r).unwrap().origin(), Point::new(-50.0, 150.0));
    }

    #[test]
    fn move_to_keeps_position_and_rejects_cycles() {
        let (mut page, ids) = page_with_rects(3);
        let g = group(&mut page, &ids[..2]).unwrap();
        page.find_mut(g).unwrap().rotation = 90.0;
        let before = page.world_bounds(ids[2]).unwrap();
        assert!(move_to(&mut page, ids[2], Some(g), 0));
        assert_eq!(page.parent(ids[2]), Some(Some(g)));
        let after = page.world_bounds(ids[2]).unwrap();
        assert!((after.center() - before.center()).hypot() < 1e-9);
        assert!((after.area() - before.area()).abs() < 1e-9);
        assert!(!move_to(&mut page, g, Some(ids[0]), 0));
        assert!(!move_to(&mut page, g, Some(g), 0));
        // Reorder within the root: index counts positions before removal.
        let (mut page, ids) = page_with_rects(3);
        assert!(move_to(&mut page, ids[0], None, 3));
        assert_eq!(order(&page), vec![ids[1], ids[2], ids[0]]);
    }

    #[test]
    fn page_space_copy_flattens_transform() {
        let mut page = Page::new("P");
        insert_drawn(&mut page, Layer::artboard("A", Frame::new(100.0, 100.0, 200.0, 200.0)));
        let r = insert_drawn(&mut page, Layer::rectangle(Frame::new(150.0, 150.0, 10.0, 10.0)));
        assert_eq!(page_space_copy(&page, r).unwrap().frame, Frame::new(150.0, 150.0, 10.0, 10.0));
    }

    #[test]
    fn deleting_last_child_removes_group() {
        let (mut page, ids) = page_with_rects(2);
        let g = group(&mut page, &ids).unwrap();
        delete(&mut page, &ids);
        assert!(!page.contains(g));
    }

    #[test]
    fn align_left() {
        let (mut page, ids) = page_with_rects(3);
        move_by(&mut page, &[ids[1]], Vec2::new(0.0, 5.0));
        align(&mut page, &ids, Align::Left);
        for id in ids {
            assert_eq!(page.world_bounds(id).unwrap().x0, 0.0);
        }
    }
}
