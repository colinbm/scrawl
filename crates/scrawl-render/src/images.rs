//! Decoded images for rendering image layers and image fills.

use std::collections::HashMap;

use scrawl_core::ImageMode;
use scrawl_core::assets::Assets;
use vello::kurbo::{Affine, Rect};
use vello::peniko::{self, Blob, Extend, ImageAlphaType, ImageBrush, ImageData, ImageFormat};

/// Decoded pixels for a document's image assets, by asset id.
#[derive(Default)]
pub struct ImageCache {
    /// `None` for assets that failed to decode, so they aren't retried every frame.
    decoded: HashMap<String, Option<ImageData>>,
}

/// Decodes PNG, JPEG, GIF or WebP bytes to straight-alpha RGBA.
pub fn decode(bytes: &[u8]) -> Option<ImageData> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(ImageData { data: Blob::from(rgba.into_raw()), format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::Alpha, width, height })
}

impl ImageCache {
    /// Decodes any of `assets` not seen before.
    pub fn load(&mut self, assets: &Assets) {
        for (id, bytes) in assets.iter() {
            if !self.decoded.contains_key(id) {
                self.decoded.insert(id.clone(), decode(bytes));
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&ImageData> {
        self.decoded.get(id)?.as_ref()
    }

    /// An image's size in pixels.
    pub fn size(&self, id: &str) -> Option<(u32, u32)> {
        self.get(id).map(|i| (i.width, i.height))
    }
}

/// A brush painting image fill `image` over a `width`×`height` frame, with its brush transform,
/// and the area it covers when that's smaller than the frame (Fit leaves the rest empty).
pub fn fill_brush(image: &ImageData, mode: ImageMode, scale: f64, width: f64, height: f64) -> (peniko::Brush, Affine, Option<Rect>) {
    let (iw, ih) = (image.width.max(1) as f64, image.height.max(1) as f64);
    let (w, h) = (width.max(1e-3), height.max(1e-3));
    let centered = |s: f64| {
        let (dx, dy) = ((w - iw * s) / 2.0, (h - ih * s) / 2.0);
        (Affine::translate((dx, dy)) * Affine::scale(s), Rect::new(dx, dy, dx + iw * s, dy + ih * s))
    };
    let (transform, area, extend) = match mode {
        ImageMode::Stretch => (Affine::scale_non_uniform(w / iw, h / ih), None, Extend::Pad),
        ImageMode::Fill => (centered((w / iw).max(h / ih)).0, None, Extend::Pad),
        ImageMode::Fit => {
            let (t, area) = centered((w / iw).min(h / ih));
            (t, Some(area), Extend::Pad)
        }
        ImageMode::Tile => (Affine::scale(scale.max(0.01)), None, Extend::Repeat),
    };
    let brush = ImageBrush::new(image.clone()).with_extend(extend);
    (peniko::Brush::Image(brush), transform, area)
}
