//! The design canvas: input handling for tools, plus selection overlays painted with egui.

use egui::{Color32, CursorIcon, Painter, PointerButton, Pos2, Rect as ERect, Sense, Stroke, StrokeKind, vec2};
use scrawl_core::hit::{self, Hit};
use scrawl_core::kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use scrawl_core::{Frame, Layer, LayerId, LayerKind, Page, ops};

use crate::app::ScrawlApp;
use crate::style::ACCENT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Artboard,
    Rectangle,
    Oval,
    Line,
}

impl Tool {
    pub const INSERT: [Tool; 4] = [Tool::Artboard, Tool::Rectangle, Tool::Oval, Tool::Line];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Artboard => "Artboard",
            Tool::Rectangle => "Rectangle",
            Tool::Oval => "Oval",
            Tool::Line => "Line",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Artboard => "A",
            Tool::Rectangle => "R",
            Tool::Oval => "O",
            Tool::Line => "L",
        }
    }

    pub fn icon(self) -> crate::ui::icons::Icon {
        use crate::ui::icons::Icon;
        match self {
            Tool::Select => Icon::Select,
            Tool::Artboard => Icon::Artboard,
            Tool::Rectangle => Icon::Rectangle,
            Tool::Oval => Icon::Oval,
            Tool::Line => Icon::Line,
        }
    }
}

/// Resize handles, in local layer space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
    Top,
    Right,
    Bottom,
    Left,
}

impl Handle {
    const ALL: [Handle; 8] = [
        Handle::TopLeft,
        Handle::TopRight,
        Handle::BottomRight,
        Handle::BottomLeft,
        Handle::Top,
        Handle::Right,
        Handle::Bottom,
        Handle::Left,
    ];

    /// Unit position on the frame: (0,0) top-left .. (1,1) bottom-right.
    fn unit(self) -> (f64, f64) {
        match self {
            Handle::TopLeft => (0.0, 0.0),
            Handle::Top => (0.5, 0.0),
            Handle::TopRight => (1.0, 0.0),
            Handle::Right => (1.0, 0.5),
            Handle::BottomRight => (1.0, 1.0),
            Handle::Bottom => (0.5, 1.0),
            Handle::BottomLeft => (0.0, 1.0),
            Handle::Left => (0.0, 0.5),
        }
    }

    fn position(self, r: Rect) -> Point {
        let (u, v) = self.unit();
        Point::new(r.x0 + u * r.width(), r.y0 + v * r.height())
    }

    fn is_edge(self) -> bool {
        matches!(self, Handle::Top | Handle::Right | Handle::Bottom | Handle::Left)
    }

    fn cursor(self) -> CursorIcon {
        match self {
            Handle::TopLeft | Handle::BottomRight => CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNeSw,
            Handle::Top | Handle::Bottom => CursorIcon::ResizeVertical,
            Handle::Left | Handle::Right => CursorIcon::ResizeHorizontal,
        }
    }
}

enum Drag {
    Pan,
    Move {
        origin: Point,
        applied: Vec2,
        ids: Vec<LayerId>,
    },
    Resize {
        id: LayerId,
        handle: Handle,
        /// The layer as it was when the drag began; each update reframes a copy of it.
        start: Layer,
        /// Page space → the layer's local space at drag start.
        to_local: Affine,
    },
    LineEnd {
        id: LayerId,
        end: usize,
        start: Layer,
        /// The line's parent space → page space.
        parent_to_page: Affine,
    },
    Marquee {
        origin: Point,
        current: Point,
        base: Vec<LayerId>,
    },
    Create {
        id: LayerId,
        tool: Tool,
        origin: Point,
        /// Page-space origin of the new layer's parent.
        parent_origin: Vec2,
    },
}

pub struct CanvasState {
    drag: Option<Drag>,
    pub hover: Option<LayerId>,
    /// Canvas rect from the most recent frame.
    pub rect: ERect,
    /// Artboard title label rects from the most recent frame, for click-to-select.
    title_hits: Vec<(ERect, LayerId)>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self { drag: None, hover: None, rect: ERect::ZERO, title_hits: vec![] }
    }
}

impl CanvasState {
    /// Abandons any in-progress drag. Returns whether there was one.
    pub fn cancel(&mut self) -> bool {
        self.drag.take().is_some()
    }
}

