//! SVG output of page content, for "Copy as SVG" and SVG and PDF export.
//!
//! Layers become nested groups with their transforms, so the structure survives. Text is written
//! as glyph outlines, which look the same everywhere without the fonts. SVG has no conic
//! gradients, so angular gradient fills are drawn as thin wedges of color; angular borders fall
//! back to a linear gradient.

use std::fmt::Write;

use scrawl_core::assets::Assets;
use scrawl_core::kurbo::{Affine, BezPath, Point, Rect, Shape};
use scrawl_core::style::stroke_style;
use scrawl_core::{
    BlendMode, Border, BorderPosition, Color, FillRule, FillType, Gradient, GradientKind, ImageMode, Layer, LayerKind, Page, Shadow,
};

use crate::TextEngine;

/// An SVG document of `area` (page space) of `page`, `scale` times its size in pixels.
pub fn page_svg(page: &Page, area: Rect, scale: f64, text: &mut TextEngine, assets: &Assets) -> String {
    let mut w = Writer { defs: String::new(), body: String::new(), next: 0, text, assets };
    w.children(&page.layers);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{}" height="{}" viewBox="{} {} {} {}"><defs>{}</defs>{}</svg>"#,
        num(area.width() * scale),
        num(area.height() * scale),
        num(area.x0),
        num(area.y0),
        num(area.width()),
        num(area.height()),
        w.defs,
        w.body
    )
}

/// A number with at most three decimals and no trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{:.3}", if v.abs() < 5e-4 { 0.0 } else { v });
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn matrix(t: Affine) -> String {
    let [a, b, c, d, e, f] = t.as_coeffs();
    format!("matrix({} {} {} {} {} {})", num(a), num(b), num(c), num(d), num(e), num(f))
}

fn hex(c: Color) -> String {
    let [r, g, b, _] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `name="#rrggbb" name-opacity="a"`, leaving out opacity 1.
fn paint_attrs(name: &str, c: Color) -> String {
    let mut s = format!(r#"{name}="{}""#, hex(c));
    if c.a < 1.0 {
        write!(s, r#" {name}-opacity="{}""#, num(c.a as f64)).unwrap();
    }
    s
}

fn blend_css(b: BlendMode) -> Option<&'static str> {
    Some(match b {
        BlendMode::Normal => return None,
        BlendMode::Darken => "darken",
        BlendMode::Multiply => "multiply",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::Lighten => "lighten",
        BlendMode::Screen => "screen",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::Overlay => "overlay",
        BlendMode::SoftLight => "soft-light",
        BlendMode::HardLight => "hard-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
    })
}

/// Standard base64, for embedding images as data URLs.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A data URL for image asset `id`, with its pixel size.
fn image_data(assets: &Assets, id: &str) -> Option<(String, f64, f64)> {
    let bytes = assets.get(id)?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes[..])).with_guessed_format().ok()?.into_dimensions().ok()?;
    let mime = match id.rsplit('.').next() {
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Some((format!("data:{mime};base64,{}", base64(bytes)), w as f64, h as f64))
}

struct Writer<'a> {
    defs: String,
    body: String,
    next: usize,
    text: &'a mut TextEngine,
    assets: &'a Assets,
}

