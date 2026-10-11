//! Pixel checks through the headless exporter. Skipped when no GPU adapter is available.

use scrawl_core::kurbo::Rect;
use scrawl_core::{Color, Fill, Frame, Layer, Page, Shadow, ops};
use scrawl_render::export::{Exporter, Image};
use vello::peniko;

fn exporter() -> Option<Exporter> {
    match Exporter::new() {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
    }
}

fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    img.pixels[i..i + 4].try_into().unwrap()
}

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3)
}

#[test]
fn fills_masks_and_shadows() {
    let Some(mut ex) = exporter() else { return };
    let mut page = Page::new("P");
    let mut red = Layer::rectangle(Frame::new(10.0, 10.0, 20.0, 20.0));
    red.style.fills = vec![Fill::solid(Color::rgba(1.0, 0.0, 0.0, 1.0))];
    red.style.borders.clear();
    red.style.shadows = vec![Shadow { offset_x: 0.0, offset_y: 0.0, blur: 4.0, ..Shadow::drop() }];
    ops::insert_drawn(&mut page, red);

    // A mask: only the left half of the blue square should show.
    let mut mask = Layer::rectangle(Frame::new(50.0, 10.0, 10.0, 20.0));
    mask.style.fills.clear();
    mask.style.borders.clear();
    mask.is_mask = true;
    ops::insert_drawn(&mut page, mask);
    let mut blue = Layer::rectangle(Frame::new(50.0, 10.0, 20.0, 20.0));
    blue.style.fills = vec![Fill::solid(Color::rgba(0.0, 0.0, 1.0, 1.0))];
    blue.style.borders.clear();
    ops::insert_drawn(&mut page, blue);

    let img = ex.render_page(&page, Rect::new(0.0, 0.0, 80.0, 40.0), 1.0, peniko::Color::WHITE).unwrap();
    assert_eq!((img.width, img.height), (80, 40));
    assert!(close(px(&img, 20, 20), [255, 0, 0, 255]), "fill: {:?}", px(&img, 20, 20));
    assert!(close(px(&img, 2, 20), [255, 255, 255, 255]), "far background: {:?}", px(&img, 2, 20));
    let shadow = px(&img, 31, 20);
    assert!(shadow[0] < 250 && shadow[0] > 150, "shadow just outside the edge: {shadow:?}");
    assert!(close(px(&img, 55, 20), [0, 0, 255, 255]), "inside mask: {:?}", px(&img, 55, 20));
    assert!(close(px(&img, 65, 20), [255, 255, 255, 255]), "outside mask: {:?}", px(&img, 65, 20));
}

/// A 2×1 PNG: red on the left, blue on the right.
fn two_pixel_png() -> Vec<u8> {
    let img = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn image_layers_and_fills() {
    let Some(mut ex) = exporter() else { return };
    let mut doc = scrawl_core::Document::default();
    let id = doc.assets.add_image(two_pixel_png()).unwrap();
    let page = &mut doc.pages[0];
    // The bitmap stretched over a 40×20 frame.
    ops::insert_drawn(page, Layer::image(id.clone(), Frame::new(0.0, 0.0, 40.0, 20.0)));
    // A fitted image fill in a 40×40 square: the image is 40×20, centered vertically.
    let mut square = Layer::rectangle(Frame::new(60.0, 0.0, 40.0, 40.0));
    square.style.fills = vec![Fill::image(id, scrawl_core::ImageMode::Fit)];
    square.style.borders.clear();
    ops::insert_drawn(page, square);

    ex.images.load(&doc.assets);
    let img = ex.render_page(&doc.pages[0], Rect::new(0.0, 0.0, 100.0, 40.0), 1.0, peniko::Color::WHITE).unwrap();
    assert!(close(px(&img, 5, 10), [255, 0, 0, 255]), "{:?}", px(&img, 5, 10));
    assert!(close(px(&img, 35, 10), [0, 0, 255, 255]));
    assert!(close(px(&img, 65, 20), [255, 0, 0, 255]));
    assert!(close(px(&img, 95, 20), [0, 0, 255, 255]));
    // Fit leaves the rest of the square empty.
    assert!(close(px(&img, 80, 3), [255, 255, 255, 255]), "{:?}", px(&img, 80, 3));
}

/// Exports the sample's first artboard in every format; the SVG, rasterized by resvg, should
/// look like Vello's render.
#[test]
fn sample_exports_in_every_format() {
    use scrawl_core::exportable::{ExportFormat, FileFormat};
    use scrawl_render::output::export_layer;
    let Some(mut ex) = exporter() else { return };
    let mut doc = scrawl_core::sample::sample_document();
    ex.text.fit_document(&mut doc);
    let page = doc.pages[0].clone();
    let artboard = page.layers.iter().find(|l| l.is_artboard()).unwrap().id;
    let fmt = |format, scale| ExportFormat { scale, suffix: String::new(), format };

    let png = export_layer(&mut ex, &doc, &page, artboard, &fmt(FileFormat::Png, 2.0)).unwrap();
    let raster = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(raster.dimensions(), (786, 1704));
    for (format, magic) in [(FileFormat::Jpg, &b"\xFF\xD8\xFF"[..]), (FileFormat::Webp, b"RIFF"), (FileFormat::Pdf, b"%PDF")] {
        let bytes = export_layer(&mut ex, &doc, &page, artboard, &fmt(format, 1.0)).unwrap();
        assert!(bytes.starts_with(magic), "{format:?}");
    }

    let svg = export_layer(&mut ex, &doc, &page, artboard, &fmt(FileFormat::Svg, 1.0)).unwrap();
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).unwrap();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(393, 852).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let vello = image::imageops::resize(&raster, 393, 852, image::imageops::FilterType::Triangle);
    // Mean channel difference over the artboard; text and blurs differ slightly between renderers.
    let total: u64 = pixmap
        .pixels()
        .iter()
        .zip(vello.pixels())
        .map(|(a, b)| {
            let a = a.demultiply();
            [a.red(), a.green(), a.blue()].iter().zip(&b.0[..3]).map(|(x, y)| x.abs_diff(*y) as u64).sum::<u64>()
        })
        .sum();
    let mean = total as f64 / (393.0 * 852.0 * 3.0);
    if let Some(dir) = std::env::var_os("SCRAWL_UI_SHOTS") {
        pixmap.save_png(std::path::Path::new(&dir).join("sample_svg.png")).unwrap();
        vello.save(std::path::Path::new(&dir).join("sample_vello.png")).unwrap();
    }
    assert!(mean < 6.0, "SVG export differs from the render: mean difference {mean:.2}");
}

