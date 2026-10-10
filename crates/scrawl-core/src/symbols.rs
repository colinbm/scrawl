//! Symbols: reusable layer trees.
//!
//! A symbol master is an artboard with a `symbol_id`; its children are the symbol's contents. An
//! instance ([`LayerKind::Symbol`]) shows those contents resized to its own frame by their
//! resizing constraints, with per-instance overrides: text, nested symbol swaps and shared styles.
//!
//! The resolved contents are derived data, kept in [`SymbolInstance::expanded`] and rebuilt by
//! [`sync_instances`], so rendering, hit testing and bounds never look masters up.

use std::collections::HashMap;

use kurbo::Point;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::components::{SharedStyle, StyleId, id_type, unique_name};
use crate::{Document, Frame, Layer, LayerId, LayerKind, Page, PageId, ops};

id_type!(
    /// Identifies a symbol; instances refer to their master by it.
    SymbolId
);

/// The name of the page new symbol masters go on.
pub const SYMBOLS_PAGE: &str = "Symbols";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolInstance {
    pub symbol_id: SymbolId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
    /// The master's contents fitted to this instance with overrides applied, in the instance's
    /// local space. Rebuilt from the master by [`sync_instances`]; never saved.
    #[serde(skip)]
    pub expanded: Vec<Layer>,
}

/// Replaces one value inside an instance. `path` names the target by master layer ids: the ids
/// of the nested instances it sits in, outermost first, then the target's own id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub path: Vec<LayerId>,
    pub value: OverrideValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum OverrideValue {
    /// A text layer's content.
    Text(String),
    /// Swaps a nested instance's symbol; `None` hides it.
    Symbol(Option<SymbolId>),
    /// Applies a shared layer or text style.
    Style(StyleId),
}

impl SymbolInstance {
    pub fn new(symbol_id: SymbolId) -> Self {
        Self { symbol_id, overrides: vec![], expanded: vec![] }
    }

    pub fn override_for(&self, path: &[LayerId]) -> Option<&OverrideValue> {
        self.overrides.iter().find(|o| o.path == path).map(|o| &o.value)
    }

    /// Sets the override for `path`, or clears it for `None`.
    pub fn set_override(&mut self, path: Vec<LayerId>, value: Option<OverrideValue>) {
        match (self.overrides.iter_mut().find(|o| o.path == path), value) {
            (Some(o), Some(v)) => o.value = v,
            (None, Some(value)) => self.overrides.push(Override { path, value }),
            (_, None) => self.overrides.retain(|o| o.path != path),
        }
    }
}

impl Layer {
    /// A new instance of `master` (a symbol master) at `origin`, at the master's size.
    pub fn instance_of(master: &Layer, origin: Point) -> Option<Layer> {
        let symbol = master.master_of()?;
        let frame = Frame::new(origin.x, origin.y, master.frame.width, master.frame.height);
        Some(Layer::new(master.name.clone(), frame, LayerKind::Symbol(SymbolInstance::new(symbol))))
    }
}

/// The id a master layer gets inside the instance `owner`: stable across rebuilds, and distinct
/// per instance so layout caches and diffs can tell the copies apart.
fn derived_id(owner: LayerId, id: LayerId) -> LayerId {
    LayerId(Uuid::from_u128(owner.0.as_u128().rotate_left(29) ^ id.0.as_u128()))
}

/// Something an instance can override, as listed in the inspector.
#[derive(Clone, Debug, PartialEq)]
pub struct Overridable {
    pub path: Vec<LayerId>,
    /// The master layer's name.
    pub name: String,
    /// How many nested instances deep the layer sits.
    pub depth: usize,
    pub kind: OverridableKind,
}

/// What can be overridden, with the value shown when this instance doesn't override it.
#[derive(Clone, Debug, PartialEq)]
pub enum OverridableKind {
    Text(String),
    Symbol(SymbolId),
    Style { style: StyleId, text: bool },
}

/// Masters and shared styles by id: everything needed to resolve instances.
pub struct Symbols {
    masters: HashMap<SymbolId, Layer>,
    styles: HashMap<StyleId, SharedStyle>,
}

