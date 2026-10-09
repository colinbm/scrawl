//! Shadows.
//!
//! Vello has no general blur filter, so blurred silhouettes of arbitrary shapes are approximated
//! by stacking bands: copies of the shape dilated or eroded by a range of distances, each at an
//! alpha chosen so the combined edge follows a Gaussian falloff. Rectangles and circles use
//! Vello's exact analytic blurred rounded rect instead. A GPU blur pass can replace
//! [`blurred_silhouette`] later without touching callers.

use std::f64::consts::SQRT_2;

use scrawl_core::{Layer, LayerKind, Shadow};
use vello::Scene;
use vello::kurbo::{Affine, BezPath, Rect, Shape, Stroke};
use vello::peniko::{self, BlendMode, Compose, Fill, Mix};

use crate::to_peniko;

/// Paints a drop shadow for `layer`, whose geometry is `path` (local space). `parent` maps the
/// layer's parent space to the target; shadow offsets are in parent space so they don't rotate
/// with the layer.
pub fn drop_shadow(scene: &mut Scene, layer: &Layer, path: &BezPath, rule: Fill, parent: Affine, s: &Shadow) {
    let local = layer.local_transform();
    let t = parent * Affine::translate((s.offset_x, s.offset_y)) * local;
    let sigma = s.blur / 2.0;

    if let Some((rect, radius)) = analytic_rounded_rect(layer) {
        let r = rect.inflate(s.spread, s.spread);
        if r.width() > 0.0 && r.height() > 0.0 {
            scene.draw_blurred_rounded_rect(t, r, to_peniko(s.color), (radius + s.spread).max(0.0), sigma.max(0.01));
        }
        return;
    }
    // Open paths cast a shadow of their stroke.
    let silhouette = if layer.is_closed() {
        path.clone()
    } else {
        let width = layer.style.borders.iter().filter(|b| b.enabled).map(|b| b.thickness).fold(0.0, f64::max);
        if width <= 0.0 {
            return;
        }
        let stroke = scrawl_core::style::stroke_style(width, &layer.style.border_options);
        vello::kurbo::stroke(path.elements().iter().copied(), &stroke, &Default::default(), 0.05)
    };
    let rule = if layer.is_closed() { rule } else { Fill::NonZero };
    blurred_silhouette(scene, &silhouette, rule, t, sigma, s.spread, s.color);
}

/// Paints an inner shadow, clipped to the shape.
pub fn inner_shadow(scene: &mut Scene, layer: &Layer, path: &BezPath, rule: Fill, parent: Affine, s: &Shadow) {
    let t = parent * layer.local_transform();
    let local_offset = {
        // The offset is in parent space; express it in the layer's local space.
        let inv = layer.local_transform().inverse();
        inv * vello::kurbo::Point::new(s.offset_x, s.offset_y) - inv * vello::kurbo::Point::ORIGIN
    };
    let sigma = s.blur / 2.0;
    // The inverse of the shape: a generous rect with the shape cut out (even-odd).
    let pad = 3.0 * sigma + s.spread.abs() + local_offset.hypot() + 2.0;
    let bounds = path.bounding_box().inflate(pad, pad);
    let mut inverse = bounds.to_path(0.1);
    inverse.extend(path.elements().iter().copied());
    let inverse = Affine::translate(local_offset) * inverse;

    scene.push_clip_layer(rule, t, path);
    blurred_silhouette(scene, &inverse, Fill::EvenOdd, t, sigma, s.spread, s.color);
    scene.pop_layer();
}

/// Rectangles with uniform corners and circles can use Vello's exact blurred rounded rect.
fn analytic_rounded_rect(layer: &Layer) -> Option<(Rect, f64)> {
    let r = layer.frame.local_rect();
    match &layer.kind {
        LayerKind::Rectangle(rect) if rect.corner_radii.is_none_or(|c| c.iter().all(|&x| x == c[0])) => {
            let radius = rect.corner_radii.map_or(rect.corner_radius, |c| c[0]);
            Some((r, radius.clamp(0.0, r.width().min(r.height()) / 2.0)))
        }
        LayerKind::Oval if (r.width() - r.height()).abs() < 1e-6 => Some((r, r.width() / 2.0)),
        _ => None,
    }
}

