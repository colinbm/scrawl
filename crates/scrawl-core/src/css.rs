//! CSS for a layer's appearance, for "Copy as CSS".

use std::fmt::Write;

use crate::{BlendMode, BorderPosition, Color, FillType, Gradient, GradientKind, Layer, LayerKind, TextAlign, TextTransform};

fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `#rrggbb`, or `rgba()` when translucent.
pub fn color(c: Color) -> String {
    let [r, g, b, a] = c.to_rgba8();
    if a == 255 { format!("#{r:02X}{g:02X}{b:02X}") } else { format!("rgba({r}, {g}, {b}, {})", num(c.a as f64)) }
}

/// A CSS gradient for a gradient normalized to a `w`×`h` box.
fn gradient(g: &Gradient, w: f64, h: f64) -> String {
    let stops: Vec<String> = g.stops.iter().map(|s| format!("{} {}%", color(s.color), num(s.position * 100.0))).collect();
    let stops = stops.join(", ");
    match g.kind {
        GradientKind::Linear => {
            let (dx, dy) = ((g.to.x - g.from.x) * w, (g.to.y - g.from.y) * h);
            // CSS angles start at "to top" and run clockwise.
            let deg = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
            format!("linear-gradient({}deg, {stops})", num(deg))
        }
        GradientKind::Radial => format!("radial-gradient(circle at {}% {}%, {stops})", num(g.from.x * 100.0), num(g.from.y * 100.0)),
        GradientKind::Angular => format!("conic-gradient(from 0deg at 50% 50%, {stops})"),
    }
}

fn blend(b: BlendMode) -> Option<&'static str> {
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