impl Symbols {
    pub fn new(doc: &Document) -> Self {
        let mut masters = HashMap::new();
        let library = doc.foreign_symbols.iter().map(|f| &f.item);
        for master in doc.symbol_masters().into_iter().map(|(_, l)| l).chain(library) {
            if let Some(id) = master.master_of() {
                masters.entry(id).or_insert_with(|| master.clone());
            }
        }
        let library = doc.foreign_styles.iter().map(|f| &f.item);
        let styles = doc.layer_styles.iter().chain(&doc.text_styles).chain(library).map(|s| (s.id, s.clone())).collect();
        Self { masters, styles }
    }

    pub fn master(&self, id: SymbolId) -> Option<&Layer> {
        self.masters.get(&id)
    }

    /// The resolved contents of `instance`, a symbol instance layer.
    pub fn expand(&self, instance: &Layer) -> Vec<Layer> {
        let LayerKind::Symbol(inst) = &instance.kind else { return vec![] };
        self.contents(instance.id, instance.frame, inst, &mut vec![])
    }

    fn contents(&self, owner: LayerId, frame: Frame, inst: &SymbolInstance, stack: &mut Vec<SymbolId>) -> Vec<Layer> {
        // A symbol that (indirectly) contains itself shows nothing at the repeat.
        let Some(master) = self.masters.get(&inst.symbol_id).filter(|_| !stack.contains(&inst.symbol_id)) else {
            return vec![];
        };
        stack.push(inst.symbol_id);
        let mut children = master.children().cloned().unwrap_or_default();
        let (old, new) = ((master.frame.width, master.frame.height), (frame.width, frame.height));
        for c in &mut children {
            self.apply_overrides(c, &[], &inst.overrides);
            c.fit_to_parent(old, new);
            self.resolve(c, owner, stack);
        }
        stack.pop();
        children
    }

    fn apply_overrides(&self, layer: &mut Layer, prefix: &[LayerId], overrides: &[Override]) {
        let path: Vec<LayerId> = prefix.iter().copied().chain([layer.id]).collect();
        for o in overrides.iter().filter(|o| o.path == path) {
            match &o.value {
                OverrideValue::Text(s) => {
                    if let LayerKind::Text(t) = &mut layer.kind {
                        t.text = s.clone();
                    }
                }
                OverrideValue::Style(id) => {
                    if let Some(s) = self.styles.get(id).filter(|s| s.is_text() == layer.is_text()) {
                        s.apply(layer);
                    }
                }
                OverrideValue::Symbol(Some(id)) => {
                    if let LayerKind::Symbol(inner) = &mut layer.kind {
                        inner.symbol_id = *id;
                    }
                }
                OverrideValue::Symbol(None) => layer.visible = false,
            }
        }
        if let LayerKind::Symbol(inner) = &mut layer.kind {
            // Deeper overrides target the nested symbol's contents: hand them down, replacing the
            // nested instance's own.
            for o in overrides.iter().filter(|o| o.path.len() > path.len() && o.path.starts_with(&path)) {
                inner.set_override(o.path[path.len()..].to_vec(), Some(o.value.clone()));
            }
            return;
        }
        for c in layer.children_mut().into_iter().flatten() {
            self.apply_overrides(c, prefix, overrides);
        }
    }

    /// Gives `layer` and its descendants ids derived from `owner`, and expands nested instances.
    fn resolve(&self, layer: &mut Layer, owner: LayerId, stack: &mut Vec<SymbolId>) {
        layer.id = derived_id(owner, layer.id);
        let (id, frame) = (layer.id, layer.frame);
        if let LayerKind::Symbol(inner) = &mut layer.kind {
            inner.expanded = self.contents(id, frame, inner, stack);
            return;
        }
        for c in layer.children_mut().into_iter().flatten() {
            self.resolve(c, owner, stack);
        }
    }

