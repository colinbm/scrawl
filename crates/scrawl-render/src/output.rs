//! Exporting layers to image, SVG and PDF files.

use scrawl_core::exportable::{ExportFormat, FileFormat, export_region};
use scrawl_core::{Document, LayerId, Page};
use vello::peniko;

use crate::export::{Exporter, Image};
use crate::svg::{page_svg, svg_to_pdf};

/// The file contents for exporting layer `id` on `page` of `doc` in `format`. The document's
/// instances and text should be up to date (as they are in the editor).
pub fn export_layer(ex: &mut Exporter, doc: &Document, page: &Page, id: LayerId, format: &ExportFormat) -> Result<Vec<u8>, String> {
    let (region, area) = export_region(page, id).ok_or("there's nothing to export")?;
    match format.format {
        FileFormat::Svg => Ok(page_svg(&region, area, format.scale, &mut ex.text, &doc.assets).into_bytes()),
        FileFormat::Pdf => svg_to_pdf(&page_svg(&region, area, format.scale, &mut ex.text, &doc.assets)),
        raster => {
            ex.images.load(&doc.assets);
            let image = ex.render_page(&region, area, format.scale, peniko::Color::TRANSPARENT)?;
            encode(&image, raster)
        }
    }
}

/// A picture of `doc`'s first page, fitted to `fit` pixels, for version thumbnails and Finder.
/// The document's instances and text should be up to date.
pub fn preview(ex: &mut Exporter, doc: &Document, fit: (f64, f64), background: peniko::Color) -> Result<Image, String> {
    let page = doc.pages.first().ok_or("no pages")?;
    let area = page.content_bounds().ok_or("the page is empty")?;
    let scale = (fit.0 / area.width().max(1.0)).min(fit.1 / area.height().max(1.0));
    ex.images.load(&doc.assets);
    ex.render_page(page, area.inflate(8.0 / scale, 8.0 / scale), scale, background)
}

/// Encodes rendered pixels as PNG, JPEG (over white, which has no alpha) or lossless WebP.
pub fn encode(img: &Image, format: FileFormat) -> Result<Vec<u8>, String> {
    let rgba = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone()).ok_or("bad image size")?;
    let mut out = std::io::Cursor::new(vec![]);
    let result = match format {
        FileFormat::Png => rgba.write_to(&mut out, image::ImageFormat::Png),
        FileFormat::Webp => {
            let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            image::ImageEncoder::write_image(enc, &rgba, img.width, img.height, image::ExtendedColorType::Rgba8)
        }
        FileFormat::Jpg => {
            let rgb = image::RgbImage::from_fn(img.width, img.height, |x, y| {
                let [r, g, b, a] = rgba.get_pixel(x, y).0;
                let over = |c: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
                image::Rgb([over(r), over(g), over(b)])
            });
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
            image::ImageEncoder::write_image(enc, &rgb, img.width, img.height, image::ExtendedColorType::Rgb8)
        }
        FileFormat::Svg | FileFormat::Pdf => return Err("not a raster format".into()),
    };
    result.map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}
