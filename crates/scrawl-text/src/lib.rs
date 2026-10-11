//! Text layout for text layers, shared by the renderer and the editor.
//!
//! [`TextEngine`] owns parley's font database and layout scratch space plus a cache of built
//! layouts, so rendering and measuring every text layer each frame stays cheap.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
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

/// Where a laid-out text goes inside its layer: the block's vertical offset (vertical alignment),
/// the indent left for list markers, each line's extra drop from paragraph spacing, and the
/// markers themselves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    pub dy: f64,
    pub indent: f64,
    pub line_dy: Vec<f64>,
    /// List markers: the line each goes on, and its text.
    pub markers: Vec<(usize, String)>,
}

impl Placement {
    /// Layout space → layer space for line `line`.
    pub fn line_offset(&self, line: usize) -> Vec2 {
        Vec2::new(self.indent, self.dy + self.line_dy.get(line).copied().unwrap_or(0.0))
    }

    /// The line at layer-space height `y`.
    pub fn line_at_y(&self, layout: &Layout, y: f64) -> usize {
        let n = layout.len();
        for (i, line) in layout.lines().enumerate() {
            let bottom = line.metrics().block_max_coord as f64 + self.line_offset(i).y;
            if y < bottom {
                return i;
            }
        }
        n.saturating_sub(1)
    }

    /// Layer space → layout space, for hit testing.
    pub fn to_layout(&self, layout: &Layout, p: Point) -> Point {
        p - self.line_offset(self.line_at_y(layout, p.y))
    }

    /// The line a layout-space box (a caret, say) sits on.
    pub fn line_of_box(&self, layout: &Layout, y0: f64, y1: f64) -> usize {
        let mid = (y0 + y1) / 2.0;
        layout.lines().position(|l| (mid as f32) < l.metrics().block_max_coord).unwrap_or(layout.len().saturating_sub(1))
    }

    /// How much taller paragraph spacing makes the text.
    pub fn extra_height(&self) -> f64 {
        self.line_dy.last().copied().unwrap_or(0.0)
    }
}

/// Walks a path by distance, for setting text along it.
pub struct PathWalker {
    points: Vec<Point>,
    /// Distance from the start to each point.
    along: Vec<f64>,
}

impl PathWalker {
    /// Follows the path's first contour.
    pub fn new(path: &BezPath) -> Self {
        let mut points: Vec<Point> = vec![];
        let mut done = false;
        kurbo::flatten(path.elements().iter().copied(), 0.05, |el| match el {
            kurbo::PathEl::MoveTo(p) if points.is_empty() => points.push(p),
            kurbo::PathEl::MoveTo(_) => done = true,
            kurbo::PathEl::LineTo(p) if !done => points.push(p),
            kurbo::PathEl::ClosePath if !done => {
                if let Some(&first) = points.first() {
                    points.push(first);
                }
                done = true;
            }
            _ => {}
        });
        let mut along = vec![0.0];
        for w in points.windows(2) {
            along.push(along.last().copied().unwrap_or(0.0) + (w[1] - w[0]).hypot());
        }
        Self { points, along }
    }

