//! A demo document exercising layer types and styles, used by "Open Sample Document", examples
//! and tests.

use kurbo::Point;

use crate::constraints::Resizing;
use crate::ops::{self, BooleanOp};
use crate::symbols::{OverrideValue, SymbolId, SymbolInstance};
use crate::text::{TextAlign, TextSizing};
use crate::{
    BlendMode, Border, BorderPosition, Color, Document, Fill, FillType, Frame, Gradient, GradientKind, GradientStop, Layer,
    LayerId, LayerKind, Marker, Rectangle, Shadow, Style,
};

fn hex(s: &str) -> Color {
    Color::from_hex(s).expect("valid hex")
}

fn gradient(kind: GradientKind, from: (f64, f64), to: (f64, f64), stops: &[(&str, f64)]) -> Fill {
    let g = Gradient {
        kind,
        from: Point::new(from.0, from.1),
        to: Point::new(to.0, to.1),
        stops: stops.iter().map(|&(c, p)| GradientStop { position: p, color: hex(c), swatch: None }).collect(),
        interpolation: Default::default(),
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
    add_symbols(&mut doc);
    add_prototype(&mut doc);
    crate::symbols::sync_instances(&mut doc, &mut |_| {});
    doc
}

/// A small prototype: the phone artboard is the start point, its card opens the Buttons
/// artboard, and a hotspot there goes back. The header and its title stay put when scrolling.
fn add_prototype(doc: &mut Document) {
    use crate::prototype::{Animation, FlowLink};
    let page = &mut doc.pages[0];
    let id_of = |page: &crate::Page, name: &str| {
        let mut found = None;
        page.walk(|l| {
            if l.name == name && found.is_none() {
                found = Some(l.id);
            }
        });
        found.expect("named layer")
    };
    let (phone, buttons, card) = (id_of(page, "iPhone 16"), id_of(page, "Buttons"), id_of(page, "Card"));
    let fixed = [id_of(page, "Header"), id_of(page, "Good morning")];
    if let Some(LayerKind::Artboard(a)) = page.find_mut(phone).map(|l| &mut l.kind) {
        a.flow_home = true;
    }
    page.find_mut(card).expect("card").flow = Some(FlowLink::to(buttons));
    for id in fixed {
        page.find_mut(id).expect("fixed layer").fixed_to_viewport = true;
    }
    let mut back = Layer::hotspot(Frame::new(0.0, 0.0, 393.0, 24.0));
    back.name = "Back".into();
    back.flow = Some(FlowLink { animation: Animation::SlideFromLeft, ..FlowLink::back() });
    if let Some(children) = page.find_mut(buttons).and_then(Layer::children_mut) {
        children.push(back);
    }
}

/// A master on the Symbols page.
fn master(doc: &mut Document, name: &str, frame: Frame, children: Vec<Layer>) -> (SymbolId, LayerId) {
    let symbol = SymbolId::new();
    let mut m = Layer::artboard(name, frame);
    if let LayerKind::Artboard(a) = &mut m.kind {
        a.children = children;
        a.symbol_id = Some(symbol);
        a.background = Some(Color::TRANSPARENT);
    }
    let page = doc.symbols_page();
    (symbol, ops::insert_root(doc.page_mut(page).expect("symbols page"), m))
}

/// A centered label that stretches with its symbol.
fn label(text: &str, frame: Frame) -> Layer {
    let mut l = Layer::text(frame.origin(), text);
    l.frame = frame;
    l.name = "Label".into();
    l.style.fills = vec![Fill::solid(Color::WHITE)];
    l.resizing = Resizing { left: true, right: true, fix_height: true, ..Default::default() };
    set_text(&mut l, |t| {
        t.sizing = TextSizing::FixedWidth;
        t.style.alignment = TextAlign::Center;
        t.style.font_size = 15.0;
        t.style.font_weight = 600;
    });
    l
}

/// Symbols: icons, a button, and an icon button nesting an icon. Instances go on a second
/// artboard on the first page.
fn add_symbols(doc: &mut Document) {
    let blue = doc.swatches.iter().find(|s| s.name == "Brand Blue").map(|s| (s.id, s.color));
    let pill = |w: f64| {
        let mut bg = Layer::rectangle(Frame::new(0.0, 0.0, w, 44.0));
        bg.kind = LayerKind::Rectangle(Rectangle { corner_radius: 22.0, corner_radii: None });
        bg.name = "Background".into();
        bg.style.borders.clear();
        if let Some((id, color)) = blue {
            bg.style.fills = vec![Fill { swatch: Some(id), ..Fill::solid(color) }];
        }
        bg.resizing = Resizing { left: true, right: true, top: true, bottom: true, ..Default::default() };
        bg
    };

    let mut star = Layer::star(Frame::new(0.0, 0.0, 20.0, 20.0));
    star.style = Style { fills: vec![Fill::solid(hex("FFD23F"))], ..Default::default() };
    let (star_icon, _) = master(doc, "Icon/Star", Frame::new(0.0, 0.0, 20.0, 20.0), vec![star]);
    let mut dot = Layer::oval(Frame::new(3.0, 3.0, 14.0, 14.0));
    dot.style = Style { fills: vec![Fill::solid(hex("2EC4B6"))], ..Default::default() };
    let (dot_icon, _) = master(doc, "Icon/Dot", Frame::new(60.0, 0.0, 20.0, 20.0), vec![dot]);

    let (button, _) = master(doc, "Button", Frame::new(0.0, 80.0, 160.0, 44.0), vec![pill(160.0), label("Button", Frame::new(12.0, 12.0, 136.0, 20.0))]);

    let mut icon = Layer::new("Icon", Frame::new(16.0, 12.0, 20.0, 20.0), LayerKind::Symbol(SymbolInstance::new(star_icon)));
    icon.resizing = Resizing { left: true, fix_width: true, fix_height: true, ..Default::default() };
    let icon_id = icon.id;
    let (icon_button, _) = master(
        doc,
        "Icon Button",
        Frame::new(0.0, 160.0, 180.0, 44.0),
        vec![pill(180.0), icon, label("Favorite", Frame::new(40.0, 12.0, 124.0, 20.0))],
    );

    let page = &mut doc.pages[0];
    let mut ab = Layer::artboard("Buttons", Frame::new(493.0, 0.0, 393.0, 260.0));
    let instance = |name: &str, symbol, frame| Layer::new(name, frame, LayerKind::Symbol(SymbolInstance::new(symbol)));
    let wide = instance("Learn more", button, Frame::new(24.0, 92.0, 240.0, 44.0));
    let mut swapped = instance("Icon Button (dot)", icon_button, Frame::new(24.0, 172.0, 220.0, 44.0));
    if let LayerKind::Symbol(s) = &mut swapped.kind {
        s.set_override(vec![icon_id], Some(OverrideValue::Symbol(Some(dot_icon))));
    }
    if let LayerKind::Artboard(a) = &mut ab.kind {
        a.children = vec![instance("Button", button, Frame::new(24.0, 24.0, 160.0, 44.0)), wide, swapped];
    }
    page.layers.push(ab);
    // Text overrides need the masters' label ids.
    let labels: Vec<(SymbolId, LayerId)> = doc
        .symbol_masters()
        .iter()
        .filter_map(|(_, m)| Some((m.master_of()?, m.children()?.iter().find(|c| c.name == "Label")?.id)))
        .collect();
    let label_of = |symbol| labels.iter().find(|(s, _)| *s == symbol).map(|&(_, id)| id);
    for (name, symbol, text) in [("Learn more", button, "Learn more"), ("Icon Button (dot)", icon_button, "Saved")] {
        let Some(lid) = label_of(symbol) else { continue };
        let Some(ab) = doc.pages[0].layers.last_mut() else { continue };
        let Some(l) = ab.children_mut().and_then(|c| c.iter_mut().find(|l| l.name == name)) else { continue };
        if let LayerKind::Symbol(s) = &mut l.kind {
            s.set_override(vec![lid], Some(OverrideValue::Text(text.into())));
        }
    }
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
