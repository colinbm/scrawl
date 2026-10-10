//! Libraries: other documents whose symbols, shared styles and color variables this one uses.
//!
//! Using a library component copies it into the document, along with everything it
//! depends on, remembering which library it came from. The copies keep the document
//! self-contained (it renders without the library). When the library changes,
//! [`Document::library_updates`] lists the stale copies and [`Document::update_library`]
//! refreshes them and the layers that use them.

use kurbo::Point;
use serde::{Deserialize, Serialize};

use crate::components::{StyleId, SwatchId, id_type};
use crate::symbols::SymbolId;
use crate::{Document, Layer, LayerKind, OverrideValue, ops};

id_type!(
    /// Identifies a linked library within a document.
    LibraryId
);

/// A linked library document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryRef {
    pub id: LibraryId,
    pub name: String,
    /// Where the library's `.scrawl` file is.
    pub path: String,
}

/// A copy of a library component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Foreign<T> {
    pub library: LibraryId,
    pub item: T,
}

/// Something a library offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Component {
    Symbol(SymbolId),
    Style(StyleId),
    Swatch(SwatchId),
}

/// A master as stored in a library copy: without derived instance contents and at the origin,
/// so moving it around the library's canvas isn't a change.
fn normalized_master(master: &Layer) -> Layer {
    let mut m = master.clone();
    m.frame.x = 0.0;
    m.frame.y = 0.0;
    m.walk_mut(&mut |l| {
        if let LayerKind::Symbol(i) = &mut l.kind {
            i.expanded.clear();
        }
    });
    m
}

/// The master of `symbol` in `doc`, local or itself from a library.
fn master_in(doc: &Document, symbol: SymbolId) -> Option<&Layer> {
    let library = doc.foreign_symbols.iter().map(|f| &f.item);
    doc.symbol_masters().into_iter().map(|(_, l)| l).chain(library).find(|l| l.master_of() == Some(symbol))
}

/// The components a layer tree uses: nested symbols, shared styles and color variables.
fn dependencies(layer: &Layer, out: &mut Vec<Component>) {
    layer.walk(&mut |l| {
        out.extend(l.shared_style.map(Component::Style));
        let mut style = l.style.clone();
        style.colors_mut(|_, link| out.extend(link.map(Component::Swatch)));
        if let LayerKind::Symbol(i) = &l.kind {
            out.push(Component::Symbol(i.symbol_id));
            for o in &i.overrides {
                match o.value {
                    OverrideValue::Symbol(Some(s)) => out.push(Component::Symbol(s)),
                    OverrideValue::Style(s) => out.push(Component::Style(s)),
                    _ => {}
                }
            }
        }
    });
}

impl Document {
    /// Links a library, or returns the existing link to the same file.
    pub fn add_library(&mut self, name: impl Into<String>, path: impl Into<String>) -> LibraryId {
        let path = path.into();
        if let Some(l) = self.libraries.iter().find(|l| l.path == path) {
            return l.id;
        }
        let id = LibraryId::new();
        self.libraries.push(LibraryRef { id, name: name.into(), path });
        id
    }

    pub fn library(&self, id: LibraryId) -> Option<&LibraryRef> {
        self.libraries.iter().find(|l| l.id == id)
    }

    /// Whether the document has `c`, locally or as a library copy.
    pub fn has_component(&self, c: Component) -> bool {
        match c {
            Component::Symbol(id) => master_in(self, id).is_some(),
            Component::Style(id) => self.shared_style(id).is_some(),
            Component::Swatch(id) => self.swatch(id).is_some(),
        }
    }

    /// Copies component `c` from `source`, library `lib`'s document, plus whatever it depends on.
    /// Components already here are left alone. Returns whether the document now has `c`.
    pub fn import(&mut self, lib: LibraryId, source: &Document, c: Component) -> bool {
        let mut queue = vec![c];
        while let Some(c) = queue.pop() {
            if self.has_component(c) {
                continue;
            }
            match c {
                Component::Symbol(id) => {
                    let Some(master) = master_in(source, id) else { continue };
                    dependencies(master, &mut queue);
                    self.foreign_symbols.push(Foreign { library: lib, item: normalized_master(master) });
                }
                Component::Style(id) => {
                    let Some(s) = source.shared_style(id) else { continue };
                    let mut style = s.style.clone();
                    style.colors_mut(|_, link| queue.extend(link.map(Component::Swatch)));
                    self.foreign_styles.push(Foreign { library: lib, item: s.clone() });
                }
                Component::Swatch(id) => {
                    let Some(s) = source.swatch(id) else { continue };
                    self.foreign_swatches.push(Foreign { library: lib, item: s.clone() });
                }
            }
        }
        self.has_component(c)
    }

