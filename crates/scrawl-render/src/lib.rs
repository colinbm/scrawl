//! Builds Vello scenes from Scrawl documents.

pub mod export;
pub mod images;
mod shadow;

pub use images::ImageCache;

use scrawl_core::style::stroke_style;
use scrawl_core::{
    BlendMode as SBlend, BorderPosition, Color, FillRule, GradientKind, Layer, LayerKind, Marker, Page, Paint,
};
use vello::Scene;
use vello::kurbo::{Affine, BezPath, Circle, Line, ParamCurve, ParamCurveDeriv, PathSeg, Point, Rect, Shape, Vec2};
use vello::peniko::{self, BlendMode, Brush, Compose, Fill, Gradient, Mix, StyleRef};

pub use scrawl_text::TextEngine;
use scrawl_text::parley::PositionedLayoutItem;

pub fn to_peniko(c: Color) -> peniko::Color {
    peniko::Color::new([c.r, c.g, c.b, c.a])
}

pub fn to_mix(b: SBlend) -> Mix {
    match b {
        SBlend::Normal => Mix::Normal,
        SBlend::Darken => Mix::Darken,
        SBlend::Multiply => Mix::Multiply,
        SBlend::ColorBurn => Mix::ColorBurn,
        SBlend::Lighten => Mix::Lighten,
        SBlend::Screen => Mix::Screen,
        SBlend::ColorDodge => Mix::ColorDodge,
        SBlend::Overlay => Mix::Overlay,
        SBlend::SoftLight => Mix::SoftLight,
        SBlend::HardLight => Mix::HardLight,
        SBlend::Difference => Mix::Difference,
        SBlend::Exclusion => Mix::Exclusion,
        SBlend::Hue => Mix::Hue,
        SBlend::Saturation => Mix::Saturation,
        SBlend::Color => Mix::Color,
        SBlend::Luminosity => Mix::Luminosity,
    }
}

/// What rendering needs beyond the page: text layout, and decoded images.
struct Resources<'a> {
    text: &'a mut TextEngine,
    images: &'a ImageCache,
}

/// A brush for `paint` on a layer of the given size, plus the brush transform that maps the
/// gradient's normalized (0..1) geometry or the image onto the frame, and the area to paint when
/// it's smaller than the shape (a fitted image).
pub fn brush(paint: Paint<'_>, width: f64, height: f64, images: &ImageCache) -> (Brush, Option<Affine>, Option<Rect>) {
    match paint {
        Paint::Color(c) => (Brush::Solid(to_peniko(c)), None, None),
        Paint::Image { id, mode, scale } => match images.get(id) {
            Some(image) => {
                let (brush, t, area) = images::fill_brush(image, mode, scale, width, height);
                (brush, Some(t), area)
            }
            None => (Brush::Solid(peniko::Color::TRANSPARENT), None, None),
        },
        Paint::Gradient(g) => {
            let stops: Vec<(f32, peniko::Color)> = g.stops.iter().map(|s| (s.position as f32, to_peniko(s.color))).collect();
            let gradient = match g.kind {
                GradientKind::Linear => Gradient::new_linear(g.from, g.to),
                GradientKind::Radial => Gradient::new_radial(g.from, (g.to - g.from).hypot().max(1e-6) as f32),
                GradientKind::Angular => {
                    let start = -std::f32::consts::FRAC_PI_2;
                    Gradient::new_sweep((0.5, 0.5), start, start + std::f32::consts::TAU)
                }
            }
            .with_stops(stops.as_slice());
            // Degenerate dimensions (lines) would make the transform singular.
            let scale = Affine::scale_non_uniform(width.max(1e-3), height.max(1e-3));
            (Brush::Gradient(gradient), Some(scale), None)
        }
    }
}

fn fill_rule(layer: &Layer) -> Fill {
    match layer.fill_rule() {
        FillRule::NonZero => Fill::NonZero,
        FillRule::EvenOdd => Fill::EvenOdd,
    }
}