    /// What an instance can override, top layer first, descending into nested instances (as
    /// swapped by `inst`'s overrides).
    pub fn overridables(&self, inst: &SymbolInstance) -> Vec<Overridable> {
        let mut out = vec![];
        let mut walk = OverridableWalk { symbols: self, own: &inst.overrides, out: &mut out, stack: vec![] };
        walk.symbol(inst.symbol_id, &[], &[], 0);
        out
    }
}

struct OverridableWalk<'a> {
    symbols: &'a Symbols,
    /// The instance's own overrides, which decide which nested symbols show.
    own: &'a [Override],
    out: &'a mut Vec<Overridable>,
    stack: Vec<SymbolId>,
}

impl OverridableWalk<'_> {
    /// `inherited` holds overrides set by the masters of enclosing nested instances, with full
    /// paths; they give the defaults.
    fn symbol(&mut self, symbol: SymbolId, prefix: &[LayerId], inherited: &[Override], depth: usize) {
        let Some(master) = self.symbols.masters.get(&symbol).filter(|_| !self.stack.contains(&symbol)) else { return };
        self.stack.push(symbol);
        for c in master.children().into_iter().flatten().rev() {
            self.layer(c, prefix, inherited, depth);
        }
        self.stack.pop();
    }

    fn layer(&mut self, layer: &Layer, prefix: &[LayerId], inherited: &[Override], depth: usize) {
        let path: Vec<LayerId> = prefix.iter().copied().chain([layer.id]).collect();
        let find = |list: &[Override]| list.iter().find(|o| o.path == path).map(|o| o.value.clone());
        let item = |kind| Overridable { path: path.clone(), name: layer.name.clone(), depth, kind };
        match &layer.kind {
            LayerKind::Text(t) => {
                let default = match find(inherited) {
                    Some(OverrideValue::Text(s)) => s,
                    _ => t.text.clone(),
                };
                self.out.push(item(OverridableKind::Text(default)));
            }
            LayerKind::Symbol(inner) => {
                let default = match find(inherited) {
                    Some(OverrideValue::Symbol(Some(id))) => id,
                    _ => inner.symbol_id,
                };
                self.out.push(item(OverridableKind::Symbol(default)));
                let shown = match find(self.own) {
                    Some(OverrideValue::Symbol(swap)) => swap,
                    _ => Some(default),
                };
                if let Some(shown) = shown {
                    // The nested master's own overrides, re-rooted here; enclosing ones win.
                    let mut deeper = inherited.to_vec();
                    for o in &inner.overrides {
                        let full: Vec<LayerId> = path.iter().chain(&o.path).copied().collect();
                        if !deeper.iter().any(|d| d.path == full) {
                            deeper.push(Override { path: full, value: o.value.clone() });
                        }
                    }
                    self.symbol(shown, &path, &deeper, depth + 1);
                }
            }
            _ => {
                for c in layer.children().into_iter().flatten().rev() {
                    self.layer(c, prefix, inherited, depth);
                }
            }
        }
        if let Some(style) = layer.shared_style {
            self.out.push(item(OverridableKind::Style { style, text: layer.is_text() }));
        }
    }
}

/// Rebuilds every instance's contents from its master. `fit` sizes text layers in the fresh
/// contents (it lives in the text engine). Returns whether anything changed.
pub fn sync_instances(doc: &mut Document, fit: &mut dyn FnMut(&mut [Layer])) -> bool {
    let symbols = Symbols::new(doc);
    let mut changed = false;
    doc.walk_layers_mut(|l| {
        if !l.is_symbol_instance() {
            return;
        }
        let mut fresh = symbols.expand(l);
        fit(&mut fresh);
        if let LayerKind::Symbol(inst) = &mut l.kind
            && inst.expanded != fresh
        {
            inst.expanded = fresh;
            changed = true;
        }
    });
    changed
}

impl Document {
    /// Every symbol master, with the page it's on.
    pub fn symbol_masters(&self) -> Vec<(PageId, &Layer)> {
        self.pages.iter().flat_map(|p| p.layers.iter().filter(|l| l.master_of().is_some()).map(move |l| (p.id, l))).collect()
    }