#[test]
fn shape_groups_draw_their_combined_outline() {
    let Some(mut ex) = exporter() else { return };
    let mut page = Page::new("P");
    let mut square = Layer::rectangle(Frame::new(0.0, 0.0, 40.0, 40.0));
    square.style.fills = vec![Fill::solid(Color::rgba(0.0, 0.6, 0.0, 1.0))];
    square.style.borders.clear();
    let a = ops::insert_drawn(&mut page, square);
    let b = ops::insert_drawn(&mut page, Layer::rectangle(Frame::new(10.0, 10.0, 20.0, 20.0)));
    let g = ops::boolean(&mut page, &[a, b], ops::BooleanOp::Subtract).unwrap();
    let img = ex.render_page(&page, Rect::new(0.0, 0.0, 40.0, 40.0), 1.0, peniko::Color::WHITE).unwrap();
    assert!(close(px(&img, 5, 20), [0, 153, 0, 255]), "the group's fill: {:?}", px(&img, 5, 20));
    assert!(close(px(&img, 20, 20), [255, 255, 255, 255]), "the hole: {:?}", px(&img, 20, 20));
    // The cut-out shape's own (grey) style isn't drawn; changing the operation fills the hole.
    ops::boolean(&mut page, &[g], ops::BooleanOp::Union).unwrap();
    let img = ex.render_page(&page, Rect::new(0.0, 0.0, 40.0, 40.0), 1.0, peniko::Color::WHITE).unwrap();
    assert!(close(px(&img, 20, 20), [0, 153, 0, 255]), "unioned: {:?}", px(&img, 20, 20));
}

