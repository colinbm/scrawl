//! Text layout for text layers, shared by the renderer and the editor.
//!
//! [`TextEngine`] owns parley's font database and layout scratch space plus a cache of built
//! layouts, so rendering and measuring every text layer each frame stays cheap.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use kurbo::{Affine, BezPath, Rect, Shape};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty,
};
use scrawl_core::{Layer, LayerKind, TextAlign, TextLayer, TextSizing, TextStyle, VerticalAlign};

pub use parley;

/// A glyph run's own color, from a style run; `None` paints with the layer's fills.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunBrush(pub Option<scrawl_core::Color>);

pub type Layout = parley::Layout<RunBrush>;

/// The font stack used when a layer names no family, or as a fallback after it.
const FALLBACK_FAMILIES: &str = "system-ui, sans-serif";

/// The largest number of cached layouts before the cache is cleared.
const CACHE_LIMIT: usize = 1024;

pub struct TextEngine {
    /// A layer laid out without its case transform, because it's being edited.
    pub raw_case: Option<scrawl_core::LayerId>,
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<RunBrush>,
    cache: HashMap<u64, Layout>,
    families: Option<Vec<String>>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// The CSS font-family list for a style: the chosen family, then fallbacks.
pub fn family_source(style: &TextStyle) -> String {
    if style.font_family.trim().is_empty() {
        FALLBACK_FAMILIES.to_owned()
    } else {
        format!("\"{}\", {FALLBACK_FAMILIES}", style.font_family.replace('"', ""))
    }
}

/// The parley style properties for a text style (excluding alignment, which is applied after
/// line breaking).
pub fn style_properties(style: &TextStyle) -> Vec<StyleProperty<'static, RunBrush>> {
    let mut props = vec![
        StyleProperty::FontFamily(FontFamily::Source(family_source(style).into())),
        StyleProperty::FontSize(style.font_size as f32),
        StyleProperty::FontWeight(FontWeight::new(style.font_weight as f32)),
        StyleProperty::FontStyle(if style.italic { FontStyle::Italic } else { FontStyle::Normal }),
        StyleProperty::LetterSpacing(style.letter_spacing as f32),
        StyleProperty::Underline(style.underline),
        StyleProperty::Strikethrough(style.strikethrough),
    ];
    if let Some(lh) = style.line_height {
        props.push(StyleProperty::LineHeight(LineHeight::Absolute(lh as f32)));
    }
    props
}

pub fn alignment(align: TextAlign) -> Alignment {
    match align {
        TextAlign::Left => Alignment::Left,
        TextAlign::Center => Alignment::Center,
        TextAlign::Right => Alignment::Right,
        TextAlign::Justify => Alignment::Justify,
    }
}

/// The wrap width for a layer: none for auto-sized text, otherwise its frame width.
pub fn wrap_width(text: &TextLayer, frame_width: f64) -> Option<f32> {
    match text.sizing {
        TextSizing::Auto => None,
        TextSizing::FixedWidth | TextSizing::Fixed => Some(frame_width.max(1.0) as f32),
    }
}


impl TextEngine {
    pub fn new() -> Self {
        Self {
            raw_case: None,
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            cache: HashMap::new(),
            families: None,
        }
    }