    /// Every symbol, local or from a library, by id and name.
    pub fn all_symbols(&self) -> Vec<(SymbolId, String)> {
        let library = self.foreign_symbols.iter().map(|f| &f.item);
        self.symbol_masters().into_iter().map(|(_, l)| l).chain(library).filter_map(|l| Some((l.master_of()?, l.name.clone()))).collect()
    }

    /// Where the master of `symbol` is: its page and layer.
    pub fn find_master(&self, symbol: SymbolId) -> Option<(PageId, LayerId)> {
        self.symbol_masters().into_iter().find(|(_, l)| l.master_of() == Some(symbol)).map(|(p, l)| (p, l.id))
    }

    /// The "Symbols" page, created at the end if there isn't one.
    pub fn symbols_page(&mut self) -> PageId {
        if let Some(p) = self.pages.iter().find(|p| p.name == SYMBOLS_PAGE) {
            return p.id;
        }
        let page = Page::new(SYMBOLS_PAGE);
        let id = page.id;
        self.pages.push(page);
        id
    }
}

/// What [`create_symbol`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Created {
    pub symbol: SymbolId,
    pub master: LayerId,
    /// The instance replacing the selection; `None` when an artboard became a master in place.
    pub instance: Option<LayerId>,
}

/// Turns the selection on `page_id` into a symbol ("Create Symbol"). A lone artboard
/// becomes a master in place. Other layers move into a new master on the Symbols page and are
/// replaced by an instance in the same spot.
pub fn create_symbol(doc: &mut Document, page_id: PageId, ids: &[LayerId]) -> Option<Created> {
    let symbol = SymbolId::new();
    let taken: Vec<String> = doc.symbol_masters().iter().map(|(_, l)| l.name.clone()).collect();
    let page = doc.page_mut(page_id)?;
    let mut ids = ops::normalize_selection(page, ids);
    if let [id] = ids[..]
        && let Some(LayerKind::Artboard(a)) = page.find_mut(id).map(|l| &mut l.kind)
    {
        if a.symbol_id.is_some() {
            return None;
        }
        a.symbol_id = Some(symbol);
        return Some(Created { symbol, master: id, instance: None });
    }
    if ids.is_empty() || ids.iter().any(|&id| page.find(id).is_none_or(Layer::is_artboard)) {
        return None;
    }

    // Keep the layers' stacking order inside the master.
    let mut order = HashMap::new();
    page.walk(|l| {
        let n = order.len();
        order.insert(l.id, n);
    });
    ids.sort_by_key(|id| order.get(id).copied());
    let bounds = page.selection_bounds(&ids)?;
    let top = *ids.last()?;
    let loc = page.location(top)?;
    let below = ids.iter().filter(|&&id| page.location(id).is_some_and(|l| l.parent == loc.parent && l.index < loc.index)).count();
    let name = match ids[..] {
        [id] => page.find(id)?.name.clone(),
        _ => unique_name("Symbol", taken.iter().map(String::as_str)),
    };
    let mut children: Vec<Layer> = ids.iter().filter_map(|&id| ops::page_space_copy(page, id)).collect();
    for c in &mut children {
        c.frame.translate(-bounds.origin().to_vec2());
    }

    ops::delete(page, &ids);
    let instance = Layer::new(name.clone(), Frame::from_rect(bounds), LayerKind::Symbol(SymbolInstance::new(symbol)));
    let instance_id = ops::insert_root(page, instance);
    if loc.parent.is_none_or(|p| page.contains(p)) {
        ops::move_to(page, instance_id, loc.parent, loc.index - below);
    }

    let symbols_page = doc.symbols_page();
    let page = doc.page_mut(symbols_page)?;
    let origin = page.content_bounds().map_or(Point::ORIGIN, |b| Point::new((b.x1 + 100.0).round(), b.y0.round()));
    let mut master = Layer::artboard(name, Frame::new(origin.x, origin.y, bounds.width(), bounds.height()));
    if let LayerKind::Artboard(a) = &mut master.kind {
        a.children = children;
        a.symbol_id = Some(symbol);
    }
    let master = ops::insert_root(page, master);
    Some(Created { symbol, master, instance: Some(instance_id) })
}