/// Appends `page` to `scene`, with `view` mapping page space to target pixels.
pub fn render_page(scene: &mut Scene, page: &Page, view: Affine, text: &mut TextEngine, images: &ImageCache) {
    let res = &mut Resources { text, images };
    // Artboard drop shadows go underneath everything.
    for layer in page.layers.iter().filter(|l| l.visible && l.is_artboard()) {
        let t = view * layer.local_transform();
        scene.draw_blurred_rounded_rect(
            t * Affine::translate((0.0, 1.0)),
            layer.frame.local_rect(),
            peniko::Color::new([0.0, 0.0, 0.0, 0.18]),
            0.0,
            2.0,
        );
    }
    render_children(scene, &page.layers, view, res);
}

/// Renders siblings bottom to top. A mask layer clips everything above it in the same parent.
fn render_children(scene: &mut Scene, children: &[Layer], parent: Affine, res: &mut Resources<'_>) {
    let mut clips = 0;
    for child in children {
        render_layer(scene, child, parent, res);
        if child.is_mask
            && child.visible
            && let Some(path) = child.local_path()
        {
            scene.push_clip_layer(fill_rule(child), parent * child.local_transform(), &path);
            clips += 1;
        }
    }
    for _ in 0..clips {
        scene.pop_layer();
    }
}

fn render_layer(scene: &mut Scene, layer: &Layer, parent: Affine, res: &mut Resources<'_>) {
    if !layer.visible || layer.opacity <= 0.0 {
        return;
    }
    let t = parent * layer.local_transform();
    let needs_group = layer.opacity < 1.0 || layer.blend_mode != SBlend::Normal;
    if needs_group {
        // The group's clip is the layer's painted bounds in its parent space.
        let Some(mut bounds) = layer.bounds_in(Affine::IDENTITY) else { return };
        if let LayerKind::Symbol(s) = &layer.kind {
            // Instance contents (e.g. long override text) may spill out of the frame.
            let t = layer.local_transform();
            bounds = s.expanded.iter().filter_map(|c| c.bounds_in(t)).fold(bounds, |a, b| a.union(b));
        }
        let pad = max_paint_outset(layer) + 1.0;
        scene.push_layer(Fill::NonZero, to_mix(layer.blend_mode), layer.opacity as f32, parent, &bounds.inflate(pad, pad));
    }

    match &layer.kind {
        LayerKind::Artboard(ab) => {
            let rect = layer.frame.local_rect();
            scene.fill(Fill::NonZero, t, to_peniko(ab.background.unwrap_or(Color::WHITE)), None, &rect);
            if ab.clip_content {
                scene.push_clip_layer(Fill::NonZero, t, &rect);
            }
            render_children(scene, &ab.children, t, res);
            if ab.clip_content {
                scene.pop_layer();
            }
        }
        LayerKind::Group(g) => render_children(scene, &g.children, t, res),
        LayerKind::Symbol(s) => render_children(scene, &s.expanded, t, res),
        LayerKind::Text(_) => render_text(scene, layer, parent, res),
        LayerKind::Image(img) => render_image(scene, layer, &img.image, parent, res.images),
        _ => {
            if let Some(path) = layer.local_path() {
                render_shape(scene, layer, &path, parent, res.images);
            }
        }
    }

    if needs_group {
        scene.pop_layer();
    }
}

/// The furthest a layer or any descendant paints outside its own geometry.
fn max_paint_outset(layer: &Layer) -> f64 {
    let own = layer.style.paint_outset();
    let children = match &layer.kind {
        LayerKind::Symbol(s) => Some(&s.expanded),
        _ => layer.children(),
    };
    children.map_or(own, |c| c.iter().map(max_paint_outset).fold(own, f64::max))
}