#[test]
fn alpha_and_luminance_masks_and_escaping_them() {
    let Some(mut ex) = exporter() else { return };
    let solid = |frame: Frame, c: Color| {
        let mut l = Layer::rectangle(frame);
        l.style.fills = vec![Fill::solid(c)];
        l.style.borders.clear();
        l
    };
    for mode in [scrawl_core::MaskMode::Alpha, scrawl_core::MaskMode::Luminance] {
        let mut page = Page::new("P");
        // The mask: left half opaque white, right half 50% grey (alpha mode: half-transparent).
        let mut mask = solid(Frame::new(0.0, 0.0, 20.0, 20.0), Color::WHITE);
        mask.is_mask = true;
        mask.mask_mode = mode;
        let m = ops::insert_drawn(&mut page, mask);
        let mut right = solid(Frame::new(20.0, 0.0, 20.0, 20.0), if mode == scrawl_core::MaskMode::Alpha { Color::rgba(1.0, 1.0, 1.0, 0.5) } else { Color::rgba(0.0, 0.0, 0.0, 1.0) });
        right.is_mask = false;
        // Group the two halves as one mask.
        let r = ops::insert_drawn(&mut page, right);
        let g = ops::group(&mut page, &[m, r]).unwrap();
        page.find_mut(g).unwrap().is_mask = true;
        page.find_mut(g).unwrap().mask_mode = mode;
        page.find_mut(g).unwrap().children_mut().unwrap()[0].is_mask = false;
        ops::insert_drawn(&mut page, solid(Frame::new(0.0, 0.0, 40.0, 20.0), Color::rgba(1.0, 0.0, 0.0, 1.0)));
        // A layer that ignores the mask shows everywhere.
        let mut free = solid(Frame::new(0.0, 25.0, 40.0, 10.0), Color::rgba(0.0, 0.0, 1.0, 1.0));
        free.breaks_mask = true;
        ops::insert_drawn(&mut page, free);
        let img = ex.render_page(&page, Rect::new(0.0, 0.0, 40.0, 40.0), 1.0, peniko::Color::TRANSPARENT).unwrap();
        let left = px(&img, 10, 10);
        let right = px(&img, 30, 10);
        assert!(left[0] > 200 && left[3] > 200, "{mode:?} left shows red: {left:?}");
        match mode {
            scrawl_core::MaskMode::Alpha => assert!(right[3] > 60 && right[3] < 200, "{mode:?} right is half see-through: {right:?}"),
            _ => assert!(right[3] < 40 || right[0] < 60, "{mode:?} black hides: {right:?}"),
        }
        assert!(close(px(&img, 30, 30), [0, 0, 255, 255]), "{mode:?} the escaping layer: {:?}", px(&img, 30, 30));
    }
}

#[test]
fn svg_alpha_masks_render_in_other_viewers() {
    let mut page = Page::new("P");
    let mut mask = Layer::rectangle(Frame::new(0.0, 0.0, 20.0, 20.0));
    mask.style.fills = vec![Fill::solid(Color::rgba(1.0, 1.0, 1.0, 0.5))];
    mask.style.borders.clear();
    mask.is_mask = true;
    mask.mask_mode = scrawl_core::MaskMode::Alpha;
    ops::insert_drawn(&mut page, mask);
    let mut red = Layer::rectangle(Frame::new(0.0, 0.0, 40.0, 20.0));
    red.style.fills = vec![Fill::solid(Color::rgba(1.0, 0.0, 0.0, 1.0))];
    red.style.borders.clear();
    ops::insert_drawn(&mut page, red);
    let svg = scrawl_render::svg::page_svg(&page, Rect::new(0.0, 0.0, 40.0, 20.0), 1.0, &mut scrawl_render::TextEngine::new(), &Default::default());
    assert!(svg.contains("<mask"), "{svg}");
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &Default::default()).unwrap();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(40, 20).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let a = |x: u32| pixmap.pixel(x, 10).unwrap().alpha();
    assert!(a(10) > 150 && a(10) < 255, "masked at half opacity, over the mask's own half fill: {}", a(10));
    assert_eq!(a(30), 0, "outside the mask");
}

#[test]
fn noise_fills_and_oklab_gradients() {
    let Some(mut ex) = exporter() else { return };
    let mut page = Page::new("P");
    let mut grain = Layer::rectangle(Frame::new(0.0, 0.0, 64.0, 32.0));
    grain.style.borders.clear();
    grain.style.fills = vec![
        Fill::solid(Color::rgba(0.5, 0.5, 0.5, 1.0)),
        Fill { fill_type: scrawl_core::FillType::Noise, noise: scrawl_core::Noise { kind: scrawl_core::NoiseKind::Black, intensity: 1.0 }, ..Fill::solid(Color::WHITE) },
    ];
    ops::insert_drawn(&mut page, grain);
    let mut g = Layer::rectangle(Frame::new(0.0, 40.0, 100.0, 10.0));
    g.style.borders.clear();
    let mut gradient = scrawl_core::Gradient {
        from: scrawl_core::kurbo::Point::new(0.0, 0.5),
        to: scrawl_core::kurbo::Point::new(1.0, 0.5),
        stops: vec![
            scrawl_core::GradientStop { position: 0.0, color: Color::rgba(1.0, 0.0, 0.0, 1.0), swatch: None },
            scrawl_core::GradientStop { position: 1.0, color: Color::rgba(0.0, 0.0, 1.0, 1.0), swatch: None },
        ],
        ..Default::default()
    };
    gradient.interpolation = scrawl_core::Interpolation::Oklab;
    let expected = gradient.color_at(0.505).to_rgba8();
    g.style.fills = vec![Fill { fill_type: scrawl_core::FillType::Gradient, gradient, ..Fill::solid(Color::BLACK) }];
    ops::insert_drawn(&mut page, g);
    let img = ex.render_page(&page, Rect::new(0.0, 0.0, 100.0, 50.0), 1.0, peniko::Color::WHITE).unwrap();
    // Grain: neighbouring pixels differ, and on average it darkens the grey.
    let row: Vec<u8> = (0..64).map(|x| px(&img, x, 10)[0]).collect();
    let distinct = row.iter().collect::<std::collections::BTreeSet<_>>().len();
    assert!(distinct > 10, "grain varies: {row:?}");
    let mean = row.iter().map(|&v| v as f64).sum::<f64>() / 64.0;
    assert!(mean < 110.0, "black noise darkens: {mean}");
    // The GPU blends in OKLab like the model does.
    let mid = px(&img, 50, 45);
    assert!(mid.iter().zip(expected).take(3).all(|(a, b)| a.abs_diff(b) <= 8), "{mid:?} vs {expected:?}");
}