/// CSS declarations describing `layer`, preceded by a comment with its name.
pub fn layer_css(layer: &Layer) -> String {
    let mut out = format!("/* {} */\n", layer.name.replace("*/", "* /"));
    let mut decl = |k: &str, v: String| writeln!(out, "{k}: {v};").expect("writing to a String");
    let (w, h) = (layer.frame.width, layer.frame.height);
    let style = &layer.style;
    decl("width", format!("{}px", num(w)));
    decl("height", format!("{}px", num(h)));
    if layer.rotation != 0.0 {
        decl("transform", format!("rotate({}deg)", num(layer.rotation)));
    }
    if layer.opacity < 1.0 {
        decl("opacity", num(layer.opacity));
    }
    if let Some(b) = blend(layer.blend_mode) {
        decl("mix-blend-mode", b.into());
    }
    let fills: Vec<_> = style.fills.iter().filter(|f| f.enabled).collect();

    if let LayerKind::Text(t) = &layer.kind {
        let ts = &t.style;
        let family = if ts.font_family.is_empty() { "system-ui".to_owned() } else { format!("\"{}\"", ts.font_family) };
        decl("font-family", family);
        decl("font-size", format!("{}px", num(ts.font_size)));
        decl("font-weight", ts.font_weight.to_string());
        if ts.italic {
            decl("font-style", "italic".into());
        }
        if let Some(lh) = ts.line_height {
            decl("line-height", format!("{}px", num(lh)));
        }
        if ts.letter_spacing != 0.0 {
            decl("letter-spacing", format!("{}px", num(ts.letter_spacing)));
        }
        let align = match ts.alignment {
            TextAlign::Left => None,
            TextAlign::Center => Some("center"),
            TextAlign::Right => Some("right"),
            TextAlign::Justify => Some("justify"),
        };
        if let Some(a) = align {
            decl("text-align", a.into());
        }
        let deco: Vec<&str> = [(ts.underline, "underline"), (ts.strikethrough, "line-through")].iter().filter(|(on, _)| *on).map(|(_, d)| *d).collect();
        if !deco.is_empty() {
            decl("text-decoration", deco.join(" "));
        }
        match ts.transform {
            TextTransform::Uppercase => decl("text-transform", "uppercase".into()),
            TextTransform::Lowercase => decl("text-transform", "lowercase".into()),
            TextTransform::None => {}
        }
        match fills.last().map(|f| (f.fill_type, f)) {
            Some((FillType::Color, f)) => decl("color", color(f.color)),
            Some((FillType::Gradient, f)) => {
                decl("background", gradient(&f.gradient, w, h));
                decl("-webkit-background-clip", "text".into());
                decl("color", "transparent".into());
            }
            _ => {}
        }
        let shadows: Vec<String> = style
            .shadows
            .iter()
            .filter(|s| s.enabled)
            .map(|s| format!("{}px {}px {}px {}", num(s.offset_x), num(s.offset_y), num(s.blur), color(s.color)))
            .collect();
        if !shadows.is_empty() {
            decl("text-shadow", shadows.join(", "));
        }
        return out;
    }

    // Backgrounds stack top first in CSS; only the bottom layer may be a plain color.
    let backgrounds: Vec<String> = fills
        .iter()
        .rev()
        .filter_map(|f| match f.fill_type {
            FillType::Color => Some(color(f.color)),
            FillType::Gradient => Some(gradient(&f.gradient, w, h)),
            FillType::Image => None,
        })
        .enumerate()
        .map(|(i, b)| if i + 1 < fills.len() && b.starts_with(['#', 'r']) && !b.starts_with("radial") { format!("linear-gradient({b}, {b})") } else { b })
        .collect();
    match backgrounds.as_slice() {
        [] => {}
        [one] if !one.contains("gradient") => decl("background-color", one.clone()),
        many => decl("background", many.join(", ")),
    }
    let mut shadows = vec![];
    if let Some(b) = style.borders.iter().find(|b| b.enabled && b.thickness > 0.0) {
        match b.position {
            // An outside border doesn't take up room, like an outset shadow.
            BorderPosition::Outside => shadows.push(format!("0 0 0 {}px {}", num(b.thickness), color(b.color))),
            _ => {
                decl("border", format!("{}px solid {}", num(b.thickness), color(b.color)));
                if b.position == BorderPosition::Inside {
                    decl("box-sizing", "border-box".into());
                }
            }
        }
    }
    match &layer.kind {
        LayerKind::Rectangle(r) => match r.corner_radii {
            Some([a, b, c, d]) => decl("border-radius", format!("{}px {}px {}px {}px", num(a), num(b), num(c), num(d))),
            None if r.corner_radius > 0.0 => decl("border-radius", format!("{}px", num(r.corner_radius))),
            None => {}
        },
        LayerKind::Oval => decl("border-radius", "50%".into()),
        _ => {}
    }
    for (list, inset) in [(&style.shadows, ""), (&style.inner_shadows, "inset ")] {
        for s in list.iter().filter(|s| s.enabled) {
            shadows.push(format!("{inset}{}px {}px {}px {}px {}", num(s.offset_x), num(s.offset_y), num(s.blur), num(s.spread), color(s.color)));
        }
    }
    if !shadows.is_empty() {
        decl("box-shadow", shadows.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fill, Frame, Rectangle, Shadow};
    use kurbo::Point;

    #[test]
    fn shapes_and_text() {
        let mut r = Layer::rectangle(Frame::new(0.0, 0.0, 100.0, 40.0));
        r.name = "Button".into();
        r.kind = LayerKind::Rectangle(Rectangle { corner_radius: 8.0, corner_radii: None });
        r.style.fills = vec![Fill::solid(Color::from_rgba8(0x1F, 0x8F, 0xFF, 255))];
        r.style.shadows = vec![Shadow::drop()];
        let css = layer_css(&r);
        assert!(css.starts_with("/* Button */\nwidth: 100px;\nheight: 40px;\n"), "{css}");
        assert!(css.contains("background-color: #1F8FFF;"));
        assert!(css.contains("border: 1px solid #979797;"));
        assert!(css.contains("border-radius: 8px;"));
        assert!(css.contains("box-shadow: 0px 2px 4px 0px rgba(0, 0, 0, 0.5);"));

        let mut g = Fill::solid(Color::BLACK);
        g.fill_type = FillType::Gradient;
        assert!(gradient(&g.gradient, 10.0, 10.0).starts_with("linear-gradient(180deg"));

        let mut t = Layer::text(Point::ORIGIN, "Hi");
        if let LayerKind::Text(text) = &mut t.kind {
            text.style.font_family = "Inter".into();
            text.style.underline = true;
        }
        let css = layer_css(&t);
        assert!(css.contains("font-family: \"Inter\";") && css.contains("text-decoration: underline;") && css.contains("color: #000000;"));
    }
}