    pub fn length(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// The point `s` along the path and the direction there (radians); `None` past either end.
    pub fn at(&self, s: f64) -> Option<(Point, f64)> {
        if s < 0.0 || s > self.length() || self.points.len() < 2 {
            return None;
        }
        let i = self.along.partition_point(|&d| d < s).clamp(1, self.points.len() - 1);
        let (a, b) = (self.points[i - 1], self.points[i]);
        let span = (self.along[i] - self.along[i - 1]).max(1e-9);
        let k = (s - self.along[i - 1]) / span;
        Some((a.lerp(b, k), (b - a).atan2()))
    }

    /// Takes a glyph drawn at the origin (baseline at y = 0) to where it sits on the path: its
    /// middle at its distance along it, turned to follow it. `x` and `advance` are the glyph's
    /// position and width in the line; `drop` its baseline's distance below the first line's.
    pub fn place(&self, offset: f64, x: f64, advance: f64, drop: f64) -> Option<Affine> {
        let (p, angle) = self.at(offset + x + advance / 2.0)?;
        Some(Affine::translate(p.to_vec2()) * Affine::rotate(angle) * Affine::translate((-advance / 2.0, drop)))
    }
}

/// The list indent for a style: room for the widest marker.
pub fn list_indent(style: &TextStyle) -> f64 {
    match style.list {
        scrawl_core::text::ListStyle::None => 0.0,
        scrawl_core::text::ListStyle::Bullet => style.font_size * 1.2,
        scrawl_core::text::ListStyle::Numbered => style.font_size * 1.8,
    }
}

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
    if !style.features.is_empty() {
        props.push(StyleProperty::FontFeatures(parley::FontFeatures::Source(style.feature_css().into())));
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

/// The wrap width for a layer: none for auto-sized text, otherwise its frame width less any list
/// indent.
pub fn wrap_width(text: &TextLayer, frame_width: f64) -> Option<f32> {
    match text.sizing {
        TextSizing::Auto => None,
        TextSizing::FixedWidth | TextSizing::Fixed => Some((frame_width - list_indent(&text.style)).max(1.0) as f32),
    }
}

/// Paragraph spacing and list markers for a laid-out `text` (as displayed).
fn place(layout: &Layout, text: &str, style: &TextStyle) -> Placement {
    let mut placement = Placement { indent: list_indent(style), ..Default::default() };
    let mut paragraphs = 0;
    let mut drop = 0.0;
    for (i, line) in layout.lines().enumerate() {
        let start = line.text_range().start;
        let starts_paragraph = start == 0 || text.as_bytes().get(start - 1) == Some(&b'\n');
        if starts_paragraph {
            if paragraphs > 0 {
                drop += style.paragraph_spacing;
            }
            paragraphs += 1;
            // Empty paragraphs get no marker.
            let empty = text[start.min(text.len())..].starts_with('\n') || start >= text.len();
            if !empty && let Some(m) = style.list.marker(paragraphs) {
                placement.markers.push((i, m));
            }
        }
        placement.line_dy.push(drop);
    }
    placement
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

    /// The path a text layer is set along, unless it's being edited (when it lies flat so the
    /// caret works), with the starting offset.
    pub fn path_of(&self, layer: &Layer) -> Option<(PathWalker, f64)> {
        let LayerKind::Text(t) = &layer.kind else { return None };
        let on = t.on_path.as_ref().filter(|_| self.raw_case != Some(layer.id))?;
        Some((PathWalker::new(&on.path.to_absolute_bez_path()), on.offset))
    }

    /// The layout for a text layer at its current frame, and where its lines go in the frame.
    pub fn layer_layout(&mut self, layer: &Layer) -> Option<(&Layout, Placement)> {
        let text = self.effective_text(layer)?;
        let text = &text;
        let frame_h = layer.frame.height;
        let valign = text.style.vertical_alignment;
        let sizing = text.sizing;
        let display = text.display_text();
        // Text on a path is one unwrapped line.
        let width = if text.on_path.is_some() { None } else { wrap_width(text, layer.frame.width) };
        let layout = self.layout(text, width);
        let mut placement = place(layout, &display, &text.style);
        let slack = frame_h - layout.height() as f64 - placement.extra_height();
        placement.dy = match (sizing, valign) {
            (TextSizing::Fixed, VerticalAlign::Middle) => slack / 2.0,
            (TextSizing::Fixed, VerticalAlign::Bottom) => slack,
            _ => 0.0,
        };
        Some((layout, placement))
    }

    /// A list marker laid out in the style of the text it marks.
    pub fn marker_layout(&mut self, style: &TextStyle, marker: &str) -> &Layout {
        let mut t = TextLayer::new(marker);
        t.style = TextStyle { list: Default::default(), paragraph_spacing: 0.0, alignment: TextAlign::Left, ..style.clone() };
        self.layout(&t, None)
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
                let (indent, display) = (list_indent(&text.style), text.display_text());
                let l = self.layout(text, None);
                let extra = place(l, &display, &text.style).extra_height();
                // An empty layout still gets a caret-sized box.
                ((l.width() as f64 + indent).max(1.0), (l.height() as f64 + extra).max(text.style.font_size))
            }
            TextSizing::FixedWidth => {
                let display = text.display_text();
                let l = self.layout(text, wrap_width(text, f.width));
                let extra = place(l, &display, &text.style).extra_height();
                (f.width, (l.height() as f64 + extra).max(text.style.font_size))
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
    /// List markers count as uncolored text.
    pub fn outlines_by_color(&mut self, layer: &Layer) -> Option<Vec<(Option<scrawl_core::Color>, BezPath)>> {
        let walker = self.path_of(layer);
        let (layout, placement) = self.layer_layout(layer)?;
        let mut groups: Vec<(Option<scrawl_core::Color>, BezPath)> = vec![];
        if let Some((walker, offset)) = walker {
            path_outlines(layout, &walker, offset, &mut groups);
            return Some(groups);
        }
        glyph_outlines(layout, |i| placement.line_offset(i), &mut groups);
        let anchors = marker_anchors(layout, &placement);
        let LayerKind::Text(text) = &layer.kind else { return None };
        let style = text.style.clone();
        for (marker, baseline, right) in anchors {
            let m = self.marker_layout(&style, &marker);
            let mb = m.lines().next().map_or(0.0, |l| l.metrics().baseline as f64);
            let at = Vec2::new(right - m.width() as f64, baseline - mb);
            let mut marked = vec![];
            glyph_outlines(m, |_| at, &mut marked);
            for (_, p) in marked {
                push_group(&mut groups, None).extend(p.elements().iter().copied());
            }
        }
        if groups.is_empty() {
            groups.push((None, BezPath::new()));
        }
        Some(groups)
    }
}

/// Each list marker with the layer-space baseline it sits on and the x its right edge aligns to.
pub fn marker_anchors(layout: &Layout, placement: &Placement) -> Vec<(String, f64, f64)> {
    let lines: Vec<f64> = layout.lines().map(|l| l.metrics().baseline as f64).collect();
    let gap = placement.indent * 0.25;
    placement
        .markers
        .iter()
        .filter_map(|(i, m)| Some((m.clone(), lines.get(*i)? + placement.line_offset(*i).y, placement.indent - gap)))
        .collect()
}

fn push_group(groups: &mut Vec<(Option<scrawl_core::Color>, BezPath)>, color: Option<scrawl_core::Color>) -> &mut BezPath {
    let i = match groups.iter().position(|(c, _)| *c == color) {
        Some(i) => i,
        None => {
            groups.push((color, BezPath::new()));
            groups.len() - 1
        }
    };
    &mut groups[i].1
}

/// A glyph's outline at the origin, y-down, with synthetic italic.
fn glyph_path(run: &parley::GlyphRun<'_, RunBrush>, id: u32) -> Option<BezPath> {
    use skrifa::MetadataProvider;
    let r = run.run();
    let font = r.font();
    let font_ref = skrifa::FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let coords: Vec<skrifa::instance::NormalizedCoord> =
        r.normalized_coords().iter().map(|&c| skrifa::instance::NormalizedCoord::from_bits(c.to_bits())).collect();
    let glyph = font_ref.outline_glyphs().get(skrifa::GlyphId::new(id))?;
    let mut pen = KurboPen(BezPath::new());
    glyph.draw(skrifa::outline::DrawSettings::unhinted(skrifa::instance::Size::new(r.font_size()), coords.as_slice()), &mut pen).ok()?;
    let skew = r.synthesis().skew().map(|deg| Affine::skew((deg as f64).to_radians().tan(), 0.0)).unwrap_or(Affine::IDENTITY);
    Some(skew * Affine::scale_non_uniform(1.0, -1.0) * pen.0)
}

/// Appends the outlines of a layout set along a path.
fn path_outlines(layout: &Layout, walker: &PathWalker, offset: f64, groups: &mut Vec<(Option<scrawl_core::Color>, BezPath)>) {
    let first = layout.lines().next().map_or(0.0, |l| l.metrics().baseline as f64);
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let path = push_group(groups, run.style().brush.0);
            let decorations = run_decorations(&run);
            for g in run.positioned_glyphs() {
                let Some(t) = walker.place(offset, g.x as f64, g.advance as f64, g.y as f64 - first) else { continue };
                if let Some(p) = glyph_path(&run, g.id) {
                    path.extend((t * p).elements().iter().copied());
                }
                // Decorations follow the path a glyph-width at a time.
                for &(top, thickness) in &decorations {
                    let r = Rect::new(0.0, top, g.advance as f64, top + thickness);
                    path.extend((t * r.to_path(0.1)).elements().iter().copied());
                }
            }
        }
    }
}

/// A run's underline and strikethrough as (top, thickness) relative to its baseline.
pub fn run_decorations(run: &parley::GlyphRun<'_, RunBrush>) -> Vec<(f64, f64)> {
    let m = run.run().font_metrics();
    let style = run.style();
    [
        (style.underline.is_some(), m.underline_offset, m.underline_size),
        (style.strikethrough.is_some(), m.strikethrough_offset, m.strikethrough_size),
    ]
    .into_iter()
    .filter(|d| d.0)
    .map(|(_, offset, size)| (-(offset as f64), (size as f64).max(1.0)))
    .collect()
}

/// Appends a layout's glyph outlines (and decorations), with line `i` moved by `offset(i)`.
fn glyph_outlines(layout: &Layout, offset: impl Fn(usize) -> Vec2, groups: &mut Vec<(Option<scrawl_core::Color>, BezPath)>) {
    for (li, line) in layout.lines().enumerate() {
        let o = offset(li);
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let path = push_group(groups, run.style().brush.0);
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
                let t = Affine::translate((g.x as f64 + o.x, g.y as f64 + o.y))
                    * skew.unwrap_or(Affine::IDENTITY)
                    * Affine::scale_non_uniform(1.0, -1.0);
                path.extend((t * pen.0).elements().iter().copied());
            }
            // Underlines and strikethroughs, as the renderer draws them.
            let m = r.font_metrics();
            let style = run.style();
            let (x0, x1) = (run.offset() as f64 + o.x, (run.offset() + run.advance()) as f64 + o.x);
            for (on, off, thickness) in [
                (style.underline.is_some(), m.underline_offset, m.underline_size),
                (style.strikethrough.is_some(), m.strikethrough_offset, m.strikethrough_size),
            ] {
                if on {
                    let top = run.baseline() as f64 - off as f64 + o.y;
                    let rect = Rect::new(x0, top, x1, top + (thickness as f64).max(1.0));
                    path.extend(rect.to_path(0.1).elements().iter().copied());
                }
            }
        }
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