const HANDLE_SIZE: f32 = 7.0;
const HANDLE_GRAB: f32 = 7.0;
/// Hit tolerance in screen points.
const HIT_TOLERANCE: f64 = 4.0;

pub fn show(app: &mut ScrawlApp, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    app.canvas.rect = rect;

    if app.fit_pending && rect.width() > 1.0 {
        app.fit_pending = false;
        app.zoom_to_fit();
    }

    if response.clicked() || response.drag_started() {
        // Take focus away from inspector fields so keyboard shortcuts reach the canvas.
        ui.ctx().memory_mut(|m| m.request_focus(response.id));
        app.renaming = None;
    }
    handle_navigation(app, ui, &response, rect);
    handle_pointer(app, ui, &response, rect);

    paint_canvas(app, ui, frame, rect);
    paint_overlays(app, &ui.painter_at(rect), rect);
}

fn handle_navigation(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) {
    let (scroll, zoom, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
    if response.hovered() || response.dragged() {
        if zoom != 1.0 {
            let anchor = pointer.unwrap_or(rect.center());
            app.viewport_mut().zoom_about(rect, anchor, zoom as f64);
        } else if scroll != egui::Vec2::ZERO {
            app.viewport_mut().pan_by(scroll);
        }
    }
}

fn space_held(ui: &egui::Ui) -> bool {
    !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_down(egui::Key::Space))
}

