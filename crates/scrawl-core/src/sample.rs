//! A demo document exercising layer types and styles, used by "Open Sample Document", examples
//! and tests.

use kurbo::Point;

use crate::ops::{self, BooleanOp};
use crate::text::{TextAlign, TextSizing};
use crate::{
    BlendMode, Border, BorderPosition, Color, Document, Fill, FillType, Frame, Gradient, GradientKind, GradientStop, Layer,
    LayerKind, Marker, Rectangle, Shadow,
};

fn hex(s: &str) -> Color {
    Color::from_hex(s).expect("valid hex")
}

fn gradient(kind: GradientKind, from: (f64, f64), to: (f64, f64), stops: &[(&str, f64)]) -> Fill {
    let g = Gradient {
        kind,
        from: Point::new(from.0, from.1),
        to: Point::new(to.0, to.1),
        stops: stops.iter().map(|&(c, p)| GradientStop { position: p, color: hex(c) }).collect(),
    };
    Fill { fill_type: FillType::Gradient, gradient: g, ..Fill::solid(Color::BLACK) }
}

/// A demo document exercising layer types and styles.
pub fn sample_document() -> Document {
    let mut doc = Document {
        colors: ["1F8FFF", "7C5CFF", "FF8A3D", "2EC4B6", "FFD23F"].iter().map(|h| hex(h)).collect(),
        ..Default::default()
    };
    let page = &mut doc.pages[0];
    page.name = "Home".into();

    ops::insert_drawn(page, Layer::artboard("iPhone 16", Frame::new(0.0, 0.0, 393.0, 852.0)));

    let mut header = Layer::new("Header", Frame::new(0.0, 0.0, 393.0, 140.0), LayerKind::Rectangle(Rectangle::default()));
    header.style.fills.push(gradient(GradientKind::Linear, (0.0, 0.0), (1.0, 1.0), &[("1F8FFF", 0.0), ("7C5CFF", 1.0)]));
    ops::insert_drawn(page, header);

    let mut card = Layer::rectangle(Frame::new(24.0, 170.0, 345.0, 200.0));
    card.kind = LayerKind::Rectangle(Rectangle { corner_radius: 16.0, corner_radii: None });
    card.style.fills = vec![Fill::solid(Color::WHITE)];
    card.style.borders.clear();
    card.style.shadows = vec![Shadow { color: Color::BLACK.with_alpha(0.25), offset_y: 8.0, blur: 24.0, ..Shadow::drop() }];
    card.name = "Card".into();
    ops::insert_drawn(page, card);

    let mut avatar = Layer::oval(Frame::new(48.0, 194.0, 64.0, 64.0));
    avatar.style.fills = vec![gradient(GradientKind::Radial, (0.35, 0.3), (1.0, 1.0), &[("FFD23F", 0.0), ("FF8A3D", 1.0)])];
    avatar.style.borders = vec![Border { position: BorderPosition::Outside, ..Border::solid(Color::WHITE, 3.0) }];
    avatar.style.shadows = vec![Shadow::drop()];
    avatar.name = "Avatar".into();
    ops::insert_drawn(page, avatar);

    let mut star = Layer::star(Frame::new(270.0, 194.0, 72.0, 72.0));
    star.style.fills = vec![gradient(GradientKind::Angular, (0.5, 0.5), (1.0, 0.5), &[("FF3D7F", 0.0), ("FFD23F", 0.33), ("2EC4B6", 0.66), ("FF3D7F", 1.0)])];
    star.style.borders.clear();
    star.style.shadows = vec![Shadow { blur: 10.0, offset_y: 4.0, ..Shadow::drop() }];
    ops::insert_drawn(page, star);

    // A dashed arrow.
    let mut arrow = Layer::line(Point::new(48.0, 320.0), Point::new(340.0, 320.0));
    arrow.style.borders[0] = Border::solid(hex("7C5CFF"), 2.0);
    arrow.style.border_options.dash = vec![6.0, 4.0];
    arrow.style.border_options.end_marker = Marker::FilledArrow;
    arrow.style.border_options.start_marker = Marker::OpenCircle;
    arrow.name = "Arrow".into();
    ops::insert_drawn(page, arrow);

    // Tiles with inner shadows and blend modes over a stripe.
    let mut stripe = Layer::rectangle(Frame::new(0.0, 420.0, 393.0, 60.0));
    stripe.style.fills = vec![Fill::solid(hex("FFD23F"))];
    stripe.style.borders.clear();
    stripe.name = "Stripe".into();
    ops::insert_drawn(page, stripe);
    for (i, mode) in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Difference].into_iter().enumerate() {
        let mut tile = Layer::rectangle(Frame::new(24.0 + i as f64 * 119.0, 400.0, 107.0, 107.0));
        tile.kind = LayerKind::Rectangle(Rectangle { corner_radius: 20.0, corner_radii: None });
        tile.style.fills = vec![Fill::solid(hex("2EC4B6"))];
        tile.style.borders.clear();
        tile.style.inner_shadows = vec![Shadow { color: Color::BLACK.with_alpha(0.35), offset_y: 4.0, blur: 10.0, ..Shadow::inner() }];
        tile.blend_mode = mode;
        tile.name = format!("Tile ({})", mode.label());
        ops::insert_drawn(page, tile);
    }

    // A mask: a circle clipping a striped rectangle.
    let mask = ops::insert_drawn(page, Layer::oval(Frame::new(24.0, 560.0, 160.0, 160.0)));
    let mut stripes = vec![mask];
    for i in 0..6 {
        let mut s = Layer::rectangle(Frame::new(0.0, 560.0 + i as f64 * 28.0, 220.0, 14.0));
        s.style.fills = vec![Fill::solid(hex(if i % 2 == 0 { "FF8A3D" } else { "7C5CFF" }))];
        s.style.borders.clear();
        stripes.push(ops::insert_drawn(page, s));
    }
    ops::use_as_mask(page, &stripes);

    // Boolean: a ring with a drop shadow.
    let outer = ops::insert_drawn(page, Layer::oval(Frame::new(220.0, 560.0, 140.0, 140.0)));
    let inner = ops::insert_drawn(page, Layer::oval(Frame::new(260.0, 600.0, 60.0, 60.0)));
    let ring = ops::boolean(page, &[outer, inner], BooleanOp::Subtract).expect("subtract");
    let ring = page.find_mut(ring).unwrap();
    ring.name = "Ring".into();
    ring.style.fills = vec![gradient(GradientKind::Linear, (0.5, 0.0), (0.5, 1.0), &[("2EC4B6", 0.0), ("1F8FFF", 1.0)])];
    ring.style.borders.clear();
    ring.style.shadows = vec![Shadow { blur: 12.0, offset_y: 6.0, color: Color::BLACK.with_alpha(0.35), ..Shadow::drop() }];

    // Text.
    let mut title = Layer::text(Point::new(24.0, 56.0), "Good morning");
    title.style.fills = vec![Fill::solid(Color::WHITE)];
    set_text(&mut title, |t| {
        t.style.font_size = 32.0;
        t.style.font_weight = 700;
    });
    title.style.shadows = vec![Shadow { color: Color::BLACK.with_alpha(0.3), offset_y: 2.0, blur: 6.0, ..Shadow::drop() }];
    ops::insert_drawn(page, title);

    let mut body = Layer::text(
        Point::new(128.0, 200.0),
        "Scrawl lays out text with parley and renders glyphs with Vello. This paragraph wraps at a fixed width.",
    );
    body.frame.width = 130.0;
    body.style.fills = vec![Fill::solid(hex("444444"))];
    set_text(&mut body, |t| {
        t.sizing = TextSizing::FixedWidth;
        t.style.font_size = 11.0;
        t.style.line_height = Some(15.0);
    });
    body.name = "Body".into();
    ops::insert_drawn(page, body);

    let mut fancy = Layer::text(Point::new(24.0, 730.0), "Gradient text");
    fancy.style.fills = vec![gradient(GradientKind::Linear, (0.0, 0.5), (1.0, 0.5), &[("FF3D7F", 0.0), ("7C5CFF", 1.0)])];
    set_text(&mut fancy, |t| {
        t.style.font_size = 34.0;
        t.style.font_weight = 800;
        t.style.italic = true;
    });
    ops::insert_drawn(page, fancy);

    let mut deco = Layer::text(Point::new(24.0, 800.0), "Underlined & spaced");
    deco.style.fills = vec![Fill::solid(hex("1F8FFF"))];
    deco.style.borders = vec![];
    set_text(&mut deco, |t| {
        t.style.font_size = 14.0;
        t.style.underline = true;
        t.style.letter_spacing = 0.5;
    });
    ops::insert_drawn(page, deco);

    let mut outlined = Layer::text(Point::new(250.0, 730.0), "Outlined");
    outlined.style.fills = vec![];
    outlined.style.borders = vec![Border::solid(hex("2EC4B6"), 1.0)];
    set_text(&mut outlined, |t| {
        t.style.font_size = 30.0;
        t.style.font_weight = 900;
        t.style.alignment = TextAlign::Center;
        t.style.strikethrough = true;
    });
    ops::insert_drawn(page, outlined);

    add_shared_components(&mut doc);
    doc
}

