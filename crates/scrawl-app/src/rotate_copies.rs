//! Layer › Path › Rotate Copies…: copies of a layer turned about a pivot, rebuilt live while the
//! pivot is dragged on the canvas or the count changes in the inspector. Enter, Escape or a click
//! elsewhere finishes.

use egui::{Color32, CursorIcon, Painter, PointerButton, Pos2, Rect as ERect, Stroke};
use scrawl_core::document::Location;
use scrawl_core::kurbo::{Affine, Point};
use scrawl_core::{Layer, LayerId, ops};

use crate::app::ScrawlApp;
use crate::canvas::{screen_transform, to_pos};
use crate::style::ACCENT;

const GRAB: f32 = 9.0;

pub struct RotateCopies {
    /// The group of the original and its copies.
    pub group: LayerId,
    original: Layer,
    location: Location,
    pub count: u32,
    /// In the parent's space.
    pivot: Point,
    dragging: bool,
}

/// Starts rotating copies of the single selected layer, five copies about its bottom center.
pub fn start(app: &mut ScrawlApp) {
    let [id] = app.selection[..] else { return };
    let page = app.page();
    let (Some(layer), Some(location)) = (page.find(id).cloned(), page.location(id)) else { return };
    if layer.is_artboard() {
        app.error = Some("Artboards can't be rotated.".into());
        return;
    }
    let Some(b) = layer.bounds_in(Affine::IDENTITY) else { return };
    let mut state = RotateCopies { group: id, original: layer, location, count: 5, pivot: Point::new(b.center().x, b.y1), dragging: false };
    app.canvas.cancel();
    app.canvas.edit = None;
    match build(app, &mut state) {
        true => app.canvas.rotate_copies = Some(state),
        false => app.error = Some("Couldn't rotate copies of that layer.".into()),
    }
}

/// (Re)makes the copies from the original.
fn build(app: &mut ScrawlApp, state: &mut RotateCopies) -> bool {
    let page = app.page_mut();
    if page.contains(state.group) {
        page.remove(state.group);
    }
    if page.insert(state.location, state.original.clone()).is_err() {
        return false;
    }
    match ops::rotate_copies(page, state.original.id, state.count, state.pivot) {
        Some(g) => {
            state.group = g;
            app.selection = vec![g];
            true
        }
        None => false,
    }
}

/// Changes the number of copies.
pub fn set_count(app: &mut ScrawlApp, count: u32) {
    let Some(mut state) = app.canvas.rotate_copies.take() else { return };
    state.count = count.clamp(1, 100);
    build(app, &mut state);
    app.canvas.rotate_copies = Some(state);
}

pub fn finish(app: &mut ScrawlApp) {
    app.canvas.rotate_copies = None;
}

/// Parent space → absolute screen points.
fn to_screen(app: &ScrawlApp, rect: ERect, state: &RotateCopies) -> Affine {
    let parent = app.page().space_transform(state.location.parent).unwrap_or_default();
    screen_transform(app, rect) * parent
}

/// Dragging the pivot; a click elsewhere finishes. Returns whether the pointer was handled.
pub fn pointer(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) -> bool {
    let Some(state) = &app.canvas.rotate_copies else { return false };
    let t = to_screen(app, rect, state);
    let pivot = to_pos(t * state.pivot);
    let near = |p: Option<Pos2>| p.is_some_and(|p| p.distance(pivot) <= GRAB);
    let press = ui.input(|i| i.pointer.press_origin());
    if response.drag_started_by(PointerButton::Primary) && near(press) {
        app.canvas.rotate_copies.as_mut().expect("checked").dragging = true;
    }
    let dragging = app.canvas.rotate_copies.as_ref().is_some_and(|s| s.dragging);
    if dragging {
        if let Some(pos) = response.interact_pointer_pos()
            && response.dragged()
        {
            let mut state = app.canvas.rotate_copies.take().expect("checked");
            state.pivot = t.inverse() * Point::new(pos.x as f64, pos.y as f64);
            build(app, &mut state);
            app.canvas.rotate_copies = Some(state);
        }
        if response.drag_stopped()
            && let Some(s) = app.canvas.rotate_copies.as_mut()
        {
            s.dragging = false;
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        return true;
    }
    if near(response.hover_pos()) {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
        return true;
    }
    if response.clicked() || response.drag_started() {
        finish(app);
    }
    false
}

/// The pivot: a ring with crosshairs, and spokes to each copy.
pub fn paint(app: &ScrawlApp, p: &Painter, rect: ERect) {
    let Some(state) = &app.canvas.rotate_copies else { return };
    let t = to_screen(app, rect, state);
    let pivot = to_pos(t * state.pivot);
    if let Some(children) = app.page().find(state.group).and_then(Layer::children) {
        let group_t = app.page().world_transform(state.group).map(|g| screen_transform(app, rect) * g);
        for c in children {
            if let Some(gt) = group_t {
                let center = to_pos(gt * c.local_transform() * c.frame.local_rect().center());
                p.line_segment([pivot, center], Stroke::new(1.0, ACCENT.gamma_multiply(0.4)));
            }
        }
    }
    p.circle(pivot, 6.0, Color32::WHITE, Stroke::new(1.5, ACCENT));
    p.line_segment([pivot - egui::vec2(10.0, 0.0), pivot + egui::vec2(10.0, 0.0)], Stroke::new(1.0, ACCENT));
    p.line_segment([pivot - egui::vec2(0.0, 10.0), pivot + egui::vec2(0.0, 10.0)], Stroke::new(1.0, ACCENT));
}