/// A bitmap stretched to the frame, with the layer's shadows and borders. Missing images show a
/// crossed-out placeholder.
fn render_image(scene: &mut Scene, layer: &Layer, id: &str, parent: Affine, images: &ImageCache) {
    let t = parent * layer.local_transform();
    let rect = layer.frame.local_rect();
    for s in layer.style.shadows.iter().filter(|s| s.enabled) {
        shadow::drop_shadow(scene, layer, &rect.to_path(0.1), Fill::NonZero, parent, s);
    }
    match images.get(id) {
        Some(image) => {
            let fit = Affine::scale_non_uniform(rect.width() / image.width.max(1) as f64, rect.height() / image.height.max(1) as f64);
            scene.draw_image(image, t * fit);
        }
        None => {
            let grey = peniko::Color::new([0.85, 0.85, 0.85, 1.0]);
            scene.fill(Fill::NonZero, t, grey, None, &rect);
            let stroke = vello::kurbo::Stroke::new(1.0);
            let dark = peniko::Color::new([0.6, 0.6, 0.6, 1.0]);
            scene.stroke(&stroke, t, dark, None, &Line::new((rect.x0, rect.y0), (rect.x1, rect.y1)));
            scene.stroke(&stroke, t, dark, None, &Line::new((rect.x1, rect.y0), (rect.x0, rect.y1)));
        }
    }
    // Fills and borders paint over the bitmap, as usual for image layers.
    let mut over = layer.clone();
    over.style.shadows.clear();
    render_shape(scene, &over, &rect.to_path(0.1), parent, images);
}

/// Fills `path` with a brush, clipped to `area` when the paint covers only part of the shape.
#[allow(clippy::too_many_arguments)]
fn fill_with(scene: &mut Scene, rule: Fill, t: Affine, brush: &Brush, bt: Option<Affine>, path: &BezPath, area: Option<Rect>) {
    match area {
        Some(area) => {
            scene.push_clip_layer(rule, t, path);
            scene.fill(Fill::NonZero, t, brush, bt, &area);
            scene.pop_layer();
        }
        None => scene.fill(rule, t, brush, bt, path),
    }
}

fn render_shape(scene: &mut Scene, layer: &Layer, path: &BezPath, parent: Affine, images: &ImageCache) {
    let t = parent * layer.local_transform();
    let rule = fill_rule(layer);
    let (w, h) = (layer.frame.width, layer.frame.height);
    let style = &layer.style;

    for s in style.shadows.iter().filter(|s| s.enabled) {
        shadow::drop_shadow(scene, layer, path, rule, parent, s);
    }

    if layer.is_closed() {
        for fill in style.fills.iter().filter(|f| f.enabled) {
            let (brush, bt, area) = brush(fill.paint(), w, h, images);
            if fill.blend_mode == SBlend::Normal {
                fill_with(scene, rule, t, &brush, bt, path, area);
            } else {
                scene.push_layer(rule, to_mix(fill.blend_mode), 1.0, t, path);
                fill_with(scene, rule, t, &brush, bt, path, area);
                scene.pop_layer();
            }
        }
        for s in style.inner_shadows.iter().filter(|s| s.enabled) {
            shadow::inner_shadow(scene, layer, path, rule, parent, s);
        }
    }

    let opts = &style.border_options;
    for border in style.borders.iter().filter(|b| b.enabled && b.thickness > 0.0) {
        let (brush, bt, _) = brush(border.paint(), w, h, images);
        // Open paths have no inside or outside, so their borders are always centered.
        let position = if layer.is_closed() { border.position } else { BorderPosition::Center };
        match position {
            BorderPosition::Center => {
                scene.stroke(&stroke_style(border.thickness, opts), t, &brush, bt, path);
            }
            BorderPosition::Inside => {
                // A double-width stroke clipped to the shape leaves exactly the inner half.
                scene.push_clip_layer(rule, t, path);
                scene.stroke(&stroke_style(border.thickness * 2.0, opts), t, &brush, bt, path);
                scene.pop_layer();
            }
            BorderPosition::Outside => {
                // A double-width stroke with the shape's interior knocked out.
                let bounds: Rect = path.bounding_box().inflate(border.thickness + 1.0, border.thickness + 1.0);
                scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t, &bounds);
                scene.stroke(&stroke_style(border.thickness * 2.0, opts), t, &brush, bt, path);
                scene.push_layer(rule, BlendMode::new(Mix::Normal, Compose::DestOut), 1.0, t, path);
                scene.fill(rule, t, peniko::Color::BLACK, None, path);
                scene.pop_layer();
                scene.pop_layer();
            }
        }
        if !layer.is_closed() {
            render_markers(scene, path, t, border.thickness, &brush, bt, opts.start_marker, opts.end_marker);
        }
    }
}