fn handle_pointer(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) {
    let vp = app.viewport();
    let (mods, press_origin, latest) =
        ui.input(|i| (i.modifiers, i.pointer.press_origin(), i.pointer.interact_pos().or(i.pointer.hover_pos())));
    let tolerance = HIT_TOLERANCE / vp.zoom;
    let entered = entered_groups(app.page(), &app.selection);

    // --- Drag start ---------------------------------------------------------------------------
    let middle = response.drag_started_by(PointerButton::Middle);
    if middle || (response.drag_started_by(PointerButton::Primary) && space_held(ui)) {
        app.canvas.drag = Some(Drag::Pan);
    } else if response.drag_started_by(PointerButton::Primary)
        && let Some(screen) = press_origin
    {
        let p = vp.to_page(rect, screen);
        app.canvas.drag = match app.tool {
            Tool::Select => begin_select_drag(app, screen, p, mods, tolerance, &entered, rect),
            tool => Some(begin_create(app, tool, round(p))),
        };
    }

    // --- Drag update --------------------------------------------------------------------------
    if let (Some(drag), Some(pos)) = (app.canvas.drag.as_mut(), latest)
        && response.dragged()
    {
        let p = vp.to_page(rect, pos);
        let page_id = app.page;
        let page = app.doc.page_mut(page_id).expect("current page exists");
        match drag {
            Drag::Pan => {
                let d = response.drag_delta();
                app.viewports.entry(page_id).or_default().pan_by(d);
            }
            Drag::Move { origin, applied, ids } => {
                let mut total = round_vec(p - *origin);
                if mods.shift {
                    // Constrain to the dominant axis.
                    if total.x.abs() > total.y.abs() { total.y = 0.0 } else { total.x = 0.0 }
                }
                ops::move_by(page, ids, total - *applied);
                *applied = total;
            }
            Drag::Resize { id, handle, start, to_local } => {
                let r = resized_rect(start.frame.local_rect(), *handle, *to_local * p, mods.shift || app.lock_aspect, mods.alt);
                let mut layer = start.clone();
                layer.reframe(r);
                if layer.rotation == 0.0 {
                    layer.frame = round_frame(layer.frame);
                }
                if let Some(l) = page.find_mut(*id) {
                    *l = layer;
                }
                page.refit_ancestor_groups(*id);
            }
            Drag::LineEnd { id, end, start, parent_to_page } => {
                if let Some([a, b]) = line_endpoints(start).map(|e| e.map(|q| *parent_to_page * q)) {
                    let fixed = if *end == 0 { b } else { a };
                    let mut moved = round(p);
                    if mods.shift {
                        moved = snap_angle(fixed, moved);
                    }
                    let (a, b) = if *end == 0 { (moved, fixed) } else { (fixed, moved) };
                    let inv = parent_to_page.inverse();
                    let line = rebuild_line(start, inv * a, inv * b);
                    if let Some(l) = page.find_mut(*id) {
                        *l = line;
                    }
                    page.refit_ancestor_groups(*id);
                }
            }
            Drag::Marquee { origin, current, base } => {
                *current = p;
                let mut sel = base.clone();
                for id in hit::layers_in_rect(page, Rect::from_points(*origin, p)) {
                    if !sel.contains(&id) {
                        sel.push(id);
                    }
                }
                app.selection = sel;
            }
            Drag::Create { id, tool, origin, parent_origin } => {
                update_create(page, *id, *tool, *origin, round(p), *parent_origin, mods);
            }
        }
    }

    // --- Drag end -----------------------------------------------------------------------------
    if response.drag_stopped() {
        match app.canvas.drag.take() {
            Some(Drag::Move { ids, .. }) => ops::reparent_to_artboards(app.page_mut(), &ids),
            Some(Drag::Create { .. }) => app.tool = Tool::Select,
            _ => {}
        }
    }

    // --- Clicks -------------------------------------------------------------------------------
    if response.clicked_by(PointerButton::Primary)
        && let Some(screen) = press_origin
    {
        let p = vp.to_page(rect, screen);
        match app.tool {
            Tool::Select => click_select(app, screen, p, mods, tolerance, &entered),
            tool => {
                // A click without dragging inserts a default-sized shape.
                let size = if tool == Tool::Artboard { Vec2::new(375.0, 667.0) } else { Vec2::new(100.0, 100.0) };
                if let Drag::Create { id, tool, origin, parent_origin } = begin_create(app, tool, round(p)) {
                    let end = if tool == Tool::Line { origin + Vec2::new(size.x, 0.0) } else { origin + size };
                    update_create(app.page_mut(), id, tool, origin, end, parent_origin, egui::Modifiers::NONE);
                }
                app.tool = Tool::Select;
            }
        }
    }
    if response.double_clicked_by(PointerButton::Primary)
        && app.tool == Tool::Select
        && let Some(screen) = press_origin
    {
        // Double-click drills into the clicked group.
        let p = vp.to_page(rect, screen);
        let mut deeper = entered.clone();
        deeper.extend(app.selection.iter().copied());
        if let Hit::Layer(id) = hit::hit_test(app.page(), p, tolerance, false, &deeper) {
            app.selection = vec![id];
        }
    }

    // --- Hover & cursor -----------------------------------------------------------------------
    app.canvas.hover = None;
    let hover_pos = response.hover_pos();
    let cursor = if matches!(app.canvas.drag, Some(Drag::Pan)) {
        Some(CursorIcon::Grabbing)
    } else if space_held(ui) && response.hovered() {
        Some(CursorIcon::Grab)
    } else if app.tool != Tool::Select && response.hovered() {
        Some(CursorIcon::Crosshair)
    } else if let Some(Drag::Resize { handle, .. }) = &app.canvas.drag {
        Some(handle.cursor())
    } else if app.canvas.drag.is_none()
        && let Some(pos) = hover_pos
    {
        if let Some((_, handle)) = handle_at(app, pos, rect) {
            Some(handle.map_or(CursorIcon::Crosshair, Handle::cursor))
        } else {
            if let Hit::Layer(id) = hit::hit_test(app.page(), vp.to_page(rect, pos), tolerance, mods.command, &entered) {
                app.canvas.hover = Some(id);
            }
            None
        }
    } else {
        None
    };
    if let Some(c) = cursor {
        ui.ctx().set_cursor_icon(c);
    }
}

/// Groups the user has drilled into: ancestors of the current selection.
fn entered_groups(page: &Page, selection: &[LayerId]) -> Vec<LayerId> {
    selection.iter().filter_map(|&id| page.ancestors(id)).flatten().collect()
}

fn title_hit(app: &ScrawlApp, screen: Pos2) -> Option<LayerId> {
    app.canvas.title_hits.iter().rev().find(|(r, _)| r.contains(screen)).map(|&(_, id)| id)
}