    fn text_layer(s: &str, f: impl FnOnce(&mut TextLayer)) -> Layer {
        let mut l = Layer::text(Point::ORIGIN, s);
        if let LayerKind::Text(t) = &mut l.kind {
            f(t);
        }
        l
    }

    #[test]
    fn paragraph_spacing_adds_space_between_paragraphs() {
        let mut e = engine();
        let mut plain = text_layer("one\ntwo\nthree", |_| {});
        let mut spaced = text_layer("one\ntwo\nthree", |t| t.style.paragraph_spacing = 10.0);
        e.fit(&mut plain);
        e.fit(&mut spaced);
        assert!((spaced.frame.height - plain.frame.height - 20.0).abs() < 0.01, "{} vs {}", spaced.frame.height, plain.frame.height);
        let (_, p) = e.layer_layout(&spaced).unwrap();
        assert_eq!(p.line_dy, [0.0, 10.0, 20.0]);
        // Wrapped lines within a paragraph don't get the spacing.
        let mut wrapped = text_layer("one two three four five six seven eight\nnine", |t| {
            t.style.paragraph_spacing = 10.0;
            t.sizing = TextSizing::FixedWidth;
        });
        wrapped.frame.width = 60.0;
        e.fit(&mut wrapped);
        let (_, p) = e.layer_layout(&wrapped).unwrap();
        let last = *p.line_dy.last().unwrap();
        assert_eq!(last, 10.0);
        assert!(p.line_dy.iter().filter(|&&d| d == 0.0).count() > 1);
    }

