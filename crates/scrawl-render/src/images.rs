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

/// Side of the generated noise tile, in pixels (and local units, at scale 1).
pub const NOISE_TILE: u32 = 128;

/// A seamless tile of grain for a noise fill, made once per kind, intensity and color.
pub fn noise_image(noise: scrawl_core::Noise, color: scrawl_core::Color) -> ImageData {
    use scrawl_core::NoiseKind;
    use std::sync::{Mutex, OnceLock};
    /// Kind, intensity bits and color bits.
    type Key = (NoiseKind, u64, [u32; 4]);
    static CACHE: OnceLock<Mutex<HashMap<Key, ImageData>>> = OnceLock::new();
    let key = (noise.kind, noise.intensity.to_bits(), [color.r, color.g, color.b, color.a].map(f32::to_bits));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(img) = cache.lock().expect("noise cache").get(&key) {
        return img.clone();
    }
    let n = NOISE_TILE;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    // A fixed xorshift sequence, so the grain doesn't change between frames or exports.
    let mut state: u32 = 0x9E37_79B9;
    let k = noise.intensity.clamp(0.0, 1.0);
    for _ in 0..n * n {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let v = (state >> 8) as f64 / (1u32 << 24) as f64;
        let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        let rgba = match noise.kind {
            NoiseKind::Original => [byte(v), byte(v), byte(v), byte(k)],
            NoiseKind::Black => [0, 0, 0, byte(v * k)],
            NoiseKind::White => [255, 255, 255, byte(v * k)],
            NoiseKind::Color => [byte(color.r as f64), byte(color.g as f64), byte(color.b as f64), byte(v * k * color.a as f64)],
        };
        px.extend_from_slice(&rgba);
    }
    let img = ImageData { data: Blob::from(px), format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::Alpha, width: n, height: n };
    cache.lock().expect("noise cache").insert(key, img.clone());
    img
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