fn begin_select_drag(
    app: &mut ScrawlApp,
    screen: Pos2,
    p: Point,
    mods: egui::Modifiers,
    tolerance: f64,
    entered: &[LayerId],
    rect: ERect,
) -> Option<Drag> {
    // Handles of a single selected layer take priority.
    if let Some((id, handle)) = handle_at(app, screen, rect) {
        let page = app.page();
        let start = page.find(id)?.clone();
        return Some(match handle {
            Some(handle) => {
                Drag::Resize { id, handle, to_local: page.world_transform(id)?.inverse(), start }
            }
            None => {
                let end = line_endpoints(&start).map_or(0, |[a, b]| {
                    let t = page.parent_transform(id).unwrap_or_default();
                    let q = app.viewport().to_page(rect, screen);
                    usize::from((t * b - q).hypot() < (t * a - q).hypot())
                });
                Drag::LineEnd { id, end, parent_to_page: page.parent_transform(id)?, start }
            }
        });
    }

    let hit = match title_hit(app, screen) {
        Some(ab) => Hit::Layer(ab),
        None => hit::hit_test(app.page(), p, tolerance, mods.command, entered),
    };
    match hit {
        Hit::Layer(id) => {
            if !app.selection.contains(&id) {
                if mods.shift {
                    app.selection.push(id);
                } else {
                    app.selection = vec![id];
                }
            }
            if mods.alt {
                // Option-drag duplicates.
                app.duplicate_selection();
            }
            Some(Drag::Move { origin: p, applied: Vec2::ZERO, ids: app.selection.clone() })
        }
        Hit::ArtboardBackground(_) | Hit::Nothing => {
            let base = if mods.shift { app.selection.clone() } else { vec![] };
            app.selection = base.clone();
            Some(Drag::Marquee { origin: p, current: p, base })
        }
    }
}

fn click_select(app: &mut ScrawlApp, screen: Pos2, p: Point, mods: egui::Modifiers, tolerance: f64, entered: &[LayerId]) {
    let hit = match title_hit(app, screen) {
        Some(ab) => Hit::Layer(ab),
        None => hit::hit_test(app.page(), p, tolerance, mods.command, entered),
    };
    match hit {
        Hit::Layer(id) if mods.shift => {
            if let Some(i) = app.selection.iter().position(|&s| s == id) {
                app.selection.remove(i);
            } else {
                app.selection.push(id);
            }
        }
        Hit::Layer(id) => app.selection = vec![id],
        Hit::ArtboardBackground(_) | Hit::Nothing => {
            if !mods.shift {
                app.selection.clear();
            }
        }
    }
}

fn begin_create(app: &mut ScrawlApp, tool: Tool, origin: Point) -> Drag {
    let frame = Frame::new(origin.x, origin.y, 0.0, 0.0);
    let layer = match tool {
        Tool::Artboard => {
            let n = app.page().layers.iter().filter(|l| l.is_artboard()).count() + 1;
            Layer::artboard(format!("Artboard {n}"), frame)
        }
        Tool::Rectangle => Layer::rectangle(frame),
        Tool::Oval => Layer::oval(frame),
        Tool::Line => Layer::line(origin, origin),
        Tool::Select => unreachable!("select tool doesn't create layers"),
    };
    let page = app.page_mut();
    let id = ops::insert_drawn(page, layer);
    let parent_origin = page.parent_transform(id).map_or(Vec2::ZERO, |t| t.translation());
    app.selection = vec![id];
    Drag::Create { id, tool, origin, parent_origin }
}

fn update_create(page: &mut Page, id: LayerId, tool: Tool, origin: Point, p: Point, parent_origin: Vec2, mods: egui::Modifiers) {
    let Some(layer) = page.find_mut(id) else { return };
    if tool == Tool::Line {
        let end = if mods.shift { snap_angle(origin, p) } else { p };
        *layer = rebuild_line(layer, origin - parent_origin, end - parent_origin);
        return;
    }
    let mut d = p - origin;
    if mods.shift {
        let s = d.x.abs().max(d.y.abs());
        d = Vec2::new(s.copysign(d.x), s.copysign(d.y));
    }
    let r = if mods.alt { Rect::from_points(origin - d, origin + d) } else { Rect::from_points(origin, origin + d) };
    layer.frame = Frame::from_rect(r - parent_origin);
}