    #[test]
    fn lists_indent_and_mark_paragraphs() {
        let mut e = engine();
        let mut plain = text_layer("apples\n\npears", |_| {});
        let mut list = text_layer("apples\n\npears", |t| t.style.list = scrawl_core::text::ListStyle::Numbered);
        e.fit(&mut plain);
        e.fit(&mut list);
        assert!(list.frame.width > plain.frame.width + 20.0);
        let (_, p) = e.layer_layout(&list).unwrap();
        // The empty middle paragraph is skipped, but still counted.
        assert_eq!(p.markers.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>(), ["1.", "3."]);
        // Marker glyphs sit in the indent, left of the text.
        let b = e.outlines(&list).unwrap().bounding_box();
        assert!(b.x0 < p.indent * 0.5, "{b:?}");
    }

    #[test]
    fn opentype_features_change_shaping() {
        let mut e = engine();
        // Tabular figures make every digit as wide; proportional "1"s are usually narrower.
        let widths = |e: &mut TextEngine, digits: &str, tnum: bool| {
            let mut l = text_layer(digits, |t| {
                t.style.font_family = "Helvetica Neue".into();
                if tnum {
                    t.style.set_feature("tnum", true);
                }
            });
            e.fit(&mut l);
            l.frame.width
        };
        let (p1, p0) = (widths(&mut e, "1111", false), widths(&mut e, "0000", false));
        let (t1, t0) = (widths(&mut e, "1111", true), widths(&mut e, "0000", true));
        // Only meaningful where the font has proportional figures by default.
        if (p1 - p0).abs() > 0.5 {
            assert!((t1 - t0).abs() < (p1 - p0).abs(), "tnum evens out digits: {t1}/{t0} vs {p1}/{p0}");
        }
        let mut s = TextStyle::default();
        s.set_feature("liga", true);
        assert!(s.features.is_empty(), "on by default: nothing stored");
        s.set_feature("liga", false);
        assert_eq!(s.feature_css(), "\"liga\" 0");
    }

