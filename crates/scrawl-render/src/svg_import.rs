//! Turning SVG files into layers, via usvg (which resolves styles, `use`, units and transforms).
//!
//! Paths become path layers with their fills and strokes (solid or gradient), groups keep their
//! opacity and blend mode, clip paths become masks, and embedded bitmaps become image layers.
//! Text needs fonts usvg isn't given here, so it's dropped; outline it before exporting.

use scrawl_core::assets::Assets;
use scrawl_core::kurbo::{Affine, BezPath, Point, Rect, Shape};
use scrawl_core::{
    BlendMode, Border, Color, Fill, FillRule, FillType, Gradient, GradientKind, GradientStop, Layer, LineCap, LineJoin, PathShape,
};
use svg2pdf::usvg::{self, Node, tiny_skia_path::PathSegment};

/// The layers of an SVG document, in its own coordinates, grouped under one layer named `name`
/// (or the single top-level layer itself).
pub fn import_svg(data: &[u8], name: &str, assets: &mut Assets) -> Result<Layer, String> {
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).map_err(|e| format!("not a valid SVG: {e}"))?;
    let mut layers = group_children(tree.root(), assets);
    let mut layer = match layers.len() {
        0 => return Err("the SVG has no shapes Scrawl can import".into()),
        1 => layers.pop().expect("one"),
        _ => group(layers).ok_or("the SVG has no visible shapes")?,
    };
    if !name.is_empty() {
        layer.name = name.to_owned();
    }
    Ok(layer)
}

fn affine(t: usvg::Transform) -> Affine {
    Affine::new([t.sx as f64, t.ky as f64, t.kx as f64, t.sy as f64, t.tx as f64, t.ty as f64])
}

fn bez(path: &usvg::tiny_skia_path::Path) -> BezPath {
    let p = |p: usvg::tiny_skia_path::Point| Point::new(p.x as f64, p.y as f64);
    let mut out = BezPath::new();
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(a) => out.move_to(p(a)),
            PathSegment::LineTo(a) => out.line_to(p(a)),
            PathSegment::QuadTo(a, b) => out.quad_to(p(a), p(b)),
            PathSegment::CubicTo(a, b, c) => out.curve_to(p(a), p(b), p(c)),
            PathSegment::Close => out.close_path(),
        }
    }
    out
}

/// Layers given in page space, wrapped in a group fitted around them.
fn group(children: Vec<Layer>) -> Option<Layer> {
    let bounds = children.iter().filter_map(|c| c.bounds_in(Affine::IDENTITY)).reduce(|a, b| a.union(b))?;
    let mut children = children;
    for c in &mut children {
        c.frame.translate(-bounds.origin().to_vec2());
    }
    Some(Layer::group(children, scrawl_core::Frame::from_rect(bounds)))
}

fn group_children(g: &usvg::Group, assets: &mut Assets) -> Vec<Layer> {
    g.children().iter().filter_map(|n| node(n, assets)).collect()
}

fn node(n: &Node, assets: &mut Assets) -> Option<Layer> {
    let mut layer = match n {
        Node::Group(g) => {
            let mut children = group_children(g, assets);
            if let Some(clip) = g.clip_path().and_then(|c| clip_mask(c, g.abs_transform())) {
                children.insert(0, clip);
            }
            let plain = g.opacity().get() >= 1.0 && g.blend_mode() == usvg::BlendMode::Normal && g.clip_path().is_none();
            let mut layer = if children.len() == 1 && plain { children.pop()? } else { group(children)? };
            layer.opacity = g.opacity().get() as f64;
            layer.blend_mode = blend(g.blend_mode());
            layer
        }
        Node::Path(p) if p.is_visible() => path(p)?,
        Node::Image(img) if img.is_visible() => {
            let bytes = match img.kind() {
                usvg::ImageKind::PNG(b) | usvg::ImageKind::JPEG(b) | usvg::ImageKind::GIF(b) | usvg::ImageKind::WEBP(b) => b.to_vec(),
                usvg::ImageKind::SVG(_) => return None,
            };
            let id = assets.add_image(bytes)?;
            let b = img.abs_bounding_box();
            Layer::image(id, scrawl_core::Frame::new(b.x() as f64, b.y() as f64, b.width() as f64, b.height() as f64))
        }
        _ => return None,
    };
    if !n.id().is_empty() {
        layer.name = n.id().to_owned();
    }
    Some(layer)
}