/// Computes the new local rect for a resize drag, given the pointer in local space.
fn resized_rect(r0: Rect, handle: Handle, q: Point, keep_aspect: bool, from_center: bool) -> Rect {
    let (u, v) = handle.unit();
    let (mut x0, mut y0, mut x1, mut y1) = (r0.x0, r0.y0, r0.x1, r0.y1);
    let c = r0.center();
    if u == 0.0 { x0 = q.x } else if u == 1.0 { x1 = q.x }
    if v == 0.0 { y0 = q.y } else if v == 1.0 { y1 = q.y }
    if from_center {
        if u == 0.0 { x1 = 2.0 * c.x - x0 } else if u == 1.0 { x0 = 2.0 * c.x - x1 }
        if v == 0.0 { y1 = 2.0 * c.y - y0 } else if v == 1.0 { y0 = 2.0 * c.y - y1 }
    }
    if keep_aspect && r0.width() > 0.0 && r0.height() > 0.0 {
        let aspect = r0.width() / r0.height();
        let (w, h) = (x1 - x0, y1 - y0);
        let (nw, nh) = if handle.is_edge() {
            if u == 0.5 { (h.abs() * aspect, h) } else { (w, w.abs() / aspect) }
        } else if (w / r0.width()).abs() > (h / r0.height()).abs() {
            (w, (w.abs() / aspect).copysign(h))
        } else {
            ((h.abs() * aspect).copysign(w), h)
        };
        // Grow away from the fixed anchor (or the center).
        if handle.is_edge() {
            if u == 0.5 { x0 = c.x - nw / 2.0; x1 = c.x + nw / 2.0; } else { y0 = c.y - nh / 2.0; y1 = c.y + nh / 2.0; }
        } else if from_center {
            x0 = c.x - nw / 2.0; x1 = c.x + nw / 2.0; y0 = c.y - nh / 2.0; y1 = c.y + nh / 2.0;
        } else {
            if u == 0.0 { x0 = x1 - nw } else { x1 = x0 + nw }
            if v == 0.0 { y0 = y1 - nh } else { y1 = y0 + nh }
        }
    }
    Rect::new(x0, y0, x1, y1)
}

/// The two endpoints, in parent space, of a straight two-point path (a "line").
fn line_endpoints(layer: &Layer) -> Option<[Point; 2]> {
    let LayerKind::Path(p) = &layer.kind else { return None };
    if p.closed || p.points.len() != 2 || layer.kind.type_name() != "Line" {
        return None;
    }
    let t = layer.local_transform();
    let s = |q: Point| t * Point::new(q.x * layer.frame.width, q.y * layer.frame.height);
    Some([s(p.points[0].point), s(p.points[1].point)])
}

/// A copy of `like` (keeping id, name and style) re-shaped as a line from `a` to `b`.
fn rebuild_line(like: &Layer, a: Point, b: Point) -> Layer {
    let fresh = Layer::line(a, b);
    Layer { frame: fresh.frame, kind: fresh.kind, rotation: 0.0, flip_horizontal: false, flip_vertical: false, ..like.clone() }
}

fn snap_angle(from: Point, to: Point) -> Point {
    let d = to - from;
    let step = std::f64::consts::FRAC_PI_4;
    let angle = (d.atan2() / step).round() * step;
    from + Vec2::from_angle(angle) * d.hypot()
}

fn round(p: Point) -> Point {
    Point::new(p.x.round(), p.y.round())
}

fn round_vec(v: Vec2) -> Vec2 {
    Vec2::new(v.x.round(), v.y.round())
}

fn round_frame(f: Frame) -> Frame {
    let r = f.to_rect();
    Frame::from_rect(Rect::new(r.x0.round(), r.y0.round(), r.x1.round(), r.y1.round()))
}

/// The handle under `screen` for a single, unlocked selected layer. `Some(None)` means a line
/// endpoint.
fn handle_at(app: &ScrawlApp, screen: Pos2, rect: ERect) -> Option<(LayerId, Option<Handle>)> {
    let [id] = app.selection[..] else { return None };
    let page = app.page();
    let layer = page.find(id)?;
    if layer.locked {
        return None;
    }
    let to_screen = screen_transform(app, rect);
    if let Some(ends) = line_endpoints(layer) {
        let t = to_screen * page.parent_transform(id)?;
        return ends.iter().any(|&e| to_pos(t * e).distance(screen) <= HANDLE_GRAB).then_some((id, None));
    }
    let t = to_screen * page.world_transform(id)?;
    let local = layer.frame.local_rect();
    visible_handles(t, local)
        .find(|&h| to_pos(t * h.position(local)).distance(screen) <= HANDLE_GRAB)
        .map(|h| (id, Some(h)))
}

/// Edge handles are hidden when the layer is too small on screen to fit them.
fn visible_handles(t: Affine, local: Rect) -> impl Iterator<Item = Handle> {
    let w = (t * Point::new(local.x1, local.y0) - t * local.origin()).hypot();
    let h = (t * Point::new(local.x0, local.y1) - t * local.origin()).hypot();
    Handle::ALL.into_iter().filter(move |hd| !hd.is_edge() || w.min(h) > 4.0 * HANDLE_SIZE as f64)
}

