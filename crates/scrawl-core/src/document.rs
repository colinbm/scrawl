use kurbo::{Affine, Rect};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Layer, LayerId, geometry::union_rects};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageId(pub Uuid);

impl PageId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub pages: Vec<Page>,
}

impl Default for Document {
    fn default() -> Self {
        Self { pages: vec![Page::new("Page 1")] }
    }
}

impl Document {
    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn page_mut(&mut self, id: PageId) -> Option<&mut Page> {
        self.pages.iter_mut().find(|p| p.id == id)
    }

    /// A page name not yet used in this document, e.g. "Page 3".
    pub fn next_page_name(&self) -> String {
        (1..)
            .map(|n| format!("Page {n}"))
            .find(|name| self.pages.iter().all(|p| &p.name != name))
            .expect("unbounded range")
    }
}

/// Where a layer sits in the tree: its parent (`None` = page root) and index among siblings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub parent: Option<LayerId>,
    pub index: usize,
}

/// A page holds a list of root layers, drawn bottom (index 0) to top.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: PageId,
    pub name: String,
    #[serde(default)]
    pub layers: Vec<Layer>,
}

impl Page {
    pub fn new(name: impl Into<String>) -> Self {
        Self { id: PageId::new(), name: name.into(), layers: vec![] }
    }

    pub fn find(&self, id: LayerId) -> Option<&Layer> {
        fn go(layers: &[Layer], id: LayerId) -> Option<&Layer> {
            layers.iter().find_map(|l| if l.id == id { Some(l) } else { go(l.children()?, id) })
        }
        go(&self.layers, id)
    }