fn render_text(scene: &mut Scene, layer: &Layer, parent: Affine, res: &mut Resources<'_>) {
    let (engine, images) = (&mut *res.text, res.images);
    let style = &layer.style;
    let t = parent * layer.local_transform();
    let (w, h) = (layer.frame.width, layer.frame.height);

    // Shadows are cast by the glyph outlines.
    let wants_outlines = style.shadows.iter().chain(&style.inner_shadows).any(|s| s.enabled);
    let outlines = if wants_outlines { engine.outlines(layer) } else { None };
    if let Some(path) = &outlines {
        for s in style.shadows.iter().filter(|s| s.enabled) {
            shadow::drop_shadow(scene, layer, path, Fill::NonZero, parent, s);
        }
    }

    let Some((layout, dy)) = engine.layer_layout(layer) else { return };
    let text_t = t * Affine::translate((0.0, dy));
    for fill in style.fills.iter().filter(|f| f.enabled) {
        let (brush, bt, _) = brush(fill.paint(), w, h, images);
        // Gradients map onto the frame, not the (vertically offset) text block.
        let bt = bt.map(|b| Affine::translate((0.0, -dy)) * b);
        if fill.blend_mode == SBlend::Normal {
            draw_layout(scene, layout, text_t, &brush, bt, Fill::NonZero.into());
        } else {
            let bounds = layer.frame.local_rect().inflate(w.max(h), w.max(h));
            scene.push_layer(Fill::NonZero, to_mix(fill.blend_mode), 1.0, t, &bounds);
            draw_layout(scene, layout, text_t, &brush, bt, Fill::NonZero.into());
            scene.pop_layer();
        }
    }
    if let Some(path) = &outlines {
        for s in style.inner_shadows.iter().filter(|s| s.enabled) {
            shadow::inner_shadow(scene, layer, path, Fill::NonZero, parent, s);
        }
    }
    for border in style.borders.iter().filter(|b| b.enabled && b.thickness > 0.0) {
        let (brush, bt, _) = brush(border.paint(), w, h, images);
        let bt = bt.map(|b| Affine::translate((0.0, -dy)) * b);
        let stroke = stroke_style(border.thickness, &style.border_options);
        draw_layout(scene, layout, text_t, &brush, bt, (&stroke).into());
    }
}

/// Draws every glyph run of a layout, plus underlines and strikethroughs.
fn draw_layout(scene: &mut Scene, layout: &scrawl_text::Layout, t: Affine, brush: &Brush, bt: Option<Affine>, style: StyleRef<'_>) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let r = run.run();
            let coords: Vec<i16> = r.normalized_coords().iter().map(|c| c.to_bits()).collect();
            let skew = r.synthesis().skew().map(|deg| Affine::skew((deg as f64).to_radians().tan(), 0.0));
            scene
                .draw_glyphs(r.font())
                .font_size(r.font_size())
                .transform(t)
                .glyph_transform(skew)
                .normalized_coords(&coords)
                .brush(brush)
                .brush_transform(bt)
                .hint(false)
                .draw(style, run.positioned_glyphs().map(|g| vello::Glyph { id: g.id, x: g.x, y: g.y }));

            let m = r.font_metrics();
            let s = run.style();
            let (x0, x1) = (run.offset() as f64, (run.offset() + run.advance()) as f64);
            let base = run.baseline() as f64;
            for (on, offset, size) in [
                (s.underline.is_some(), m.underline_offset, m.underline_size),
                (s.strikethrough.is_some(), m.strikethrough_offset, m.strikethrough_size),
            ] {
                if on {
                    // Offsets are the top of the decoration above the baseline, in y-up units.
                    let top = base - offset as f64;
                    let rect = Rect::new(x0, top, x1, top + (size as f64).max(1.0));
                    scene.fill(Fill::NonZero, t, brush, bt, &rect);
                }
            }
        }
    }
}