/// Page space → absolute screen points.
fn screen_transform(app: &ScrawlApp, rect: ERect) -> Affine {
    Affine::translate((rect.min.x as f64, rect.min.y as f64)) * app.viewport().transform()
}

fn to_pos(p: Point) -> Pos2 {
    Pos2::new(p.x as f32, p.y as f32)
}

// ---- Painting -------------------------------------------------------------------------------

fn canvas_background(ui: &egui::Ui) -> vello::peniko::Color {
    if ui.visuals().dark_mode {
        vello::peniko::Color::new([0.118, 0.118, 0.125, 1.0])
    } else {
        vello::peniko::Color::new([0.945, 0.945, 0.95, 1.0])
    }
}

fn paint_canvas(app: &mut ScrawlApp, ui: &egui::Ui, frame: &mut eframe::Frame, rect: ERect) {
    let Some(rs) = frame.wgpu_render_state() else {
        app.gpu_error.get_or_insert_with(|| "wgpu is not available".into());
        return;
    };
    if app.gpu.is_none() && app.gpu_error.is_none() {
        match crate::gpu::VelloCanvas::new(rs) {
            Ok(c) => app.gpu = Some(c),
            Err(e) => app.gpu_error = Some(e),
        }
    }
    if let Some(err) = &app.gpu_error {
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, err, egui::FontId::proportional(14.0), Color32::RED);
        return;
    }

    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = ((rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32);
    app.scene.reset();
    let view = Affine::scale(ppp as f64) * app.viewport().transform();
    let page = app.doc.page(app.page).unwrap_or(&app.doc.pages[0]);
    scrawl_render::render_page(&mut app.scene, page, view);

    let bg = canvas_background(ui);
    let gpu = app.gpu.as_mut().expect("initialized above");
    if let Some(tex) = gpu.render(rs, &app.scene, w, h, bg) {
        let uv = ERect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        ui.painter().image(tex, rect, uv, Color32::WHITE);
    }
}

/// Flattens a path to screen-space polylines.
fn polylines(path: &BezPath, t: Affine) -> Vec<(Vec<Pos2>, bool)> {
    let mut out: Vec<(Vec<Pos2>, bool)> = vec![];
    scrawl_core::kurbo::flatten((t * path.clone()).elements().iter().copied(), 0.25, |el| match el {
        scrawl_core::kurbo::PathEl::MoveTo(p) => out.push((vec![to_pos(p)], false)),
        scrawl_core::kurbo::PathEl::LineTo(p) => {
            if let Some((pts, _)) = out.last_mut() {
                pts.push(to_pos(p));
            }
        }
        scrawl_core::kurbo::PathEl::ClosePath => {
            if let Some((_, closed)) = out.last_mut() {
                *closed = true;
            }
        }
        _ => {}
    });
    out
}

/// Draws the outline of a layer's geometry (or frame, for groups).
fn paint_outline(p: &Painter, page: &Page, id: LayerId, to_screen: Affine, stroke: Stroke) {
    let (Some(layer), Some(world)) = (page.find(id), page.world_transform(id)) else { return };
    let t = to_screen * world;
    let path = match layer.local_path() {
        Some(path) if !layer.is_artboard() => path,
        _ => layer.frame.local_rect().to_path(0.1),
    };
    for (pts, closed) in polylines(&path, t) {
        if closed {
            p.add(egui::Shape::closed_line(pts, stroke));
        } else {
            p.add(egui::Shape::line(pts, stroke));
        }
    }
}

