//! Shared styles and color variables: named, reusable values that layers link to.
//!
//! A linked layer keeps its own copy of the value, so it renders without lookups
//! and can drift from the shared version until the user updates or resets it. Changing a shared
//! value pushes it to every linked layer.

use serde::{Deserialize, Serialize};

use crate::{Color, Document, Layer, LayerKind, Style, TextStyle};

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub uuid::Uuid);

        impl $name {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4())
            }
        }
    };
}
pub(crate) use id_type;

id_type!(
    /// Identifies a shared layer or text style.
    StyleId
);
id_type!(
    /// Identifies a color variable.
    SwatchId
);

/// A named layer style. Text styles also carry typography.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharedStyle {
    pub id: StyleId,
    pub name: String,
    pub style: Style,
    /// Typography, for text styles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextStyle>,
}

impl SharedStyle {
    /// A shared style capturing `layer`'s appearance, plus its typography if it's text.
    pub fn from_layer(name: impl Into<String>, layer: &Layer) -> Self {
        let text = match &layer.kind {
            LayerKind::Text(t) => Some(t.style.clone()),
            _ => None,
        };
        Self { id: StyleId::new(), name: name.into(), style: layer.style.clone(), text }
    }

    pub fn is_text(&self) -> bool {
        self.text.is_some()
    }

    /// Gives `layer` this style and links it.
    pub fn apply(&self, layer: &mut Layer) {
        layer.style = self.style.clone();
        if let (Some(ts), LayerKind::Text(t)) = (&self.text, &mut layer.kind) {
            t.style = ts.clone();
        }
        layer.shared_style = Some(self.id);
    }

    /// Whether `layer` looks exactly like this style.
    pub fn matches(&self, layer: &Layer) -> bool {
        layer.style == self.style
            && match (&self.text, &layer.kind) {
                (Some(ts), LayerKind::Text(t)) => t.style == *ts,
                _ => true,
            }
    }
}

/// A color variable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub id: SwatchId,
    pub name: String,
    pub color: Color,
}

impl Swatch {
    /// Appends a new variable with a fresh name to `list`.
    pub fn add_to(list: &mut Vec<Swatch>, color: Color) -> SwatchId {
        let name = unique_name("Color", list.iter().map(|s| s.name.as_str()));
        let id = SwatchId::new();
        list.push(Swatch { id, name, color });
        id
    }
}

/// Clears color-variable links whose color no longer matches the variable, e.g. after the user
/// picked a different color. `lookup` gives a variable's current color; links to unknown
/// variables are kept.
pub fn unlink_stale_colors(style: &mut Style, lookup: impl Fn(SwatchId) -> Option<Color>) {
    style.colors_mut(|c, link| {
        if link.and_then(&lookup).is_some_and(|v| v != *c) {
            *link = None;
        }
    });
}

/// `base` followed by the first number that makes it unique among `taken`, e.g. "Color 3".
pub fn unique_name<'a>(base: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    (1..)
        .map(|n| format!("{base} {n}"))
        .find(|name| taken.clone().all(|t| t != name))
        .expect("unbounded range")
}

impl Document {
    /// A shared style, local or from a library.
    pub fn shared_style(&self, id: StyleId) -> Option<&SharedStyle> {
        let library = self.foreign_styles.iter().map(|f| &f.item);
        self.layer_styles.iter().chain(&self.text_styles).chain(library).find(|s| s.id == id)
    }

    fn shared_style_mut(&mut self, id: StyleId) -> Option<&mut SharedStyle> {
        self.layer_styles.iter_mut().chain(&mut self.text_styles).find(|s| s.id == id)
    }

    /// A color variable, local or from a library.
    pub fn swatch(&self, id: SwatchId) -> Option<&Swatch> {
        self.swatches.iter().chain(self.foreign_swatches.iter().map(|f| &f.item)).find(|s| s.id == id)
    }

    /// Visits every layer on every page.
    pub fn walk_layers_mut(&mut self, mut f: impl FnMut(&mut Layer)) {
        for page in &mut self.pages {
            page.walk_mut(&mut f);
        }
    }

    /// Visits every style in the document: on layers and in shared styles (including library
    /// ones, which may use library color variables).
    pub fn styles_mut(&mut self, mut f: impl FnMut(&mut Style)) {
        self.walk_layers_mut(|l| f(&mut l.style));
        let library = self.foreign_styles.iter_mut().map(|f| &mut f.item);
        for s in self.layer_styles.iter_mut().chain(&mut self.text_styles).chain(library) {
            f(&mut s.style);
        }
    }

    /// Adds a shared style to the text or layer list, by its kind.
    pub fn add_shared_style(&mut self, style: SharedStyle) -> StyleId {
        let id = style.id;
        if style.is_text() { &mut self.text_styles } else { &mut self.layer_styles }.push(style);
        id
    }

    /// Creates a shared style from `layer` with a fresh name. The layer isn't linked; callers
    /// usually do that next.
    pub fn create_shared_style(&mut self, layer: &Layer) -> StyleId {
        let text = layer.is_text();
        let list = if text { &self.text_styles } else { &self.layer_styles };
        let name = unique_name(if text { "Text Style" } else { "Layer Style" }, list.iter().map(|s| s.name.as_str()));
        self.add_shared_style(SharedStyle::from_layer(name, layer))
    }