    /// Lays out `text` wrapped at `width` (or unwrapped for `None`). Cached by content and
    /// style.
    pub fn layout(&mut self, text: &TextLayer, width: Option<f32>) -> &Layout {
        let key = {
            let mut h = DefaultHasher::new();
            text.text.hash(&mut h);
            format!("{:?}{:?}", text.style, text.runs).hash(&mut h);
            width.map(f32::to_bits).hash(&mut h);
            h.finish()
        };
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= CACHE_LIMIT {
                self.cache.clear();
            }
            let display = text.display_text();
            let mut builder = self.layout_cx.ranged_builder(&mut self.font_cx, &display, 1.0, true);
            for p in style_properties(&text.style) {
                builder.push_default(p);
            }
            // A case transform that changes byte lengths (ß → SS) would misplace the runs.
            if display.len() == text.text.len() {
                for run in &text.runs {
                    let range = run.start..run.end;
                    let styled = run.style.apply(&text.style);
                    for p in style_properties(&styled) {
                        builder.push(p, range.clone());
                    }
                    builder.push(StyleProperty::Brush(RunBrush(run.style.color)), range);
                }
            }
            let mut layout: Layout = builder.build(&display);
            layout.break_all_lines(width);
            let options = AlignmentOptions { align_when_overflowing: false, ..Default::default() };
            // Auto-width text aligns within its own (widest-line) width.
            layout.align(alignment(text.style.alignment), options);
            self.cache.insert(key, layout);
        }
        &self.cache[&key]
    }

    /// A layer's text as it should be laid out: without the case transform while editing.
    fn effective_text(&self, layer: &Layer) -> Option<TextLayer> {
        let LayerKind::Text(text) = &layer.kind else { return None };
        let mut text = text.clone();
        if self.raw_case == Some(layer.id) {
            text.style.transform = scrawl_core::TextTransform::None;
        }
        Some(text)
    }

    /// The layout for a text layer at its current frame, and the vertical offset of the text
    /// within the frame.
    pub fn layer_layout(&mut self, layer: &Layer) -> Option<(&Layout, f64)> {
        let text = self.effective_text(layer)?;
        let text = &text;
        let frame_h = layer.frame.height;
        let valign = text.style.vertical_alignment;
        let sizing = text.sizing;
        let layout = self.layout(text, wrap_width(text, layer.frame.width));
        let slack = frame_h - layout.height() as f64;
        let dy = match (sizing, valign) {
            (TextSizing::Fixed, VerticalAlign::Middle) => slack / 2.0,
            (TextSizing::Fixed, VerticalAlign::Bottom) => slack,
            _ => 0.0,
        };
        Some((layout, dy))
    }

    /// The frame size a text layer should have given its sizing mode, if it differs from the
    /// current one.
    pub fn fitted_size(&mut self, layer: &Layer) -> Option<(f64, f64)> {
        let text = self.effective_text(layer)?;
        let text = &text;
        let f = layer.frame;
        let size = match text.sizing {
            TextSizing::Fixed => return None,
            TextSizing::Auto => {
                let l = self.layout(text, None);
                // An empty layout still gets a caret-sized box.
                ((l.width() as f64).max(1.0), (l.height() as f64).max(text.style.font_size))
            }
            TextSizing::FixedWidth => {
                let l = self.layout(text, wrap_width(text, f.width));
                (f.width, (l.height() as f64).max(text.style.font_size))
            }
        };
        let close = (size.0 - f.width).abs() < 1e-3 && (size.1 - f.height).abs() < 1e-3;
        (!close).then_some(size)
    }

    /// Resizes a text layer's frame to fit its content, keeping its top-left corner fixed.
    /// Returns whether it changed.
    pub fn fit(&mut self, layer: &mut Layer) -> bool {
        match self.fitted_size(layer) {
            Some((w, h)) => {
                layer.reframe(Rect::new(0.0, 0.0, w, h));
                true
            }
            None => false,
        }
    }

    /// Fits every text layer under `layers` (recursively). Returns whether any changed.
    pub fn fit_all(&mut self, layers: &mut [Layer]) -> bool {
        let mut changed = false;
        for l in layers {
            if l.is_text() {
                changed |= self.fit(l);
            }
            let group = l.is_group();
            if let LayerKind::Symbol(inst) = &mut l.kind {
                changed |= self.fit_all(&mut inst.expanded);
            }
            if let Some(children) = l.children_mut() {
                let child_changed = self.fit_all(children);
                if child_changed && group {
                    l.fit_to_children();
                }
                changed |= child_changed;
            }
        }
        changed
    }

    /// Fits the text layers on every page of a document, after rebuilding symbol instances.
    pub fn fit_document(&mut self, doc: &mut scrawl_core::Document) {
        scrawl_core::symbols::sync_instances(doc, &mut |layers| {
            self.fit_all(layers);
        });
        for page in &mut doc.pages {
            self.fit_all(&mut page.layers);
        }
    }

    /// Installed font family names, sorted.
    pub fn family_names(&mut self) -> &[String] {
        if self.families.is_none() {
            let mut names: Vec<String> = self.font_cx.collection.family_names().map(str::to_owned).collect();
            names.sort_by_key(|n| n.to_lowercase());
            names.dedup();
            self.families = Some(names);
        }
        self.families.as_deref().unwrap_or_default()
    }

    /// The glyph outlines of a text layer in its local space, for "Convert to Outlines".
    pub fn outlines(&mut self, layer: &Layer) -> Option<BezPath> {
        let mut all = BezPath::new();
        for (_, p) in self.outlines_by_color(layer)? {
            all.extend(p.elements().iter().copied());
        }
        Some(all)
    }

    /// Glyph outlines grouped by run color: `None` for text painted with the layer's fills.
    pub fn outlines_by_color(&mut self, layer: &Layer) -> Option<Vec<(Option<scrawl_core::Color>, BezPath)>> {
        let (layout, dy) = self.layer_layout(layer)?;
        let mut groups: Vec<(Option<scrawl_core::Color>, BezPath)> = vec![];
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let color = run.style().brush.0;
                let i = match groups.iter().position(|(c, _)| *c == color) {
                    Some(i) => i,
                    None => {
                        groups.push((color, BezPath::new()));
                        groups.len() - 1
                    }
                };
                let path = &mut groups[i].1;
                let r = run.run();
                let font = r.font();
                let Ok(font_ref) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else { continue };
                use skrifa::MetadataProvider;
                let outlines = font_ref.outline_glyphs();
                let coords: Vec<skrifa::instance::NormalizedCoord> =
                    r.normalized_coords().iter().map(|&c| skrifa::instance::NormalizedCoord::from_bits(c.to_bits())).collect();
                let size = skrifa::instance::Size::new(r.font_size());
                let skew = r.synthesis().skew().map(|deg| Affine::skew((deg as f64).to_radians().tan(), 0.0));
                for g in run.positioned_glyphs() {
                    let Some(glyph) = outlines.get(skrifa::GlyphId::new(g.id)) else { continue };
                    let mut pen = KurboPen(BezPath::new());
                    let settings = skrifa::outline::DrawSettings::unhinted(size, coords.as_slice());
                    if glyph.draw(settings, &mut pen).is_err() {
                        continue;
                    }
                    // Font units are y-up; flip into our y-down space at the glyph's origin.
                    let t = Affine::translate((g.x as f64, g.y as f64 + dy))
                        * skew.unwrap_or(Affine::IDENTITY)
                        * Affine::scale_non_uniform(1.0, -1.0);
                    path.extend((t * pen.0).elements().iter().copied());
                }
                // Underlines and strikethroughs, as the renderer draws them.
                let m = r.font_metrics();
                let style = run.style();
                let (x0, x1) = (run.offset() as f64, (run.offset() + run.advance()) as f64);
                for (on, offset, thickness) in [
                    (style.underline.is_some(), m.underline_offset, m.underline_size),
                    (style.strikethrough.is_some(), m.strikethrough_offset, m.strikethrough_size),
                ] {
                    if on {
                        let top = run.baseline() as f64 - offset as f64 + dy;
                        let rect = Rect::new(x0, top, x1, top + (thickness as f64).max(1.0));
                        path.extend(rect.to_path(0.1).elements().iter().copied());
                    }
                }
            }
        }
        if groups.is_empty() {
            groups.push((None, BezPath::new()));
        }
        Some(groups)
    }
}