/// Replaces an instance with a group of its current contents ("Detach from Symbol").
/// Nested instances stay instances. Returns the group.
pub fn detach(page: &mut Page, id: LayerId) -> Option<LayerId> {
    let l = page.find(id)?;
    let LayerKind::Symbol(inst) = &l.kind else { return None };
    let children: Vec<Layer> = inst.expanded.iter().map(Layer::duplicate).collect();
    if children.is_empty() {
        return None;
    }
    let mut group = Layer::group(children, l.frame);
    group.name = l.name.clone();
    group.visible = l.visible;
    group.locked = l.locked;
    group.rotation = l.rotation;
    group.flip_horizontal = l.flip_horizontal;
    group.flip_vertical = l.flip_vertical;
    group.opacity = l.opacity;
    group.blend_mode = l.blend_mode;
    group.is_mask = l.is_mask;
    group.resizing = l.resizing;
    group.fit_to_children();
    let gid = group.id;
    *page.find_mut(id)? = group;
    page.refit_ancestor_groups(gid);
    Some(gid)
}

/// Detaches every instance of `symbol` in the document, e.g. before its master is deleted.
pub fn detach_all(doc: &mut Document, symbol: SymbolId) {
    for page in &mut doc.pages {
        let mut ids = vec![];
        page.walk(|l| {
            if matches!(&l.kind, LayerKind::Symbol(i) if i.symbol_id == symbol) {
                ids.push(l.id);
            }
        });
        for id in ids {
            // Instances that show nothing (a missing master) can only be removed.
            if detach(page, id).is_none() {
                ops::delete(page, &[id]);
            }
        }
    }
}