/// Color variables and shared styles, linked to some of the layers above.
fn add_shared_components(doc: &mut Document) {
    let named = |doc: &Document, name: &str| {
        let mut found = vec![];
        doc.pages[0].walk(|l| {
            if l.name == name || l.name.starts_with(&format!("{name} (")) {
                found.push(l.id);
            }
        });
        found
    };
    for (name, hex_color, layers) in [("Sunshine", "FFD23F", &["Stripe"][..]), ("Brand Blue", "1F8FFF", &["Underlined & spaced"][..])] {
        let id = doc.add_swatch(hex(hex_color));
        doc.swatches.last_mut().expect("just added").name = name.into();
        let ids: Vec<_> = layers.iter().flat_map(|n| named(doc, n)).collect();
        for lid in ids {
            let fill = &mut doc.pages[0].find_mut(lid).expect("named layer").style.fills[0];
            fill.color = hex(hex_color);
            fill.swatch = Some(id);
        }
    }

    // Every tile shares a layer style; the title defines a text style.
    let tiles = named(doc, "Tile");
    let tile = doc.pages[0].find(tiles[0]).expect("tile").clone();
    let style = doc.create_shared_style(&tile);
    let title = named(doc, "Good morning")[0];
    let title_layer = doc.pages[0].find(title).expect("title").clone();
    let heading = doc.create_shared_style(&title_layer);
    for (id, name) in [(style, "Tile"), (heading, "Title")] {
        let s = doc.layer_styles.iter_mut().chain(&mut doc.text_styles).find(|s| s.id == id).expect("created");
        s.name = name.into();
    }
    for lid in tiles {
        doc.pages[0].find_mut(lid).expect("tile").shared_style = Some(style);
    }
    doc.pages[0].find_mut(title).expect("title").shared_style = Some(heading);
}

fn set_text(layer: &mut Layer, f: impl FnOnce(&mut crate::TextLayer)) {
    if let LayerKind::Text(t) = &mut layer.kind {
        f(t);
    }
}