/// A clip path's first shape as a mask layer, in page space.
fn clip_mask(clip: &usvg::ClipPath, owner: usvg::Transform) -> Option<Layer> {
    let t = affine(owner) * affine(clip.transform());
    let shape = clip.root().children().iter().find_map(|n| match n {
        Node::Path(p) => Some(affine(p.abs_transform()) * bez(p.data())),
        _ => None,
    })?;
    let shape = t * shape;
    let mut l = Layer::path(&PathShape::from_bez_path(&shape));
    l.name = "Clip".into();
    l.style = Default::default();
    l.is_mask = true;
    Some(l)
}

fn path(p: &usvg::Path) -> Option<Layer> {
    let t = affine(p.abs_transform());
    let shape = t * bez(p.data());
    if shape.elements().is_empty() {
        return None;
    }
    let mut ps = PathShape::from_bez_path(&shape);
    if let Some(f) = p.fill() {
        ps.fill_rule = match f.rule() {
            usvg::FillRule::NonZero => FillRule::NonZero,
            usvg::FillRule::EvenOdd => FillRule::EvenOdd,
        };
    }
    let mut layer = Layer::path(&ps);
    layer.name = "Path".into();
    let bounds = shape.bounding_box();
    layer.style = Default::default();
    if let Some(f) = p.fill() {
        let (fill_type, color, gradient) = paint(f.paint(), f.opacity().get(), t, bounds);
        layer.style.fills.push(Fill { fill_type, color, gradient, ..Fill::solid(color) });
    }
    if let Some(s) = p.stroke() {
        let (fill_type, color, gradient) = paint(s.paint(), s.opacity().get(), t, bounds);
        // Strokes scale with the transform; use its average scale for the width.
        let scale = t.determinant().abs().sqrt();
        layer.style.borders.push(Border { fill_type, gradient, ..Border::solid(color, s.width().get() as f64 * scale) });
        let o = &mut layer.style.border_options;
        o.cap = match s.linecap() {
            usvg::LineCap::Butt => LineCap::Butt,
            usvg::LineCap::Round => LineCap::Round,
            usvg::LineCap::Square => LineCap::Square,
        };
        o.join = match s.linejoin() {
            usvg::LineJoin::Round => LineJoin::Round,
            usvg::LineJoin::Bevel => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
        o.dash = s.dasharray().map_or(vec![], |d| d.iter().map(|v| *v as f64 * scale).collect());
    }
    Some(layer)
}

fn color(c: usvg::Color, alpha: f32) -> Color {
    Color::from_rgba8(c.red, c.green, c.blue, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A paint as a fill type, a color (for solids, or a fallback) and a gradient normalized to
/// `bounds` (page space), given the path's transform `t`.
fn paint(p: &usvg::Paint, opacity: f32, t: Affine, bounds: Rect) -> (FillType, Color, Gradient) {
    let norm = |q: Point| {
        Point::new((q.x - bounds.x0) / bounds.width().max(1e-9), (q.y - bounds.y0) / bounds.height().max(1e-9))
    };
    let stops = |stops: &[usvg::Stop]| -> Vec<GradientStop> {
        stops
            .iter()
            .map(|s| GradientStop { position: s.offset().get() as f64, color: color(s.color(), s.opacity().get() * opacity) })
            .collect()
    };
    let (kind, from, to, list) = match p {
        usvg::Paint::Color(c) => return (FillType::Color, color(*c, opacity), Gradient::default()),
        usvg::Paint::LinearGradient(g) => {
            let gt = t * affine(g.transform());
            let (a, b) = (gt * Point::new(g.x1() as f64, g.y1() as f64), gt * Point::new(g.x2() as f64, g.y2() as f64));
            (GradientKind::Linear, a, b, stops(g.stops()))
        }
        usvg::Paint::RadialGradient(g) => {
            let gt = t * affine(g.transform());
            let c = Point::new(g.cx() as f64, g.cy() as f64);
            let edge = c + scrawl_core::kurbo::Vec2::new(g.r().get() as f64, 0.0);
            (GradientKind::Radial, gt * c, gt * edge, stops(g.stops()))
        }
        // Patterns have no equivalent; use a mid grey so the shape stays visible.
        usvg::Paint::Pattern(_) => return (FillType::Color, Color::rgba(0.5, 0.5, 0.5, opacity), Gradient::default()),
    };
    let fallback = list.first().map_or(Color::BLACK, |s| s.color);
    (FillType::Gradient, fallback, Gradient { kind, from: norm(from), to: norm(to), stops: list, interpolation: Default::default() })
}

fn blend(b: usvg::BlendMode) -> BlendMode {
    match b {
        usvg::BlendMode::Normal => BlendMode::Normal,
        usvg::BlendMode::Multiply => BlendMode::Multiply,
        usvg::BlendMode::Screen => BlendMode::Screen,
        usvg::BlendMode::Overlay => BlendMode::Overlay,
        usvg::BlendMode::Darken => BlendMode::Darken,
        usvg::BlendMode::Lighten => BlendMode::Lighten,
        usvg::BlendMode::ColorDodge => BlendMode::ColorDodge,
        usvg::BlendMode::ColorBurn => BlendMode::ColorBurn,
        usvg::BlendMode::HardLight => BlendMode::HardLight,
        usvg::BlendMode::SoftLight => BlendMode::SoftLight,
        usvg::BlendMode::Difference => BlendMode::Difference,
        usvg::BlendMode::Exclusion => BlendMode::Exclusion,
        usvg::BlendMode::Hue => BlendMode::Hue,
        usvg::BlendMode::Saturation => BlendMode::Saturation,
        usvg::BlendMode::Color => BlendMode::Color,
        usvg::BlendMode::Luminosity => BlendMode::Luminosity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrawl_core::LayerKind;

    #[test]
    fn shapes_fills_strokes_and_gradients() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
                <stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f" stop-opacity="0.5"/>
            </linearGradient></defs>
            <rect id="Box" x="10" y="20" width="30" height="40" fill="#00ff00" stroke="black" stroke-width="2"/>
            <g opacity="0.5" transform="translate(50 0)">
                <circle cx="20" cy="20" r="10" fill="url(#g)"/>
                <path d="M0 50 L20 70" stroke="#123456" stroke-dasharray="4 2" fill="none"/>
            </g>
        </svg>"##;
        let root = import_svg(svg, "Drawing", &mut Assets::default()).unwrap();
        assert_eq!(root.name, "Drawing");
        let kids = root.children().unwrap();
        assert_eq!(kids.len(), 2);

        let rect = &kids[0];
        assert_eq!(rect.name, "Box");
        assert_eq!(rect.bounds_in(Affine::translate(root.frame.origin().to_vec2())), Some(Rect::new(10.0, 20.0, 40.0, 60.0)));
        assert_eq!(rect.style.fills[0].color.to_hex(), "00FF00");
        assert_eq!(rect.style.borders[0].thickness, 2.0);

        let g = &kids[1];
        assert_eq!(g.opacity, 0.5);
        let circle = &g.children().unwrap()[0];
        let grad = &circle.style.fills[0].gradient;
        assert_eq!(circle.style.fills[0].fill_type, FillType::Gradient);
        assert!((grad.from.x - 0.0).abs() < 1e-6 && (grad.to.x - 1.0).abs() < 1e-6);
        assert!((grad.stops[1].color.a - 0.5).abs() < 0.01);
        let line = &g.children().unwrap()[1];
        assert!(matches!(line.kind, LayerKind::Path(_)) && line.style.fills.is_empty());
        assert_eq!(line.style.border_options.dash, vec![4.0, 2.0]);
    }

    #[test]
    fn exported_svg_imports_back() {
        let mut page = scrawl_core::Page::new("P");
        let mut r = Layer::rectangle(scrawl_core::Frame::new(10.0, 10.0, 50.0, 30.0));
        r.style.fills = vec![Fill::solid(Color::rgba(1.0, 0.5, 0.0, 1.0))];
        scrawl_core::ops::insert_drawn(&mut page, r);
        let svg = crate::svg::page_svg(&page, Rect::new(0.0, 0.0, 100.0, 100.0), 1.0, &mut crate::TextEngine::new(), &Assets::default());
        let back = import_svg(svg.as_bytes(), "", &mut Assets::default()).unwrap();
        let b = back.bounds_in(Affine::IDENTITY).unwrap();
        assert!((b.x0 - 10.0).abs() < 1.0 && (b.width() - 50.0).abs() < 2.0, "{b:?}");
    }
}