    #[test]
    fn text_follows_its_path() {
        use kurbo::Circle;
        let mut e = engine();
        let circle = Circle::new((100.0, 100.0), 80.0).to_path(0.1);
        let mut l = text_layer("Around and around the garden we go", |t| {
            t.style.font_size = 14.0;
            t.sizing = TextSizing::Fixed;
            t.on_path = Some(scrawl_core::text::TextPath { path: scrawl_core::PathShape::from_bez_path(&circle), offset: 0.0 });
        });
        l.frame = scrawl_core::Frame::new(0.0, 0.0, 200.0, 200.0);
        let b = e.outlines(&l).unwrap().bounding_box();
        // About half the circle's worth of text curves round it rather than running off in a
        // 250-point line.
        assert!(b.width() < 220.0 && b.height() > 60.0, "{b:?}");
        // While editing, it lies flat for the caret.
        e.raw_case = Some(l.id);
        let flat = e.outlines(&l).unwrap().bounding_box();
        assert!(flat.height() < 30.0, "{flat:?}");
        e.raw_case = None;
        // Underlines follow the path too.
        if let LayerKind::Text(t) = &mut l.kind {
            t.style.underline = true;
        }
        let underlined = e.outlines(&l).unwrap().elements().len();
        if let LayerKind::Text(t) = &mut l.kind {
            t.style.underline = false;
        }
        assert!(underlined > e.outlines(&l).unwrap().elements().len() + 20, "a segment per glyph");
        // Glyphs past the end of a short path are left out.
        let short = kurbo::Line::new((0.0, 0.0), (30.0, 0.0)).to_path(0.1);
        if let LayerKind::Text(t) = &mut l.kind {
            t.on_path = Some(scrawl_core::text::TextPath { path: scrawl_core::PathShape::from_bez_path(&short), offset: 0.0 });
        }
        let b = e.outlines(&l).unwrap().bounding_box();
        assert!(b.width() < 40.0, "{b:?}");
    }

    #[test]
    fn walking_a_path() {
        let w = PathWalker::new(&kurbo::Line::new((0.0, 0.0), (0.0, 50.0)).to_path(0.1));
        assert!((w.length() - 50.0).abs() < 1e-9);
        let (p, angle) = w.at(20.0).unwrap();
        assert!((p.y - 20.0).abs() < 1e-9 && (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!(w.at(60.0).is_none() && w.at(-1.0).is_none());
    }

    #[test]
    fn families_are_listed() {
        let mut e = engine();
        assert!(!e.family_names().is_empty());
    }
}