fn svg_pixels(page: &Page, area: Rect) -> resvg::tiny_skia::Pixmap {
    let svg = scrawl_render::svg::page_svg(page, area, 1.0, &mut scrawl_render::TextEngine::new(), &Default::default());
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &Default::default()).unwrap();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(area.width() as u32, area.height() as u32).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap
}

#[test]
fn svg_keeps_markers_angular_gradients_and_spread() {
    use scrawl_core::kurbo::Point;
    // A line ending in a filled arrow: the arrowhead's wide part is off the line itself.
    let mut page = Page::new("P");
    let mut line = Layer::line(Point::new(10.0, 20.0), Point::new(90.0, 20.0));
    line.style.borders[0] = scrawl_core::Border::solid(Color::rgba(0.0, 0.0, 0.0, 1.0), 2.0);
    line.style.border_options.end_marker = scrawl_core::Marker::FilledArrow;
    ops::insert_drawn(&mut page, line);
    let px = svg_pixels(&page, Rect::new(0.0, 0.0, 100.0, 40.0));
    assert!(px.pixel(85, 22).unwrap().alpha() > 150, "arrowhead below the line");
    assert_eq!(px.pixel(30, 25).unwrap().alpha(), 0, "nothing below the line elsewhere");

    // An angular gradient: red at the top going round to blue.
    let mut page = Page::new("P");
    let mut sq = Layer::rectangle(Frame::new(0.0, 0.0, 100.0, 100.0));
    sq.style.borders.clear();
    let g = scrawl_core::Gradient {
        kind: scrawl_core::GradientKind::Angular,
        stops: vec![
            scrawl_core::GradientStop { position: 0.0, color: Color::rgba(1.0, 0.0, 0.0, 1.0), swatch: None },
            scrawl_core::GradientStop { position: 1.0, color: Color::rgba(0.0, 0.0, 1.0, 1.0), swatch: None },
        ],
        ..Default::default()
    };
    let expect_right = g.color_at(0.25).to_rgba8();
    sq.style.fills = vec![Fill { fill_type: scrawl_core::FillType::Gradient, gradient: g, ..Fill::solid(Color::BLACK) }];
    ops::insert_drawn(&mut page, sq);
    let px = svg_pixels(&page, Rect::new(0.0, 0.0, 100.0, 100.0));
    let right = px.pixel(95, 50).unwrap();
    assert!(right.red().abs_diff(expect_right[0]) < 12 && right.blue().abs_diff(expect_right[2]) < 12, "{right:?} vs {expect_right:?}");
    let top = px.pixel(55, 2).unwrap();
    assert!(top.red() > 200 && top.blue() < 60, "red at the start: {top:?}");

    // Spread: a sharp shadow grown by 10 shows beyond the shape.
    let mut page = Page::new("P");
    let mut sq = Layer::rectangle(Frame::new(30.0, 30.0, 40.0, 40.0));
    sq.style.borders.clear();
    sq.style.shadows = vec![scrawl_core::Shadow { offset_x: 0.0, offset_y: 0.0, blur: 0.0, spread: 10.0, ..scrawl_core::Shadow::drop() }];
    ops::insert_drawn(&mut page, sq);
    let px = svg_pixels(&page, Rect::new(0.0, 0.0, 100.0, 100.0));
    assert!(px.pixel(24, 50).unwrap().alpha() > 30, "spread shadow left of the shape");
    assert_eq!(px.pixel(12, 50).unwrap().alpha(), 0, "but not past the spread");
}