    pub fn find_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        fn go(layers: &mut [Layer], id: LayerId) -> Option<&mut Layer> {
            for l in layers {
                if l.id == id {
                    return Some(l);
                }
                if let Some(found) = l.children_mut().and_then(|c| go(c, id)) {
                    return Some(found);
                }
            }
            None
        }
        go(&mut self.layers, id)
    }

    pub fn contains(&self, id: LayerId) -> bool {
        self.find(id).is_some()
    }

    /// The chain of ancestors from the page root down to (excluding) `id`.
    pub fn ancestors(&self, id: LayerId) -> Option<Vec<LayerId>> {
        fn go(layers: &[Layer], id: LayerId, path: &mut Vec<LayerId>) -> bool {
            for l in layers {
                if l.id == id {
                    return true;
                }
                if let Some(children) = l.children() {
                    path.push(l.id);
                    if go(children, id, path) {
                        return true;
                    }
                    path.pop();
                }
            }
            false
        }
        let mut path = vec![];
        go(&self.layers, id, &mut path).then_some(path)
    }

    pub fn location(&self, id: LayerId) -> Option<Location> {
        let parent = self.ancestors(id)?.last().copied();
        let index = self.siblings(parent)?.iter().position(|l| l.id == id)?;
        Some(Location { parent, index })
    }

    pub fn parent(&self, id: LayerId) -> Option<Option<LayerId>> {
        self.ancestors(id).map(|a| a.last().copied())
    }

    /// The child list of `parent`, or the page root for `None`.
    pub fn siblings(&self, parent: Option<LayerId>) -> Option<&Vec<Layer>> {
        match parent {
            None => Some(&self.layers),
            Some(p) => self.find(p)?.children(),
        }
    }

    pub fn siblings_mut(&mut self, parent: Option<LayerId>) -> Option<&mut Vec<Layer>> {
        match parent {
            None => Some(&mut self.layers),
            Some(p) => self.find_mut(p)?.children_mut(),
        }
    }

    /// Maps the local space of `parent` (or page space for `None`) into page space.
    pub fn space_transform(&self, parent: Option<LayerId>) -> Option<Affine> {
        let Some(parent) = parent else { return Some(Affine::IDENTITY) };
        let mut t = Affine::IDENTITY;
        let mut layers = &self.layers;
        for id in self.ancestors(parent)?.into_iter().chain([parent]) {
            let l = layers.iter().find(|l| l.id == id)?;
            t *= l.local_transform();
            layers = l.children().unwrap_or(layers);
        }
        Some(t)
    }

    /// Maps the local space of layer `id` into page space.
    pub fn world_transform(&self, id: LayerId) -> Option<Affine> {
        self.space_transform(Some(id))
    }

    /// Transform of the space `id`'s frame is expressed in.
    pub fn parent_transform(&self, id: LayerId) -> Option<Affine> {
        self.space_transform(self.parent(id)?)
    }

    /// Axis-aligned bounds of a layer in page space.
    pub fn world_bounds(&self, id: LayerId) -> Option<Rect> {
        self.find(id)?.bounds_in(self.parent_transform(id)?)
    }

    pub fn selection_bounds(&self, ids: &[LayerId]) -> Option<Rect> {
        union_rects(ids.iter().filter_map(|&id| self.world_bounds(id)))
    }

    /// Bounds of all root layers in page space.
    pub fn content_bounds(&self) -> Option<Rect> {
        union_rects(self.layers.iter().filter_map(|l| l.bounds_in(Affine::IDENTITY)))
    }

    /// Removes and returns a layer along with where it was.
    pub fn remove(&mut self, id: LayerId) -> Option<(Location, Layer)> {
        let loc = self.location(id)?;
        let layer = self.siblings_mut(loc.parent)?.remove(loc.index);
        Some((loc, layer))
    }

    /// Inserts `layer` at `loc`, clamping the index. Returns the layer back if `loc.parent` isn't
    /// a container on this page.
    pub fn insert(&mut self, loc: Location, layer: Layer) -> Result<(), Box<Layer>> {
        match self.siblings_mut(loc.parent) {
            Some(siblings) => {
                let i = loc.index.min(siblings.len());
                siblings.insert(i, layer);
                Ok(())
            }
            None => Err(Box::new(layer)),
        }
    }

    /// The topmost artboard containing `point` (page space), if any.
    pub fn artboard_at(&self, point: kurbo::Point) -> Option<LayerId> {
        self.layers
            .iter()
            .rev()
            .find(|l| l.is_artboard() && l.visible && l.frame.to_rect().contains(point))
            .map(|l| l.id)
    }

    /// Visits every layer depth-first in paint order (bottom to top).
    pub fn walk<'a>(&'a self, mut f: impl FnMut(&'a Layer)) {
        for l in &self.layers {
            l.walk(&mut f);
        }
    }

    /// Re-fits every group on the path from the root to `id` (inclusive), innermost first, so
    /// group frames stay tight after their contents change.
    pub fn refit_ancestor_groups(&mut self, id: LayerId) {
        let Some(mut chain) = self.ancestors(id) else { return };
        chain.push(id);
        for gid in chain.into_iter().rev() {
            if let Some(l) = self.find_mut(gid) {
                l.fit_to_children();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frame;
    use kurbo::Point;

    fn sample() -> (Page, LayerId, LayerId) {
        let mut page = Page::new("P");
        let mut ab = Layer::artboard("A", Frame::new(100.0, 100.0, 400.0, 300.0));
        let rect = Layer::rectangle(Frame::new(10.0, 20.0, 50.0, 50.0));
        let rid = rect.id;
        ab.children_mut().unwrap().push(rect);
        let aid = ab.id;
        page.layers.push(ab);
        (page, aid, rid)
    }

    #[test]
    fn world_transform_composes_parents() {
        let (page, _, rid) = sample();
        let t = page.world_transform(rid).unwrap();
        assert_eq!(t * Point::ORIGIN, Point::new(110.0, 120.0));
        assert_eq!(page.world_bounds(rid).unwrap(), Rect::new(110.0, 120.0, 160.0, 170.0));
    }

    #[test]
    fn remove_and_insert() {
        let (mut page, aid, rid) = sample();
        let (loc, layer) = page.remove(rid).unwrap();
        assert_eq!(loc, Location { parent: Some(aid), index: 0 });
        assert!(!page.contains(rid));
        page.insert(loc, layer).unwrap();
        assert_eq!(page.parent(rid), Some(Some(aid)));
    }

    #[test]
    fn next_page_name_skips_taken() {
        let mut doc = Document::default();
        doc.pages.push(Page::new("Page 2"));
        assert_eq!(doc.next_page_name(), "Page 3");
    }
}