/// Deletes a symbol: its instances are detached and its master removed.
pub fn remove_symbol(doc: &mut Document, symbol: SymbolId) {
    detach_all(doc, symbol);
    if let Some((page, id)) = doc.find_master(symbol)
        && let Some(page) = doc.page_mut(page)
    {
        ops::delete(page, &[id]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::Resizing;
    use crate::{Color, Fill};
    use kurbo::Rect;

    /// A 100×40 "Button" master: a stretching background and a centered, fixed-size label.
    fn button(doc: &mut Document) -> (SymbolId, LayerId, LayerId) {
        let mut bg = Layer::rectangle(Frame::new(0.0, 0.0, 100.0, 40.0));
        bg.resizing = Resizing { left: true, right: true, top: true, bottom: true, ..Default::default() };
        let mut label = Layer::text(Point::new(30.0, 10.0), "Label");
        label.frame = Frame::new(30.0, 10.0, 40.0, 20.0);
        label.resizing = Resizing { fix_width: true, fix_height: true, ..Default::default() };
        let (bg_id, label_id) = (bg.id, label.id);
        let mut master = Layer::artboard("Button", Frame::new(0.0, 0.0, 100.0, 40.0));
        let symbol = SymbolId::new();
        if let LayerKind::Artboard(a) = &mut master.kind {
            a.children = vec![bg, label];
            a.symbol_id = Some(symbol);
        }
        let page = doc.symbols_page();
        doc.page_mut(page).unwrap().layers.push(master);
        (symbol, bg_id, label_id)
    }

    fn instance(doc: &mut Document, symbol: SymbolId, frame: Frame) -> LayerId {
        let l = Layer::new("Instance", frame, LayerKind::Symbol(SymbolInstance::new(symbol)));
        let id = l.id;
        doc.pages[0].layers.push(l);
        id
    }

    fn expanded(doc: &Document, id: LayerId) -> Vec<Layer> {
        match &doc.pages.iter().find_map(|p| p.find(id)).unwrap().kind {
            LayerKind::Symbol(i) => i.expanded.clone(),
            _ => panic!("not an instance"),
        }
    }

    fn set_override(doc: &mut Document, id: LayerId, path: Vec<LayerId>, value: OverrideValue) {
        let l = doc.pages.iter_mut().find_map(|p| p.find_mut(id)).unwrap();
        if let LayerKind::Symbol(i) = &mut l.kind {
            i.set_override(path, Some(value));
        }
    }

    #[test]
    fn instances_resize_by_constraints_and_take_overrides() {
        let mut doc = Document::default();
        let (symbol, bg, label) = button(&mut doc);
        let inst = instance(&mut doc, symbol, Frame::new(10.0, 10.0, 200.0, 40.0));
        set_override(&mut doc, inst, vec![label], OverrideValue::Text("Go".into()));
        assert!(sync_instances(&mut doc, &mut |_| {}));
        assert!(!sync_instances(&mut doc, &mut |_| {}), "a second sync is a no-op");

        let contents = expanded(&doc, inst);
        assert_eq!(contents[0].frame, Frame::new(0.0, 0.0, 200.0, 40.0));
        assert_eq!(contents[1].frame, Frame::new(80.0, 10.0, 40.0, 20.0));
        let LayerKind::Text(t) = &contents[1].kind else { panic!() };
        assert_eq!(t.text, "Go");
        // Copies get their own ids, distinct from the master's.
        assert!(contents[0].id != bg && contents[1].id != label);
    }

    #[test]
    fn nested_symbols_swap_and_hide() {
        let mut doc = Document::default();
        let (button, _, label) = button(&mut doc);
        let dot = {
            let mut m = Layer::artboard("Dot", Frame::new(300.0, 0.0, 20.0, 20.0));
            let symbol = SymbolId::new();
            if let LayerKind::Artboard(a) = &mut m.kind {
                a.children = vec![Layer::oval(Frame::new(0.0, 0.0, 20.0, 20.0))];
                a.symbol_id = Some(symbol);
            }
            doc.pages[1].layers.push(m);
            symbol
        };
        // A "Row" master holding a nested button.
        let nested = Layer::new("Nested", Frame::new(0.0, 0.0, 100.0, 40.0), LayerKind::Symbol(SymbolInstance::new(button)));
        let nested_id = nested.id;
        let mut row = Layer::artboard("Row", Frame::new(0.0, 100.0, 120.0, 40.0));
        let row_symbol = SymbolId::new();
        if let LayerKind::Artboard(a) = &mut row.kind {
            a.children = vec![nested];
            a.symbol_id = Some(row_symbol);
        }
        doc.pages[1].layers.push(row);

        let inst = instance(&mut doc, row_symbol, Frame::new(0.0, 0.0, 120.0, 40.0));
        set_override(&mut doc, inst, vec![nested_id, label], OverrideValue::Text("Deep".into()));
        sync_instances(&mut doc, &mut |_| {});
        let LayerKind::Symbol(n) = &expanded(&doc, inst)[0].kind else { panic!() };
        let LayerKind::Text(t) = &n.expanded[1].kind else { panic!() };
        assert_eq!(t.text, "Deep");

        let symbols = Symbols::new(&doc);
        let LayerKind::Symbol(i) = &doc.pages[0].find(inst).unwrap().kind else { panic!() };
        let items = symbols.overridables(i);
        assert_eq!(items.len(), 2, "the nested symbol and its label");
        assert_eq!(items[0].kind, OverridableKind::Symbol(button));
        assert_eq!(items[1].path, vec![nested_id, label]);

        set_override(&mut doc, inst, vec![nested_id], OverrideValue::Symbol(Some(dot)));
        sync_instances(&mut doc, &mut |_| {});
        let LayerKind::Symbol(n) = &expanded(&doc, inst)[0].kind else { panic!() };
        assert_eq!(n.expanded[0].kind, LayerKind::Oval);

        set_override(&mut doc, inst, vec![nested_id], OverrideValue::Symbol(None));
        sync_instances(&mut doc, &mut |_| {});
        assert!(!expanded(&doc, inst)[0].visible);
    }

    #[test]
    fn style_overrides_apply_shared_styles() {
        let mut doc = Document::default();
        let (symbol, bg, _) = button(&mut doc);
        let mut black = Layer::rectangle(Frame::new(0.0, 0.0, 1.0, 1.0));
        black.style.fills = vec![Fill::solid(Color::BLACK)];
        let style = doc.create_shared_style(&black);
        let inst = instance(&mut doc, symbol, Frame::new(0.0, 0.0, 100.0, 40.0));
        set_override(&mut doc, inst, vec![bg], OverrideValue::Style(style));
        sync_instances(&mut doc, &mut |_| {});
        assert_eq!(expanded(&doc, inst)[0].style.fills[0].color, Color::BLACK);
    }

    #[test]
    fn symbols_containing_themselves_terminate() {
        let mut doc = Document::default();
        let (symbol, _, _) = button(&mut doc);
        let selfie = Layer::new("Self", Frame::new(0.0, 0.0, 10.0, 10.0), LayerKind::Symbol(SymbolInstance::new(symbol)));
        doc.pages[1].layers[0].children_mut().unwrap().push(selfie);
        let inst = instance(&mut doc, symbol, Frame::new(0.0, 0.0, 100.0, 40.0));
        sync_instances(&mut doc, &mut |_| {});
        let LayerKind::Symbol(inner) = &expanded(&doc, inst)[2].kind else { panic!() };
        assert!(inner.expanded.is_empty());
    }

    #[test]
    fn create_symbol_then_detach_keeps_layers_in_place() {
        let mut doc = Document::default();
        let page_id = doc.pages[0].id;
        let page = &mut doc.pages[0];
        ops::insert_drawn(page, Layer::artboard("A", Frame::new(0.0, 0.0, 400.0, 400.0)));
        let a = ops::insert_drawn(page, Layer::rectangle(Frame::new(50.0, 50.0, 40.0, 40.0)));
        let b = ops::insert_drawn(page, Layer::oval(Frame::new(120.0, 60.0, 30.0, 30.0)));
        let before = page.selection_bounds(&[a, b]).unwrap();

        let created = create_symbol(&mut doc, page_id, &[b, a]).unwrap();
        let inst = created.instance.unwrap();
        assert_eq!(doc.pages[1].name, SYMBOLS_PAGE);
        let master = doc.pages[1].find(created.master).unwrap();
        assert_eq!(master.children().unwrap().len(), 2);
        assert_eq!(master.children().unwrap()[0].kind.type_name(), "Rectangle", "stacking order kept");
        assert_eq!(doc.pages[0].world_bounds(inst), Some(before));
        assert!(!doc.pages[0].contains(a));

        sync_instances(&mut doc, &mut |_| {});
        let page = &mut doc.pages[0];
        let group = detach(page, inst).unwrap();
        let kids = page.find(group).unwrap().children().unwrap().clone();
        let t = page.world_transform(group).unwrap();
        let bounds = kids.iter().filter_map(|k| k.bounds_in(t)).reduce(|x: Rect, y| x.union(y)).unwrap();
        assert!((bounds.x0 - before.x0).abs() < 1e-9 && (bounds.y1 - before.y1).abs() < 1e-9);
        assert_eq!(page.find(group).unwrap().bounds_in(page.parent_transform(group).unwrap()), Some(before));
    }

    #[test]
    fn removing_a_symbol_detaches_instances() {
        let mut doc = Document::default();
        let (symbol, _, _) = button(&mut doc);
        let inst = instance(&mut doc, symbol, Frame::new(0.0, 0.0, 100.0, 40.0));
        sync_instances(&mut doc, &mut |_| {});
        remove_symbol(&mut doc, symbol);
        assert!(doc.find_master(symbol).is_none());
        assert!(!doc.pages[0].contains(inst));
        assert!(doc.pages[0].layers[0].is_group());
    }

    #[test]
    fn instances_round_trip_without_their_contents() {
        let mut doc = Document::default();
        let (symbol, _, label) = button(&mut doc);
        let inst = instance(&mut doc, symbol, Frame::new(0.0, 0.0, 100.0, 40.0));
        set_override(&mut doc, inst, vec![label], OverrideValue::Text("Saved".into()));
        sync_instances(&mut doc, &mut |_| {});
        let json = serde_json::to_string(&doc).unwrap();
        assert!(!json.contains("expanded"));
        let back = crate::io::from_bytes(&crate::io::to_bytes(&doc).unwrap()).unwrap();
        assert_eq!(back, doc, "contents are rebuilt on load");
    }
}