struct KurboPen(BezPath);

impl skrifa::outline::OutlinePen for KurboPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, y as f64));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((cx0 as f64, cy0 as f64), (x as f64, y as f64));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((cx0 as f64, cy0 as f64), (cx1 as f64, cy1 as f64), (x as f64, y as f64));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Point, Shape};

    fn engine() -> TextEngine {
        TextEngine::new()
    }

    #[test]
    fn auto_text_fits_its_content() {
        let mut e = engine();
        let mut l = Layer::text(Point::new(10.0, 20.0), "Hello");
        assert!(e.fit(&mut l));
        assert!(l.frame.width > 20.0 && l.frame.height >= 16.0, "{:?}", l.frame);
        assert_eq!((l.frame.x, l.frame.y), (10.0, 20.0));
        // Longer text is wider; fitting again with no change is a no-op.
        let w = l.frame.width;
        assert!(!e.fit(&mut l));
        if let LayerKind::Text(t) = &mut l.kind {
            t.text = "Hello, world".into();
        }
        e.fit(&mut l);
        assert!(l.frame.width > w);
    }

    #[test]
    fn fixed_width_wraps_and_grows_down() {
        let mut e = engine();
        let mut l = Layer::text(Point::ORIGIN, "one two three four five six seven eight nine ten");
        e.fit(&mut l);
        let one_line = l.frame.height;
        if let LayerKind::Text(t) = &mut l.kind {
            t.sizing = TextSizing::FixedWidth;
        }
        l.frame.width = 80.0;
        e.fit(&mut l);
        assert_eq!(l.frame.width, 80.0);
        assert!(l.frame.height > one_line * 2.0, "{} vs {}", l.frame.height, one_line);
    }

    #[test]
    fn outlines_cover_the_text() {
        let mut e = engine();
        let mut l = Layer::text(Point::ORIGIN, "Hi");
        if let LayerKind::Text(t) = &mut l.kind {
            t.style.font_size = 40.0;
        }
        e.fit(&mut l);
        let path = e.outlines(&l).expect("text layer");
        let b = path.bounding_box();
        assert!(b.width() > 10.0 && b.height() > 10.0, "{b:?}");
        // Glyphs sit inside the frame (allowing for side bearings).
        assert!(b.y0 >= -2.0 && b.y1 <= l.frame.height + 2.0, "{b:?} in {:?}", l.frame);
    }

    #[test]
    fn families_are_listed() {
        let mut e = engine();
        assert!(!e.family_names().is_empty());
    }
}