    /// Copies from library `lib` that differ from `source`, its current document.
    pub fn library_updates(&self, lib: LibraryId, source: &Document) -> Vec<Component> {
        let mut out = vec![];
        for f in self.foreign_symbols.iter().filter(|f| f.library == lib) {
            let Some(id) = f.item.master_of() else { continue };
            if master_in(source, id).is_some_and(|m| normalized_master(m) != f.item) {
                out.push(Component::Symbol(id));
            }
        }
        for f in self.foreign_styles.iter().filter(|f| f.library == lib) {
            if source.shared_style(f.item.id).is_some_and(|s| *s != f.item) {
                out.push(Component::Style(f.item.id));
            }
        }
        for f in self.foreign_swatches.iter().filter(|f| f.library == lib) {
            if source.swatch(f.item.id).is_some_and(|s| *s != f.item) {
                out.push(Component::Swatch(f.item.id));
            }
        }
        out
    }

    /// Refreshes every stale copy from library `lib`, and the layers using them. Returns how many
    /// components changed.
    pub fn update_library(&mut self, lib: LibraryId, source: &Document) -> usize {
        let updates = self.library_updates(lib, source);
        for &c in &updates {
            match c {
                Component::Symbol(id) => {
                    let Some(master) = master_in(source, id) else { continue };
                    let mut deps = vec![];
                    dependencies(master, &mut deps);
                    if let Some(f) = self.foreign_symbols.iter_mut().find(|f| f.item.master_of() == Some(id)) {
                        f.item = normalized_master(master);
                    }
                    // An updated master may use components that weren't needed before.
                    for d in deps {
                        self.import(lib, source, d);
                    }
                }
                Component::Style(id) => {
                    let Some(s) = source.shared_style(id) else { continue };
                    if let Some(f) = self.foreign_styles.iter_mut().find(|f| f.item.id == id) {
                        f.item = s.clone();
                    }
                    self.propagate_shared_style(id);
                }
                Component::Swatch(id) => {
                    let Some(s) = source.swatch(id) else { continue };
                    if let Some(f) = self.foreign_swatches.iter_mut().find(|f| f.item.id == id) {
                        f.item = s.clone();
                    }
                    self.recolor_links(id, s.color);
                }
            }
        }
        updates.len()
    }

    /// Unlinks a library. Its components that are in use stay, as local ones: masters move to the
    /// Symbols page, styles and color variables join the document's own.
    pub fn remove_library(&mut self, lib: LibraryId) {
        self.libraries.retain(|l| l.id != lib);
        let (symbols, rest): (Vec<_>, Vec<_>) = self.foreign_symbols.drain(..).partition(|f| f.library == lib);
        self.foreign_symbols = rest;
        if !symbols.is_empty() {
            let page = self.symbols_page();
            let page = self.page_mut(page).expect("just found");
            for f in symbols {
                let mut master = f.item;
                let origin = page.content_bounds().map_or(Point::ORIGIN, |b| Point::new((b.x1 + 100.0).round(), b.y0.round()));
                master.frame.x = origin.x;
                master.frame.y = origin.y;
                ops::insert_root(page, master);
            }
        }
        let (styles, rest): (Vec<_>, Vec<_>) = self.foreign_styles.drain(..).partition(|f| f.library == lib);
        self.foreign_styles = rest;
        for f in styles {
            self.add_shared_style(f.item);
        }
        let (swatches, rest): (Vec<_>, Vec<_>) = self.foreign_swatches.drain(..).partition(|f| f.library == lib);
        self.foreign_swatches = rest;
        self.swatches.extend(swatches.into_iter().map(|f| f.item));
    }

