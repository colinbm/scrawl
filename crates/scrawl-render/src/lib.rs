//! Builds Vello scenes from Scrawl documents.

use scrawl_core::style::stroke_style;
use scrawl_core::{BorderPosition, Color, FillRule, Layer, LayerKind, Page};
use vello::kurbo::{Affine, BezPath, Rect, Shape};
use vello::peniko::{self, BlendMode, Compose, Fill, Mix};
use vello::Scene;

pub fn to_peniko(c: Color) -> peniko::Color {
    peniko::Color::new([c.r, c.g, c.b, c.a])
}

/// Appends `page` to `scene`, with `view` mapping page space to target pixels.
pub fn render_page(scene: &mut Scene, page: &Page, view: Affine) {
    // Artboard drop shadows go underneath everything.
    for layer in page.layers.iter().filter(|l| l.visible && l.is_artboard()) {
        let t = view * layer.local_transform();
        let r = layer.frame.local_rect();
        scene.draw_blurred_rounded_rect(
            t * Affine::translate((0.0, 1.0)),
            r,
            peniko::Color::new([0.0, 0.0, 0.0, 0.18]),
            0.0,
            2.0,
        );
    }
    for layer in &page.layers {
        render_layer(scene, layer, view);
    }
}

fn render_layer(scene: &mut Scene, layer: &Layer, parent: Affine) {
    if !layer.visible || layer.opacity <= 0.0 {
        return;
    }
    let t = parent * layer.local_transform();
    let needs_opacity_layer = layer.opacity < 1.0;
    if needs_opacity_layer {
        // The clip is the layer's bounds in its parent space, padded for borders.
        let Some(bounds) = layer.bounds_in(Affine::IDENTITY) else { return };
        let pad = layer.style.border_outset() + 1.0;
        scene.push_layer(Fill::NonZero, Mix::Normal, layer.opacity as f32, parent, &bounds.inflate(pad, pad));
    }

    match &layer.kind {
        LayerKind::Artboard(ab) => {
            let rect = layer.frame.local_rect();
            scene.fill(Fill::NonZero, t, to_peniko(ab.background.unwrap_or(Color::WHITE)), None, &rect);
            if ab.clip_content {
                scene.push_clip_layer(Fill::NonZero, t, &rect);
            }
            for child in &ab.children {
                render_layer(scene, child, t);
            }
            if ab.clip_content {
                scene.pop_layer();
            }
        }
        LayerKind::Group(g) => {
            for child in &g.children {
                render_layer(scene, child, t);
            }
        }
        _ => {
            if let Some(path) = layer.local_path() {
                render_shape(scene, layer, &path, t);
            }
        }
    }

    if needs_opacity_layer {
        scene.pop_layer();
    }
}

fn render_shape(scene: &mut Scene, layer: &Layer, path: &BezPath, t: Affine) {
    let rule = match layer.fill_rule() {
        FillRule::NonZero => Fill::NonZero,
        FillRule::EvenOdd => Fill::EvenOdd,
    };
    if layer.is_closed() {
        for fill in layer.style.fills.iter().filter(|f| f.enabled) {
            scene.fill(rule, t, to_peniko(fill.color), None, path);
        }
    }
    for border in layer.style.borders.iter().filter(|b| b.enabled && b.thickness > 0.0) {
        let color = to_peniko(border.color);
        // Open paths have no inside or outside, so their borders are always centered.
        let position = if layer.is_closed() { border.position } else { BorderPosition::Center };
        match position {
            BorderPosition::Center => {
                scene.stroke(&stroke_style(border.thickness), t, color, None, path);
            }
            BorderPosition::Inside => {
                // A double-width stroke clipped to the shape leaves exactly the inner half.
                scene.push_clip_layer(rule, t, path);
                scene.stroke(&stroke_style(border.thickness * 2.0), t, color, None, path);
                scene.pop_layer();
            }
            BorderPosition::Outside => {
                // A double-width stroke with the shape's interior knocked out.
                let bounds: Rect = path.bounding_box().inflate(border.thickness + 1.0, border.thickness + 1.0);
                scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t, &bounds);
                scene.stroke(&stroke_style(border.thickness * 2.0), t, color, None, path);
                scene.push_layer(rule, BlendMode::new(Mix::Normal, Compose::DestOut), 1.0, t, path);
                scene.fill(rule, t, peniko::Color::BLACK, None, path);
                scene.pop_layer();
                scene.pop_layer();
            }
        }
    }
}