fn paint_overlays(app: &mut ScrawlApp, p: &Painter, rect: ERect) {
    let to_screen = screen_transform(app, rect);
    let page = app.doc.page(app.page).unwrap_or(&app.doc.pages[0]);
    let text_color = p.ctx().style_of(p.ctx().theme()).visuals.weak_text_color();

    // Artboard titles.
    app.canvas.title_hits.clear();
    for l in page.layers.iter().filter(|l| l.is_artboard() && l.visible) {
        let pos = to_pos(to_screen * l.frame.origin()) - vec2(0.0, 4.0);
        let color = if app.selection.contains(&l.id) { ACCENT } else { text_color };
        let r = p.text(pos, egui::Align2::LEFT_BOTTOM, &l.name, egui::FontId::proportional(11.0), color);
        app.canvas.title_hits.push((r.expand(2.0), l.id));
    }

    // Hover outline.
    if let Some(h) = app.canvas.hover.filter(|h| !app.selection.contains(h)) {
        paint_outline(p, page, h, to_screen, Stroke::new(1.5, ACCENT));
    }

    // Selection.
    let thin = Stroke::new(1.0, ACCENT);
    for &id in &app.selection {
        paint_outline(p, page, id, to_screen, thin);
    }
    if app.selection.len() > 1
        && let Some(b) = page.selection_bounds(&app.selection)
    {
        let r = ERect::from_two_pos(to_pos(to_screen * b.origin()), to_pos(to_screen * Point::new(b.x1, b.y1)));
        p.rect_stroke(r, 0.0, thin, StrokeKind::Middle);
    }
    if let [id] = app.selection[..]
        && let Some(layer) = page.find(id)
        && !layer.locked
        && !matches!(app.canvas.drag, Some(Drag::Move { .. }))
    {
        let handle = |center: Pos2, round: bool| {
            let r = ERect::from_center_size(center, egui::Vec2::splat(HANDLE_SIZE));
            let corner = if round { HANDLE_SIZE / 2.0 } else { 1.0 };
            p.rect(r, corner, Color32::WHITE, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        };
        if let Some(ends) = line_endpoints(layer) {
            let t = to_screen * page.parent_transform(id).unwrap_or_default();
            ends.iter().for_each(|&e| handle(to_pos(t * e), true));
        } else if let Some(world) = page.world_transform(id) {
            let t = to_screen * world;
            let local = layer.frame.local_rect();
            visible_handles(t, local).for_each(|h| handle(to_pos(t * h.position(local)), false));
        }
    }

    // Marquee.
    if let Some(Drag::Marquee { origin, current, .. }) = &app.canvas.drag {
        let r = ERect::from_two_pos(to_pos(to_screen * *origin), to_pos(to_screen * *current));
        p.rect(r, 0.0, ACCENT.gamma_multiply(0.08), thin, StrokeKind::Inside);
    }

    // Size readout while drawing or resizing.
    let sizing = match &app.canvas.drag {
        Some(Drag::Resize { id, .. } | Drag::Create { id, .. } | Drag::LineEnd { id, .. }) => Some(*id),
        _ => None,
    };
    if let Some(id) = sizing
        && let (Some(layer), Some(b)) = (page.find(id), page.world_bounds(id))
    {
        let label = match line_endpoints(layer) {
            Some([a, b]) => format!("{:.0}", (b - a).hypot()),
            None => format!("{:.0} × {:.0}", layer.frame.width, layer.frame.height),
        };
        let anchor = to_pos(to_screen * Point::new(b.center().x, b.y1)) + vec2(0.0, 10.0);
        let galley = p.layout_no_wrap(label, egui::FontId::proportional(11.0), Color32::WHITE);
        let bg = ERect::from_center_size(anchor + vec2(0.0, galley.size().y / 2.0), galley.size() + vec2(10.0, 4.0));
        p.rect_filled(bg, 3.0, ACCENT);
        p.galley(bg.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_resize_with_aspect_lock() {
        let r0 = Rect::new(0.0, 0.0, 100.0, 50.0);
        let r = resized_rect(r0, Handle::BottomRight, Point::new(200.0, 60.0), true, false);
        assert_eq!(r, Rect::new(0.0, 0.0, 200.0, 100.0));
        let r = resized_rect(r0, Handle::TopLeft, Point::new(-100.0, 0.0), true, false);
        assert_eq!(r, Rect::new(-100.0, -50.0, 100.0, 50.0));
    }

    #[test]
    fn edge_resize_from_center() {
        let r0 = Rect::new(0.0, 0.0, 100.0, 50.0);
        let r = resized_rect(r0, Handle::Right, Point::new(120.0, 999.0), false, true);
        assert_eq!(r, Rect::new(-20.0, 0.0, 120.0, 50.0));
    }

    #[test]
    fn angle_snaps_to_45() {
        let p = snap_angle(Point::ORIGIN, Point::new(100.0, 10.0));
        assert!((p.y).abs() < 1e-9 && (p.x - 100.5).abs() < 0.1);
    }
}