/// Arrowheads and other end markers on open paths.
#[allow(clippy::too_many_arguments)]
fn render_markers(
    scene: &mut Scene,
    path: &BezPath,
    t: Affine,
    width: f64,
    brush: &Brush,
    bt: Option<Affine>,
    start: Marker,
    end: Marker,
) {
    let segs: Vec<PathSeg> = path.segments().collect();
    let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return };
    // Tangents pointing outwards from the path at each end.
    let tangent = |seg: &PathSeg, at_end: bool| -> Vec2 {
        let d = match seg {
            PathSeg::Line(l) => l.p1 - l.p0,
            PathSeg::Quad(q) => q.deriv().eval(if at_end { 1.0 } else { 0.0 }).to_vec2(),
            PathSeg::Cubic(c) => c.deriv().eval(if at_end { 1.0 } else { 0.0 }).to_vec2(),
        };
        let d = if at_end { d } else { -d };
        if d.hypot() > 1e-9 { d.normalize() } else { Vec2::new(1.0, 0.0) }
    };
    for (marker, at, dir) in [(start, first.eval(0.0), tangent(first, false)), (end, last.eval(1.0), tangent(last, true))] {
        draw_marker(scene, marker, at, dir, width, t, brush, bt);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_marker(scene: &mut Scene, marker: Marker, at: Point, dir: Vec2, width: f64, t: Affine, brush: &Brush, bt: Option<Affine>) {
    let size = (width * 3.0).max(6.0);
    let normal = Vec2::new(-dir.y, dir.x);
    let stroke = stroke_style(width, &Default::default());
    let back = at - dir * size;
    let (left, right) = (back + normal * size * 0.6, back - normal * size * 0.6);
    match marker {
        Marker::None => {}
        Marker::OpenArrow => {
            let mut p = BezPath::new();
            p.move_to(left);
            p.line_to(at);
            p.line_to(right);
            scene.stroke(&stroke.with_join(vello::kurbo::Join::Miter), t, brush, bt, &p);
        }
        Marker::FilledArrow => {
            let mut p = BezPath::new();
            p.move_to(at + dir * width * 0.5);
            p.line_to(left);
            p.line_to(right);
            p.close_path();
            scene.fill(Fill::NonZero, t, brush, bt, &p);
        }
        Marker::Line => {
            scene.stroke(&stroke, t, brush, bt, &Line::new(at + normal * size * 0.6, at - normal * size * 0.6));
        }
        Marker::OpenCircle | Marker::FilledCircle => {
            let c = Circle::new(at, size * 0.5);
            if marker == Marker::FilledCircle {
                scene.fill(Fill::NonZero, t, brush, bt, &c);
            } else {
                scene.fill(Fill::NonZero, t, peniko::Color::WHITE, None, &c);
                scene.stroke(&stroke, t, brush, bt, &c);
            }
        }
        Marker::OpenSquare | Marker::FilledSquare => {
            let half = size * 0.5;
            let mut p = BezPath::new();
            for (i, (a, b)) in [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)].iter().enumerate() {
                let q = at + dir * half * *a + normal * half * *b;
                if i == 0 { p.move_to(q) } else { p.line_to(q) }
            }
            p.close_path();
            if marker == Marker::FilledSquare {
                scene.fill(Fill::NonZero, t, brush, bt, &p);
            } else {
                scene.fill(Fill::NonZero, t, peniko::Color::WHITE, None, &p);
                scene.stroke(&stroke, t, brush, bt, &p);
            }
        }
    }
}