impl Writer<'_> {
    fn id(&mut self, kind: &str) -> String {
        self.next += 1;
        format!("{kind}{}", self.next)
    }

    /// Siblings bottom to top. A mask clips the siblings above it.
    fn children(&mut self, layers: &[Layer]) {
        let mut clips = 0;
        let mut i = 0;
        while i < layers.len() {
            let l = &layers[i];
            if l.breaks_mask {
                for _ in 0..std::mem::take(&mut clips) {
                    self.body.push_str("</g>");
                }
            }
            self.layer(l);
            i += 1;
            if l.is_mask && l.visible && l.mask_mode != scrawl_core::MaskMode::Outline {
                // Alpha and luminance masks: an SVG <mask> of the mask layer itself.
                let end = layers[i..].iter().position(|c| c.breaks_mask).map_or(layers.len(), |p| i + p);
                let id = self.id("mask");
                let body = std::mem::take(&mut self.body);
                let mut alone = l.clone();
                alone.is_mask = false;
                self.layer(&alone);
                let mask_body = std::mem::replace(&mut self.body, body);
                let kind = if l.mask_mode == scrawl_core::MaskMode::Luminance { "luminance" } else { "alpha" };
                write!(self.defs, r#"<mask id="{id}" mask-type="{kind}" style="mask-type:{kind}" maskUnits="userSpaceOnUse" x="-100000" y="-100000" width="200000" height="200000">{mask_body}</mask>"#).unwrap();
                write!(self.body, r#"<g mask="url(#{id})">"#).unwrap();
                self.children(&layers[i..end]);
                self.body.push_str("</g>");
                i = end;
                continue;
            }
            if l.is_mask
                && l.visible
                && let Some(path) = l.local_path()
            {
                let id = self.id("mask");
                write!(
                    self.defs,
                    r#"<clipPath id="{id}"><path d="{}" transform="{}"{}/></clipPath>"#,
                    path.to_svg(),
                    matrix(l.local_transform()),
                    rule_attr(l, "clip-rule")
                )
                .unwrap();
                write!(self.body, r#"<g clip-path="url(#{id})">"#).unwrap();
                clips += 1;
            }
        }
        for _ in 0..clips {
            self.body.push_str("</g>");
        }
    }

    fn layer(&mut self, l: &Layer) {
        if !l.visible || l.opacity <= 0.0 || matches!(l.kind, LayerKind::Slice | LayerKind::Hotspot) {
            return;
        }
        write!(self.body, r#"<g transform="{}""#, matrix(l.local_transform())).unwrap();
        if l.opacity < 1.0 {
            write!(self.body, r#" opacity="{}""#, num(l.opacity)).unwrap();
        }
        if let Some(b) = blend_css(l.blend_mode) {
            write!(self.body, r#" style="mix-blend-mode:{b}""#).unwrap();
        }
        self.body.push('>');
        let rect = l.frame.local_rect();
        match &l.kind {
            LayerKind::Artboard(ab) => {
                write!(self.body, r#"<rect width="{}" height="{}" {}/>"#, num(rect.width()), num(rect.height()), paint_attrs("fill", ab.background.unwrap_or(Color::WHITE))).unwrap();
                if ab.clip_content {
                    let id = self.id("clip");
                    write!(self.defs, r#"<clipPath id="{id}"><rect width="{}" height="{}"/></clipPath>"#, num(rect.width()), num(rect.height())).unwrap();
                    write!(self.body, r#"<g clip-path="url(#{id})">"#).unwrap();
                    self.children(&ab.children);
                    self.body.push_str("</g>");
                } else {
                    self.children(&ab.children);
                }
            }
            LayerKind::Group(g) if g.shape => {
                if let Some(path) = l.combined_path() {
                    self.shape(l, &path, false);
                }
            }
            LayerKind::Group(g) => self.children(&g.children),
            LayerKind::Symbol(s) => self.children(&s.expanded),
            LayerKind::Text(_) => {
                // Text in the layer's style, then style runs with colors of their own.
                let groups = self.text.outlines_by_color(l).unwrap_or_default();
                for (color, path) in &groups {
                    match color {
                        None => self.shape(l, path, true),
                        Some(c) => write!(self.body, r#"<path d="{}" {}/>"#, path.to_svg(), paint_attrs("fill", *c)).unwrap(),
                    }
                }
            }
            LayerKind::Image(img) => {
                let outline = rect.to_path(0.1);
                self.shadows(l, &outline);
                if let Some((url, _, _)) = image_data(self.assets, &img.image) {
                    write!(self.body, r#"<image width="{}" height="{}" preserveAspectRatio="none" xlink:href="{url}"/>"#, num(rect.width()), num(rect.height())).unwrap();
                }
                let mut over = l.clone();
                over.style.shadows.clear();
                self.shape(&over, &outline, false);
            }
            LayerKind::Slice | LayerKind::Hotspot => {}
            _ => {
                if let Some(path) = l.local_path() {
                    self.shape(l, &path, false);
                }
            }
        }
        self.body.push_str("</g>");
    }

    fn shadows(&mut self, l: &Layer, path: &BezPath) {
        for s in l.style.shadows.iter().filter(|s| s.enabled) {
            let id = self.id("shadow");
            // Spread grows (or shrinks) the shape before the blur.
            let morph = match s.spread {
                v if v > 0.0 => format!(r#"<feMorphology operator="dilate" radius="{}"/>"#, num(v)),
                v if v < 0.0 => format!(r#"<feMorphology operator="erode" radius="{}"/>"#, num(-v)),
                _ => String::new(),
            };
            write!(
                self.defs,
                r#"<filter id="{id}" x="-100%" y="-100%" width="300%" height="300%">{morph}<feGaussianBlur stdDeviation="{}"/></filter>"#,
                num(s.blur / 2.0)
            )
            .unwrap();
            write!(
                self.body,
                r#"<path d="{}" transform="translate({} {})" {}{} filter="url(#{id})"/>"#,
                path.to_svg(),
                num(s.offset_x),
                num(s.offset_y),
                paint_attrs("fill", s.color),
                rule_attr(l, "fill-rule")
            )
            .unwrap();
        }
    }

    /// Shadows, fills, inner shadows and borders of a shape (or text outline) in local space.
    fn shape(&mut self, l: &Layer, path: &BezPath, text: bool) {
        let (w, h) = (l.frame.width, l.frame.height);
        let d = path.to_svg();
        let rule = rule_attr(l, "fill-rule");
        self.shadows(l, path);
        let closed = text || l.is_closed();
        if closed {
            for fill in l.style.fills.iter().filter(|f| f.enabled) {
                let blend = blend_css(fill.blend_mode).map_or(String::new(), |b| format!(r#" style="mix-blend-mode:{b}""#));
                match (fill.fill_type, &fill.image) {
                    (FillType::Image, Some(id)) => self.image_fill(&d, &rule, id, fill.image_mode, fill.image_scale, w, h, &blend),
                    (FillType::Image, None) => {}
                    (FillType::Gradient, _) if fill.gradient.kind == GradientKind::Angular => self.angular_fill(&d, &rule, &fill.gradient, w, h, &blend),
                    (FillType::Gradient, _) => {
                        let id = self.gradient(&fill.gradient, w, h);
                        write!(self.body, r#"<path d="{d}" fill="url(#{id})"{rule}{blend}/>"#).unwrap();
                    }
                    (FillType::Color, _) => {
                        write!(self.body, r#"<path d="{d}" {}{rule}{blend}/>"#, paint_attrs("fill", fill.color)).unwrap();
                    }
                    (FillType::Noise, _) => self.noise_fill(&d, &rule, fill.noise, fill.color, &blend),
                }
            }
            for s in l.style.inner_shadows.iter().filter(|s| s.enabled) {
                self.inner_shadow(&d, &rule, s);
            }
        }
        for b in l.style.borders.iter().filter(|b| b.enabled && b.thickness > 0.0) {
            self.border(l, &d, &rule, b, closed && !text, w, h);
            // Arrowheads and other end markers on open paths.
            let opts = &l.style.border_options;
            if !closed && !text {
                let paint = match b.fill_type {
                    FillType::Gradient => format!("url(#{})", self.gradient(&b.gradient, w, h)),
                    _ => hex(b.color),
                };
                let opacity = if b.color.a < 1.0 && b.fill_type != FillType::Gradient { format!(r#" opacity="{}""#, num(b.color.a as f64)) } else { String::new() };
                for (shape, how) in crate::marker_shapes(path, b.thickness, opts.start_marker, opts.end_marker) {
                    let md = shape.to_svg();
                    let stroke = format!(r#"stroke="{paint}" stroke-width="{}" stroke-linejoin="miter""#, num(b.thickness));
                    match how {
                        crate::MarkerPaint::Fill => write!(self.body, r#"<path d="{md}" fill="{paint}"{opacity}/>"#),
                        crate::MarkerPaint::Stroke => write!(self.body, r#"<path d="{md}" fill="none" {stroke}{opacity}/>"#),
                        crate::MarkerPaint::Hollow => write!(self.body, r#"<path d="{md}" fill="white" {stroke}{opacity}/>"#),
                    }
                    .unwrap();
                }
            }
        }
    }

    /// An angular gradient as thin wedges around the frame's center, each in the gradient's
    /// color at its angle, clipped to the shape.
    #[allow(clippy::too_many_arguments)]
    fn angular_fill(&mut self, d: &str, rule: &str, g: &Gradient, w: f64, h: f64, blend: &str) {
        const WEDGES: usize = 90;
        let clip = self.id("clip");
        write!(self.defs, r#"<clipPath id="{clip}"><path d="{d}"{}/></clipPath>"#, rule.replace("fill-rule", "clip-rule")).unwrap();
        let (cx, cy) = (w / 2.0, h / 2.0);
        let r = w.hypot(h);
        write!(self.body, r#"<g clip-path="url(#{clip})"{blend}>"#).unwrap();
        for i in 0..WEDGES {
            // Starting at the top, clockwise, as the canvas draws it; a hair of overlap hides seams.
            let a0 = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * i as f64 / WEDGES as f64;
            let a1 = a0 + std::f64::consts::TAU / WEDGES as f64 * 1.02;
            let c = g.color_at((i as f64 + 0.5) / WEDGES as f64);
            write!(
                self.body,
                r#"<path d="M{} {}L{} {}L{} {}Z" {}/>"#,
                num(cx),
                num(cy),
                num(cx + r * a0.cos()),
                num(cy + r * a0.sin()),
                num(cx + r * a1.cos()),
                num(cy + r * a1.sin()),
                paint_attrs("fill", c)
            )
            .unwrap();
        }
        self.body.push_str("</g>");
    }

    /// The noise tile as a repeating PNG pattern, at the scale the canvas draws it.
    fn noise_fill(&mut self, d: &str, rule: &str, noise: scrawl_core::Noise, color: scrawl_core::Color, blend: &str) {
        let img = crate::images::noise_image(noise, color);
        let Some(rgba) = image::RgbaImage::from_raw(img.width, img.height, img.data.data().to_vec()) else { return };
        let mut png = std::io::Cursor::new(vec![]);
        if rgba.write_to(&mut png, image::ImageFormat::Png).is_err() {
            return;
        }
        let (n, pid) = (img.width, self.id("noise"));
        write!(
            self.defs,
            r#"<pattern id="{pid}" patternUnits="userSpaceOnUse" width="{n}" height="{n}" patternTransform="scale(0.5)"><image width="{n}" height="{n}" xlink:href="data:image/png;base64,{}"/></pattern>"#,
            base64(png.get_ref())
        )
        .unwrap();
        write!(self.body, r#"<path d="{d}" fill="url(#{pid})"{rule}{blend}/>"#).unwrap();
    }

    fn gradient(&mut self, g: &Gradient, w: f64, h: f64) -> String {
        let id = self.id("gradient");
        // SVG blends in sRGB; other spaces are sampled into extra stops.
        let stops: String = g
            .srgb_stops()
            .iter()
            .map(|s| {
                let opacity = if s.color.a < 1.0 { format!(r#" stop-opacity="{}""#, num(s.color.a as f64)) } else { String::new() };
                format!(r#"<stop offset="{}" stop-color="{}"{opacity}/>"#, num(s.position), hex(s.color))
            })
            .collect();
        // Gradient geometry is normalized to the frame.
        let units = format!(r#"gradientUnits="userSpaceOnUse" gradientTransform="{}""#, matrix(Affine::scale_non_uniform(w.max(1e-3), h.max(1e-3))));
        match g.kind {
            GradientKind::Radial => write!(
                self.defs,
                r#"<radialGradient id="{id}" {units} cx="{}" cy="{}" r="{}">{stops}</radialGradient>"#,
                num(g.from.x),
                num(g.from.y),
                num((g.to - g.from).hypot())
            ),
            // SVG has no conic gradients; a linear one along the same axis is the closest.
            GradientKind::Linear | GradientKind::Angular => write!(
                self.defs,
                r#"<linearGradient id="{id}" {units} x1="{}" y1="{}" x2="{}" y2="{}">{stops}</linearGradient>"#,
                num(g.from.x),
                num(g.from.y),
                num(g.to.x),
                num(g.to.y)
            ),
        }
        .unwrap();
        id
    }

    #[allow(clippy::too_many_arguments)]
    fn image_fill(&mut self, d: &str, rule: &str, id: &str, mode: ImageMode, scale: f64, w: f64, h: f64, blend: &str) {
        let Some((url, iw, ih)) = image_data(self.assets, id) else { return };
        if mode == ImageMode::Tile {
            let pid = self.id("pattern");
            write!(
                self.defs,
                r#"<pattern id="{pid}" patternUnits="userSpaceOnUse" width="{iw}" height="{ih}" patternTransform="scale({})"><image width="{iw}" height="{ih}" xlink:href="{url}"/></pattern>"#,
                num(scale)
            )
            .unwrap();
            write!(self.body, r#"<path d="{d}" fill="url(#{pid})"{rule}{blend}/>"#).unwrap();
            return;
        }
        let place = |s: f64| Rect::from_origin_size(Point::new((w - iw * s) / 2.0, (h - ih * s) / 2.0), (iw * s, ih * s));
        let r = match mode {
            ImageMode::Fill => place((w / iw).max(h / ih)),
            ImageMode::Fit => place((w / iw).min(h / ih)),
            _ => Rect::new(0.0, 0.0, w, h),
        };
        let clip = self.id("clip");
        write!(self.defs, r#"<clipPath id="{clip}"><path d="{d}"{}/></clipPath>"#, rule.replace("fill-rule", "clip-rule")).unwrap();
        write!(
            self.body,
            r#"<image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none" clip-path="url(#{clip})" xlink:href="{url}"{blend}/>"#,
            num(r.x0),
            num(r.y0),
            num(r.width()),
            num(r.height())
        )
        .unwrap();
    }

    /// The shadow inside a shape: its alpha minus a shifted, blurred copy, in the shadow color.
    fn inner_shadow(&mut self, d: &str, rule: &str, s: &Shadow) {
        let id = self.id("inner");
        write!(
            self.defs,
            r#"<filter id="{id}" x="-50%" y="-50%" width="200%" height="200%"><feOffset in="SourceAlpha" dx="{}" dy="{}"/><feGaussianBlur stdDeviation="{}" result="b"/><feComposite in="SourceAlpha" in2="b" operator="out" result="rim"/><feFlood {}/><feComposite in2="rim" operator="in"/></filter>"#,
            num(s.offset_x),
            num(s.offset_y),
            num(s.blur / 2.0),
            paint_attrs("flood-color", s.color).replace("flood-color-opacity", "flood-opacity")
        )
        .unwrap();
        write!(self.body, r#"<path d="{d}" fill="black"{rule} filter="url(#{id})"/>"#).unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn border(&mut self, l: &Layer, d: &str, rule: &str, b: &Border, closed: bool, w: f64, h: f64) {
        let paint = match b.fill_type {
            FillType::Gradient => format!(r#"stroke="url(#{})""#, self.gradient(&b.gradient, w, h)),
            _ => paint_attrs("stroke", b.color),
        };
        let position = if closed { b.position } else { BorderPosition::Center };
        let width = if position == BorderPosition::Center { b.thickness } else { b.thickness * 2.0 };
        let style = stroke_style(width, &l.style.border_options);
        let mut attrs = format!(r#"fill="none" {paint} stroke-width="{}""#, num(width));
        let cap = match style.start_cap {
            scrawl_core::kurbo::Cap::Butt => "butt",
            scrawl_core::kurbo::Cap::Round => "round",
            scrawl_core::kurbo::Cap::Square => "square",
        };
        let join = match style.join {
            scrawl_core::kurbo::Join::Miter => "miter",
            scrawl_core::kurbo::Join::Round => "round",
            scrawl_core::kurbo::Join::Bevel => "bevel",
        };
        write!(attrs, r#" stroke-linecap="{cap}" stroke-linejoin="{join}""#).unwrap();
        if !style.dash_pattern.is_empty() {
            let dash: Vec<String> = style.dash_pattern.iter().map(|v| num(*v)).collect();
            write!(attrs, r#" stroke-dasharray="{}""#, dash.join(" ")).unwrap();
        }
        match position {
            BorderPosition::Center => write!(self.body, r#"<path d="{d}" {attrs}/>"#).unwrap(),
            BorderPosition::Inside => {
                let clip = self.id("clip");
                write!(self.defs, r#"<clipPath id="{clip}"><path d="{d}"{}/></clipPath>"#, rule.replace("fill-rule", "clip-rule")).unwrap();
                write!(self.body, r#"<path d="{d}" {attrs} clip-path="url(#{clip})"/>"#).unwrap();
            }
            BorderPosition::Outside => {
                let mask = self.id("mask");
                let pad = b.thickness + 2.0;
                write!(
                    self.defs,
                    r#"<mask id="{mask}" maskUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><rect x="{}" y="{}" width="{}" height="{}" fill="white"/><path d="{d}" fill="black"{rule}/></mask>"#,
                    num(-pad),
                    num(-pad),
                    num(w + 2.0 * pad),
                    num(h + 2.0 * pad),
                    num(-pad),
                    num(-pad),
                    num(w + 2.0 * pad),
                    num(h + 2.0 * pad)
                )
                .unwrap();
                write!(self.body, r#"<path d="{d}" {attrs} mask="url(#{mask})"/>"#).unwrap();
            }
        }
    }
}

fn rule_attr(l: &Layer, name: &str) -> String {
    match l.fill_rule() {
        FillRule::EvenOdd => format!(r#" {name}="evenodd""#),
        FillRule::NonZero => String::new(),
    }
}

/// A PDF of an SVG document, keeping vectors as vectors.
pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, String> {
    let tree = svg2pdf::usvg::Tree::from_str(svg, &svg2pdf::usvg::Options::default()).map_err(|e| format!("SVG error: {e}"))?;
    svg2pdf::to_pdf(&tree, svg2pdf::ConversionOptions::default(), svg2pdf::PageOptions::default()).map_err(|e| format!("PDF error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrawl_core::{Fill, Frame, ops};

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn shapes_become_parseable_svg_and_pdf() {
        let mut page = Page::new("P");
        ops::insert_drawn(&mut page, Layer::artboard("A", Frame::new(0.0, 0.0, 100.0, 100.0)));
        let mut r = Layer::rectangle(Frame::new(10.0, 10.0, 30.0, 20.0));
        r.style.fills = vec![Fill::solid(Color::rgba(1.0, 0.0, 0.0, 0.5))];
        r.style.borders[0].position = BorderPosition::Outside;
        r.style.shadows = vec![Shadow::drop()];
        r.rotation = 30.0;
        ops::insert_drawn(&mut page, r);
        let svg = page_svg(&page, Rect::new(0.0, 0.0, 100.0, 100.0), 2.0, &mut TextEngine::new(), &Assets::default());
        assert!(svg.contains(r##"fill="#ff0000" fill-opacity="0.5""##), "{svg}");
        assert!(svg.contains(r#"width="200""#));
        let tree = svg2pdf::usvg::Tree::from_str(&svg, &Default::default()).expect("valid SVG");
        assert_eq!(tree.size().width(), 200.0);
        let pdf = svg_to_pdf(&svg).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }
}
