//! Writes a sample document exercising every layer type: `cargo run -p scrawl-core --example sample [out.scrawl]`.

use scrawl_core::kurbo::Point;
use scrawl_core::{Border, BorderPosition, Color, Document, Fill, Frame, Layer, LayerKind, Rectangle, io, ops};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "sample.scrawl".into());
    let mut doc = Document::default();
    let page = &mut doc.pages[0];
    page.name = "Home".into();

    ops::insert_drawn(page, Layer::artboard("iPhone 16", Frame::new(0.0, 0.0, 393.0, 852.0)));
    let mut header = Layer::new("Header", Frame::new(0.0, 0.0, 393.0, 120.0), LayerKind::Rectangle(Rectangle::default()));
    header.style.fills.push(Fill::solid(Color::from_hex("1F8FFF").unwrap()));
    ops::insert_drawn(page, header);

    let mut card = Layer::rectangle(Frame::new(24.0, 150.0, 345.0, 200.0));
    card.kind = LayerKind::Rectangle(Rectangle { corner_radius: 16.0 });
    card.style.fills = vec![Fill::solid(Color::from_hex("F3F0FF").unwrap())];
    card.style.borders = vec![Border { position: BorderPosition::Inside, ..Border::solid(Color::from_hex("7C5CFF").unwrap(), 2.0) }];
    card.name = "Card".into();
    ops::insert_drawn(page, card);

    let mut avatar = Layer::oval(Frame::new(48.0, 174.0, 64.0, 64.0));
    avatar.style.fills = vec![Fill::solid(Color::from_hex("FF8A3D").unwrap())];
    avatar.style.borders.clear();
    avatar.name = "Avatar".into();
    let a = ops::insert_drawn(page, avatar);
    let mut tag = Layer::rectangle(Frame::new(128.0, 190.0, 120.0, 32.0));
    tag.style.fills = vec![Fill::solid(Color::from_hex("7C5CFF").unwrap().with_alpha(0.3))];
    tag.style.borders.clear();
    tag.name = "Tag".into();
    let t = ops::insert_drawn(page, tag);
    let g = ops::group(page, &[a, t]).unwrap();
    page.find_mut(g).unwrap().name = "Profile".into();

    let mut divider = Layer::line(Point::new(24.0, 400.0), Point::new(369.0, 400.0));
    divider.style.borders[0].color = Color::from_hex("CCCCCC").unwrap();
    ops::insert_drawn(page, divider);

    for i in 0..3 {
        let mut tile = Layer::rectangle(Frame::new(24.0 + i as f64 * 119.0, 430.0, 107.0, 107.0));
        tile.kind = LayerKind::Rectangle(Rectangle { corner_radius: 12.0 });
        tile.rotation = i as f64 * 8.0;
        tile.opacity = 1.0 - i as f64 * 0.25;
        tile.style.fills = vec![Fill::solid(Color::from_hex("2EC4B6").unwrap())];
        tile.style.borders = vec![Border { position: BorderPosition::Outside, ..Border::solid(Color::BLACK.with_alpha(0.6), 3.0) }];
        tile.name = format!("Tile {}", i + 1);
        ops::insert_drawn(page, tile);
    }

    ops::insert_drawn(page, Layer::artboard("Desktop", Frame::new(493.0, 0.0, 1024.0, 640.0)));
    let mut hero = Layer::oval(Frame::new(650.0, 150.0, 300.0, 300.0));
    hero.style.fills = vec![Fill::solid(Color::from_hex("FFD23F").unwrap())];
    hero.name = "Sun".into();
    ops::insert_drawn(page, hero);

    io::save(&doc, std::path::Path::new(&out)).expect("save sample");
    println!("wrote {out}");
}