    /// Makes shared style `id` look like `layer`, then pushes it to every linked layer.
    pub fn update_shared_style(&mut self, id: StyleId, layer: &Layer) {
        let Some(s) = self.shared_style_mut(id) else { return };
        s.style = layer.style.clone();
        if let (Some(ts), LayerKind::Text(t)) = (&mut s.text, &layer.kind) {
            *ts = t.style.clone();
        }
        self.propagate_shared_style(id);
    }

    /// Re-applies shared style `id` to every layer linked to it.
    pub fn propagate_shared_style(&mut self, id: StyleId) {
        let Some(s) = self.shared_style(id).cloned() else { return };
        self.walk_layers_mut(|l| {
            if l.shared_style == Some(id) {
                s.apply(l);
            }
        });
    }

    /// Deletes a shared style. Linked layers keep their look but are unlinked.
    pub fn remove_shared_style(&mut self, id: StyleId) {
        self.layer_styles.retain(|s| s.id != id);
        self.text_styles.retain(|s| s.id != id);
        self.walk_layers_mut(|l| {
            if l.shared_style == Some(id) {
                l.shared_style = None;
            }
        });
    }

    pub fn add_swatch(&mut self, color: Color) -> SwatchId {
        Swatch::add_to(&mut self.swatches, color)
    }

    /// Changes a color variable and every color linked to it.
    pub fn set_swatch_color(&mut self, id: SwatchId, color: Color) {
        let Some(s) = self.swatches.iter_mut().find(|s| s.id == id) else { return };
        s.color = color;
        self.recolor_links(id, color);
    }

    /// Sets every color linked to variable `id` to `color`.
    pub(crate) fn recolor_links(&mut self, id: SwatchId, color: Color) {
        self.styles_mut(|style| {
            style.colors_mut(|c, link| {
                if *link == Some(id) {
                    *c = color;
                }
            })
        });
    }

    /// Deletes a color variable. Linked colors keep their value but are unlinked.
    pub fn remove_swatch(&mut self, id: SwatchId) {
        self.swatches.retain(|s| s.id != id);
        self.styles_mut(|style| {
            style.colors_mut(|_, link| {
                if *link == Some(id) {
                    *link = None;
                }
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fill, Frame, Page};
    use kurbo::Point;

    fn doc_with(layers: Vec<Layer>) -> Document {
        let mut page = Page::new("P");
        page.layers = layers;
        Document { pages: vec![page], ..Default::default() }
    }

    #[test]
    fn shared_style_updates_reach_linked_layers() {
        let a = Layer::rectangle(Frame::new(0.0, 0.0, 10.0, 10.0));
        let b = Layer::oval(Frame::new(20.0, 0.0, 10.0, 10.0));
        let mut doc = doc_with(vec![a, b]);
        let source = doc.pages[0].layers[0].clone();
        let id = doc.create_shared_style(&source);
        assert_eq!(doc.layer_styles[0].name, "Layer Style 1");
        let s = doc.shared_style(id).unwrap().clone();
        doc.pages[0].layers.iter_mut().for_each(|l| s.apply(l));

        // Diverge one layer, then push its look to the shared style.
        let mut edited = doc.pages[0].layers[0].clone();
        edited.style.fills = vec![Fill::solid(Color::BLACK)];
        assert!(!s.matches(&edited));
        doc.pages[0].layers[0] = edited.clone();
        doc.update_shared_style(id, &edited);
        assert!(doc.pages[0].layers.iter().all(|l| l.style.fills[0].color == Color::BLACK));

        doc.remove_shared_style(id);
        assert!(doc.pages[0].layers.iter().all(|l| l.shared_style.is_none()));
    }

    #[test]
    fn text_styles_carry_typography() {
        let mut t = Layer::text(Point::ORIGIN, "Hi");
        if let LayerKind::Text(text) = &mut t.kind {
            text.style.font_size = 40.0;
        }
        let s = SharedStyle::from_layer("Heading", &t);
        assert!(s.is_text());
        let mut other = Layer::text(Point::ORIGIN, "Other");
        assert!(!s.matches(&other));
        s.apply(&mut other);
        assert!(s.matches(&other));
        let LayerKind::Text(text) = &other.kind else { panic!() };
        assert_eq!(text.style.font_size, 40.0);
    }

    #[test]
    fn color_variables_propagate_and_unlink() {
        let mut doc = doc_with(vec![Layer::rectangle(Frame::new(0.0, 0.0, 10.0, 10.0))]);
        let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
        let id = doc.add_swatch(red);
        let fill = &mut doc.pages[0].layers[0].style.fills[0];
        fill.color = red;
        fill.swatch = Some(id);

        let blue = Color::rgba(0.0, 0.0, 1.0, 1.0);
        doc.set_swatch_color(id, blue);
        assert_eq!(doc.pages[0].layers[0].style.fills[0].color, blue);

        // A hand-picked color breaks the link.
        let mut style = doc.pages[0].layers[0].style.clone();
        style.fills[0].color = red;
        unlink_stale_colors(&mut style, |s| doc.swatch(s).map(|s| s.color));
        assert_eq!(style.fills[0].swatch, None);

        doc.remove_swatch(id);
        assert_eq!(doc.pages[0].layers[0].style.fills[0].swatch, None);
        assert_eq!(doc.pages[0].layers[0].style.fills[0].color, blue);
    }
}