    /// The library copies of `lib`'s components, for listing.
    pub fn library_symbols(&self, lib: LibraryId) -> impl Iterator<Item = &Layer> {
        self.foreign_symbols.iter().filter(move |f| f.library == lib).map(|f| &f.item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{SymbolInstance, sync_instances};
    use crate::{Color, Fill, Frame};

    /// A library with a "Chip" symbol whose fill links to a "Brand" variable and whose shape
    /// uses a "Chip" layer style.
    fn library() -> (Document, SymbolId, SwatchId, StyleId) {
        let mut lib = Document::default();
        let brand = lib.add_swatch(Color::rgba(1.0, 0.0, 0.0, 1.0));
        let mut shape = Layer::rectangle(Frame::new(0.0, 0.0, 40.0, 20.0));
        shape.style.fills = vec![Fill { swatch: Some(brand), ..Fill::solid(Color::rgba(1.0, 0.0, 0.0, 1.0)) }];
        let style = lib.create_shared_style(&shape);
        shape.shared_style = Some(style);
        let mut master = Layer::artboard("Chip", Frame::new(500.0, 500.0, 40.0, 20.0));
        let symbol = SymbolId::new();
        if let LayerKind::Artboard(a) = &mut master.kind {
            a.children = vec![shape];
            a.symbol_id = Some(symbol);
        }
        lib.pages[0].layers.push(master);
        (lib, symbol, brand, style)
    }

    #[test]
    fn importing_a_symbol_brings_its_dependencies() {
        let (lib, symbol, brand, style) = library();
        let mut doc = Document::default();
        let id = doc.add_library("UI Kit", "/libs/ui.scrawl");
        assert_eq!(doc.add_library("Again", "/libs/ui.scrawl"), id);
        assert!(doc.import(id, &lib, Component::Symbol(symbol)));
        assert_eq!(doc.foreign_symbols.len(), 1);
        assert_eq!(doc.foreign_symbols[0].item.frame.origin(), Point::ORIGIN);
        assert!(doc.shared_style(style).is_some() && doc.swatch(brand).is_some());
        assert!(doc.library_updates(id, &lib).is_empty());

        // Instances of library symbols render like local ones.
        let inst = Layer::new("Chip", Frame::new(0.0, 0.0, 80.0, 20.0), LayerKind::Symbol(SymbolInstance::new(symbol)));
        doc.pages[0].layers.push(inst);
        sync_instances(&mut doc, &mut |_| {});
        let LayerKind::Symbol(i) = &doc.pages[0].layers[0].kind else { panic!() };
        assert_eq!(i.expanded[0].frame.width, 80.0);
    }

    #[test]
    fn library_changes_update_copies_and_linked_layers() {
        let (mut lib, symbol, brand, style) = library();
        let mut doc = Document::default();
        let id = doc.add_library("UI Kit", "/libs/ui.scrawl");
        doc.import(id, &lib, Component::Symbol(symbol));
        // A local layer using the library's style and variable.
        let mut local = Layer::oval(Frame::new(0.0, 0.0, 10.0, 10.0));
        doc.shared_style(style).unwrap().clone().apply(&mut local);
        doc.pages[0].layers.push(local);

        // Moving the master in the library isn't a change; recoloring is.
        lib.pages[0].layers[0].frame.x = 0.0;
        assert!(doc.library_updates(id, &lib).is_empty());
        let blue = Color::rgba(0.0, 0.0, 1.0, 1.0);
        lib.set_swatch_color(brand, blue);
        let mut updates = doc.library_updates(id, &lib);
        updates.sort_by_key(|c| format!("{c:?}"));
        assert_eq!(updates.len(), 3, "the variable, the style using it and the master: {updates:?}");

        assert_eq!(doc.update_library(id, &lib), 3);
        assert_eq!(doc.pages[0].layers[0].style.fills[0].color, blue);
        assert!(doc.library_updates(id, &lib).is_empty());
    }

    #[test]
    fn removing_a_library_keeps_components_locally() {
        let (lib, symbol, _, _) = library();
        let mut doc = Document::default();
        let id = doc.add_library("UI Kit", "/libs/ui.scrawl");
        doc.import(id, &lib, Component::Symbol(symbol));
        doc.remove_library(id);
        assert!(doc.libraries.is_empty() && doc.foreign_symbols.is_empty());
        assert!(doc.find_master(symbol).is_some());
        assert_eq!(doc.layer_styles.len(), 1);
        assert_eq!(doc.swatches.len(), 1);
    }
}