/// Standard normal CDF via an erf approximation (Abramowitz–Stegun 7.1.26).
fn normal_cdf(x: f64) -> f64 {
    let z = x / SQRT_2;
    let t = 1.0 / (1.0 + 0.3275911 * z.abs());
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592)
            * t
            * (-z * z).exp();
    let erf = if z >= 0.0 { y } else { -y };
    0.5 * (1.0 + erf)
}

/// The band distances (positive = dilate) and per-band alphas approximating a Gaussian blur of
/// the given sigma. Bands are ordered from the outermost (largest dilation) inwards.
fn bands(sigma: f64) -> Vec<(f64, f32)> {
    let n = ((4.0 * sigma / 1.5).ceil() as usize).clamp(4, 24);
    let extent = 2.0 * sigma;
    let step = 2.0 * extent / n as f64;
    let mut out = Vec::with_capacity(n);
    let mut prev_cover = 0.0_f64;
    for k in 0..n {
        // Band k reaches distances below r. Once it's added, coverage applies on (r - step, r],
        // so match the Gaussian at that interval's midpoint.
        let r = extent - k as f64 * step;
        let cover = if k + 1 == n { 1.0 } else { normal_cdf(-(r - step / 2.0) / sigma) };
        let alpha = if prev_cover >= 1.0 { 0.0 } else { 1.0 - (1.0 - cover) / (1.0 - prev_cover) };
        out.push((r, alpha.clamp(0.0, 1.0) as f32));
        prev_cover = cover;
    }
    out
}

/// Fills `path` blurred by `sigma` and grown by `spread`, in `color`.
fn blurred_silhouette(scene: &mut Scene, path: &BezPath, rule: Fill, t: Affine, sigma: f64, spread: f64, color: scrawl_core::Color) {
    let opaque = peniko::Color::new([color.r, color.g, color.b, 1.0]);
    let pad = 3.0 * sigma + spread.abs() + 1.0;
    let clip = path.bounding_box().inflate(pad, pad);
    // Draw all bands opaque within a group, then apply the shadow color's alpha once.
    scene.push_layer(Fill::NonZero, Mix::Normal, color.a, t, &clip);
    if sigma < 0.5 {
        dilated(scene, path, rule, t, spread, 1.0, opaque, &clip);
    } else {
        for (r, alpha) in bands(sigma) {
            if alpha > 0.0 {
                dilated(scene, path, rule, t, r + spread, alpha, opaque, &clip);
            }
        }
    }
    scene.pop_layer();
}

/// Paints `path` grown by `d` (or shrunk, if negative) at `alpha`.
#[allow(clippy::too_many_arguments)]
fn dilated(scene: &mut Scene, path: &BezPath, rule: Fill, t: Affine, d: f64, alpha: f32, color: peniko::Color, clip: &Rect) {
    scene.push_layer(Fill::NonZero, Mix::Normal, alpha, t, clip);
    scene.fill(rule, t, color, None, path);
    if d > 0.01 {
        scene.stroke(&Stroke::new(2.0 * d), t, color, None, path);
    } else if d < -0.01 {
        // Erode: knock out a band along the edge.
        scene.push_layer(Fill::NonZero, BlendMode::new(Mix::Normal, Compose::DestOut), 1.0, t, clip);
        scene.stroke(&Stroke::new(-2.0 * d), t, color, None, path);
        scene.pop_layer();
    }
    scene.pop_layer();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_follow_gaussian_profile() {
        let sigma = 4.0;
        let b = bands(sigma);
        // Coverage at distance d (positive = outside) is 1 - Π(1 - a) over bands reaching d.
        let coverage = |d: f64| 1.0 - b.iter().filter(|(r, _)| *r > d).fold(1.0, |acc, (_, a)| acc * (1.0 - *a as f64));
        for d in [-6.0, -2.0, 0.0, 2.0, 6.0] {
            let target = normal_cdf(-d / sigma);
            assert!((coverage(d) - target).abs() < 0.12, "d={d}: {} vs {target}", coverage(d));
        }
        assert!(coverage(-10.0) > 0.99 && coverage(10.0) < 0.01);
    }

    #[test]
    fn cdf_sanity() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((normal_cdf(1.0) - 0.8413).abs() < 1e-3);
    }
}
