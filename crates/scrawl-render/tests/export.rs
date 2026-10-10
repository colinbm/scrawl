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
