//! The design canvas: input handling for tools, plus selection overlays painted with egui.

use egui::{Color32, CursorIcon, Painter, PointerButton, Pos2, Rect as ERect, Sense, Stroke, StrokeKind, vec2};
use scrawl_core::hit::{self, Hit};
use scrawl_core::snap::{self, Feedback, Targets};
use scrawl_core::kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use scrawl_core::{FillType, Frame, Gradient, Layer, LayerId, LayerKind, Page, ops};

use crate::app::ScrawlApp;
use crate::style::ACCENT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Artboard,
    Rectangle,
    Oval,
    Line,
    Pen,
    Triangle,
    Polygon,
    Star,
    Text,
}

impl Tool {
    pub const INSERT: [Tool; 9] = [
        Tool::Artboard,
        Tool::Rectangle,
        Tool::Oval,
        Tool::Line,
        Tool::Pen,
        Tool::Text,
        Tool::Triangle,
        Tool::Polygon,
        Tool::Star,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Artboard => "Artboard",
            Tool::Rectangle => "Rectangle",
            Tool::Oval => "Oval",
            Tool::Line => "Line",
            Tool::Pen => "Vector",
            Tool::Triangle => "Triangle",
            Tool::Polygon => "Polygon",
            Tool::Star => "Star",
            Tool::Text => "Text",
        }
    }

    /// Single-key shortcut, or "" if none (the extra shapes have none).
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Artboard => "A",
            Tool::Rectangle => "R",
            Tool::Oval => "O",
            Tool::Line => "L",
            Tool::Pen => "P",
            Tool::Text => "T",
            Tool::Triangle | Tool::Polygon | Tool::Star => "",
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
            Tool::Pen => Icon::Pen,
            Tool::Triangle => Icon::Triangle,
            Tool::Polygon => Icon::Polygon,
            Tool::Star => Icon::Star,
            Tool::Text => Icon::Text,
        }
    }
}

/// Identifies a gradient being edited: a fill or border of a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientTarget {
    pub layer: LayerId,
    pub border: bool,
    pub index: usize,
}

impl GradientTarget {
    pub fn gradient<'a>(&self, layer: &'a Layer) -> Option<&'a Gradient> {
        let (ft, g) = if self.border {
            let b = layer.style.borders.get(self.index)?;
            (b.fill_type, &b.gradient)
        } else {
            let f = layer.style.fills.get(self.index)?;
            (f.fill_type, &f.gradient)
        };
        (ft == FillType::Gradient).then_some(g)
    }

    fn gradient_mut<'a>(&self, layer: &'a mut Layer) -> Option<&'a mut Gradient> {
        let g = if self.border {
            &mut layer.style.borders.get_mut(self.index)?.gradient
        } else {
            &mut layer.style.fills.get_mut(self.index)?.gradient
        };
        Some(g)
    }
}

/// Screen positions of the from/to handles of the gradient being edited, if it's visible.
fn gradient_handles(app: &ScrawlApp, rect: ERect) -> Option<(GradientTarget, [Pos2; 2])> {
    let target = app.gradient_edit?;
    if app.selection != [target.layer] || app.canvas.edit.is_some() {
        return None;
    }
    let page = app.page();
    let layer = page.find(target.layer)?;
    let g = target.gradient(layer)?;
    let t = screen_transform(app, rect) * page.world_transform(target.layer)?;
    let (w, h) = (layer.frame.width, layer.frame.height);
    let at = |p: Point| to_pos(t * Point::new(p.x * w, p.y * h));
    Some((target, [at(g.from), at(g.to)]))
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
    GradientHandle {
        target: GradientTarget,
        end: usize,
        /// Page space → the layer's local space.
        to_local: Affine,
    },
    Rotate {
        id: LayerId,
        center: Point,
        start_angle: f64,
        start_rotation: f64,
    },
    ResizeMany {
        handle: Handle,
        bounds: Rect,
        /// Each layer at drag start with its local → page transform.
        starts: Vec<(Layer, Affine)>,
    },
    Move {
        origin: Point,
        applied: Vec2,
        ids: Vec<LayerId>,
        /// Page-space bounds of the moving layers at drag start, for snapping.
        start: Option<Rect>,
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

impl Drag {
    /// The layers this drag moves or reshapes, if it should snap.
    fn moving(&self) -> Option<Vec<LayerId>> {
        match self {
            Drag::Move { ids, .. } => Some(ids.clone()),
            Drag::Resize { id, .. } | Drag::LineEnd { id, .. } => Some(vec![*id]),
            Drag::ResizeMany { starts, .. } => Some(starts.iter().map(|(l, _)| l.id).collect()),
            _ => None,
        }
    }
}

pub struct CanvasState {
    drag: Option<Drag>,
    /// Point-editing mode for a path layer.
    pub edit: Option<crate::vector::VectorEdit>,
    /// A path being drawn with the pen tool.
    pub pen: Option<crate::vector::PenState>,
    /// A text layer being edited in place.
    pub text_edit: Option<crate::text_edit::TextEditing>,
    pub hover: Option<LayerId>,
    /// Canvas rect from the most recent frame.
    pub rect: ERect,
    /// Artboard title label rects from the most recent frame, for click-to-select.
    title_hits: Vec<(ERect, LayerId)>,
    /// What the current drag can snap to.
    snap: Option<Targets>,
    /// Smart guides to show for the current drag.
    feedback: Feedback,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            drag: None,
            edit: None,
            pen: None,
            text_edit: None,
            hover: None,
            rect: ERect::ZERO,
            title_hits: vec![],
            snap: None,
            feedback: Feedback::default(),
        }
    }
}

impl CanvasState {
    /// Abandons any in-progress drag. Returns whether there was one.
    pub fn cancel(&mut self) -> bool {
        self.snap = None;
        self.feedback = Feedback::default();
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
        // Take focus away from inspector fields so keyboard shortcuts reach the canvas. (Focusing
        // the canvas itself would count as "egui wants keyboard input" and block shortcuts.)
        ui.ctx().memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        app.renaming = None;
    }
    handle_navigation(app, ui, &response, rect);
    if !handle_pan(app, ui, &response) && !crate::text_edit::pointer(app, ui, &response, rect) {
        if app.canvas.edit.is_some() {
            crate::vector::edit_pointer(app, ui, &response, rect);
        } else if app.tool == Tool::Pen {
            crate::vector::pen_pointer(app, ui, &response, rect);
        } else {
            handle_pointer(app, ui, &response, rect);
        }
    }

    paint_canvas(app, ui, frame, rect);
    let painter = ui.painter_at(rect);
    paint_overlays(app, &painter, rect);
    crate::vector::paint_edit(app, &painter, rect);
    crate::vector::paint_pen(app, &painter, rect, response.hover_pos());
    crate::text_edit::paint(app, &painter, rect);
}

/// Space-drag and middle-drag panning, available in every mode. Returns whether a pan is in
/// progress (so other handlers should ignore the pointer).
fn handle_pan(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response) -> bool {
    let start = response.drag_started_by(PointerButton::Middle)
        || (response.drag_started_by(PointerButton::Primary) && space_held(ui));
    if start {
        app.canvas.drag = Some(Drag::Pan);
    }
    if !matches!(app.canvas.drag, Some(Drag::Pan)) {
        return false;
    }
    if response.dragged() {
        let d = response.drag_delta();
        app.viewport_mut().pan_by(d);
    }
    if response.drag_stopped() {
        app.canvas.drag = None;
    }
    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    true
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
    let snap_threshold = crate::snapping::THRESHOLD / vp.zoom;
    let entered = entered_groups(app.page(), &app.selection);

    // --- Drag start ---------------------------------------------------------------------------
    if response.drag_started_by(PointerButton::Primary)
        && let Some(screen) = press_origin
    {
        let p = vp.to_page(rect, screen);
        app.canvas.snap = None;
        app.canvas.drag = match app.tool {
            Tool::Select => begin_select_drag(app, screen, p, mods, tolerance, &entered, rect),
            tool => {
                let mut origin = round(p);
                if app.snap_to_layers {
                    let targets = crate::snapping::targets(app, &[], Some(p));
                    if !mods.ctrl {
                        origin = snap::snap_point(origin, &targets, snap_threshold, (true, true)).0;
                    }
                    app.canvas.snap = Some(targets);
                }
                Some(begin_create(app, tool, origin))
            }
        };
        if app.tool == Tool::Select
            && app.snap_to_layers
            && let Some(moving) = app.canvas.drag.as_ref().and_then(Drag::moving)
        {
            app.canvas.snap = Some(crate::snapping::targets(app, &moving, None));
        }
    }

    // --- Drag update --------------------------------------------------------------------------
    if let (Some(drag), Some(pos)) = (app.canvas.drag.as_mut(), latest)
        && response.dragged()
    {
        let p = vp.to_page(rect, pos);
        let page = app.doc.page_mut(app.page).expect("current page exists");
        // ⌃ suspends snapping for as long as it's held.
        let snap = app.canvas.snap.as_ref().filter(|_| !mods.ctrl);
        let mut fb = Feedback::default();
        let snap_handle = |p: Point, handle: Handle, aligned: bool, fb: &mut Feedback| match snap {
            Some(t) if aligned => {
                let (u, v) = handle.unit();
                let (q, f) = snap::snap_point(p, t, snap_threshold, (u != 0.5, v != 0.5));
                *fb = f;
                q
            }
            _ => p,
        };
        match drag {
            Drag::Pan => {}
            Drag::GradientHandle { target, end, to_local } => {
                let local = *to_local * p;
                if let Some(layer) = page.find_mut(target.layer) {
                    let (w, h) = (layer.frame.width, layer.frame.height);
                    let norm = Point::new(if w > 0.0 { local.x / w } else { 0.0 }, if h > 0.0 { local.y / h } else { 0.0 });
                    if let Some(g) = target.gradient_mut(layer) {
                        if *end == 0 { g.from = norm } else { g.to = norm }
                    }
                }
            }
            Drag::Rotate { id, center, start_angle, start_rotation } => {
                let mut delta = ((p - *center).atan2() - *start_angle).to_degrees();
                if mods.shift {
                    delta = (delta / 15.0).round() * 15.0;
                }
                if let Some(l) = page.find_mut(*id) {
                    l.rotation = (*start_rotation + delta).rem_euclid(360.0);
                }
                page.refit_ancestor_groups(*id);
            }
            Drag::ResizeMany { handle, bounds, starts } => {
                let p = snap_handle(p, *handle, true, &mut fb);
                let nb = resized_rect(*bounds, *handle, p, mods.shift || app.lock_aspect, mods.alt);
                let sx = if bounds.width() > 0.0 { nb.width() / bounds.width() } else { 1.0 };
                let sy = if bounds.height() > 0.0 { nb.height() / bounds.height() } else { 1.0 };
                let scale = Affine::translate(nb.origin().to_vec2())
                    * Affine::scale_non_uniform(sx, sy)
                    * Affine::translate(-bounds.origin().to_vec2());
                for (start, world) in starts.iter() {
                    // Map the layer's local rect through the scale and back into local space.
                    let back = world.inverse() * scale * *world;
                    let r = (back * start.frame.local_rect().to_path(0.1)).bounding_box();
                    let mut layer = start.clone();
                    layer.reframe(r);
                    if let Some(l) = page.find_mut(start.id) {
                        *l = layer;
                    }
                }
                if let Some((first, _)) = starts.first() {
                    page.refit_ancestor_groups(first.id);
                }
            }
            Drag::Move { origin, applied, ids, start } => {
                let mut total = round_vec(p - *origin);
                let mut axes = (true, true);
                if mods.shift {
                    // Constrain to the dominant axis.
                    if total.x.abs() > total.y.abs() {
                        total.y = 0.0;
                        axes.1 = false;
                    } else {
                        total.x = 0.0;
                        axes.0 = false;
                    }
                }
                if let (Some(t), Some(b)) = (snap, start) {
                    let (offset, f) = snap::snap_rect(*b + total, t, snap_threshold, axes);
                    total += offset;
                    fb = f;
                }
                ops::move_by(page, ids, total - *applied);
                *applied = total;
            }
            Drag::Resize { id, handle, start, to_local } => {
                let p = snap_handle(p, *handle, crate::snapping::axis_aligned(*to_local), &mut fb);
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
                    } else if let Some(t) = snap {
                        let (q, f) = snap::snap_point(moved, t, snap_threshold, (true, true));
                        moved = q;
                        fb = f;
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
                let mut q = round(p);
                if let Some(t) = snap
                    && !(*tool == Tool::Line && mods.shift)
                {
                    let (s, f) = snap::snap_point(q, t, snap_threshold, (true, true));
                    q = s;
                    fb = f;
                }
                update_create(page, *id, *tool, *origin, q, *parent_origin, mods);
            }
        }
        app.canvas.feedback = fb;
    }

    // --- Drag end -----------------------------------------------------------------------------
    if response.drag_stopped() {
        app.canvas.snap = None;
        app.canvas.feedback = Feedback::default();
        match app.canvas.drag.take() {
            Some(Drag::Move { ids, .. }) => ops::reparent_to_artboards(app.page_mut(), &ids),
            Some(Drag::Create { id, tool: Tool::Text, .. }) => {
                let narrow = app.page().find(id).is_some_and(|l| l.frame.width < 8.0);
                if narrow {
                    set_text_sizing(app, id, scrawl_core::TextSizing::Auto);
                }
                crate::text_edit::begin(app, id, false);
            }
            Some(Drag::Create { .. }) => app.tool = Tool::Select,
            _ => {}
        }
    }

    // --- Clicks -------------------------------------------------------------------------------
    if response.clicked_by(PointerButton::Primary)
        && let Some(screen) = response.interact_pointer_pos()
    {
        let p = vp.to_page(rect, screen);
        match app.tool {
            Tool::Select => click_select(app, screen, p, mods, tolerance, &entered),
            tool => {
                // A click without dragging inserts a default-sized shape.
                let size = if tool == Tool::Artboard { Vec2::new(375.0, 667.0) } else { Vec2::new(100.0, 100.0) };
                if let Drag::Create { id, tool, origin, parent_origin } = begin_create(app, tool, round(p)) {
                    if tool == Tool::Text {
                        set_text_sizing(app, id, scrawl_core::TextSizing::Auto);
                        crate::text_edit::begin(app, id, false);
                        return;
                    }
                    let end = if tool == Tool::Line { origin + Vec2::new(size.x, 0.0) } else { origin + size };
                    update_create(app.page_mut(), id, tool, origin, end, parent_origin, egui::Modifiers::NONE);
                }
                app.tool = Tool::Select;
            }
        }
    }
    if response.double_clicked_by(PointerButton::Primary)
        && app.tool == Tool::Select
        && let Some(screen) = response.interact_pointer_pos()
    {
        // Double-click drills into the clicked group.
        let p = vp.to_page(rect, screen);
        let mut deeper = entered.clone();
        deeper.extend(app.selection.iter().copied());
        if let Hit::Layer(id) = hit::hit_test(app.page(), p, tolerance, false, &deeper) {
            let is_shape = app.page().find(id).is_some_and(|l| l.children().is_none());
            if app.selection == [id] && is_shape {
                app.enter_vector_edit(id);
            } else {
                app.selection = vec![id];
            }
        }
    }

    // --- Hover & cursor -----------------------------------------------------------------------
    app.canvas.hover = None;
    let hover_pos = response.hover_pos();
    let cursor = if space_held(ui) && response.hovered() {
        Some(CursorIcon::Grab)
    } else if app.tool != Tool::Select && response.hovered() {
        Some(CursorIcon::Crosshair)
    } else if let Some(Drag::Resize { handle, .. } | Drag::ResizeMany { handle, .. }) = &app.canvas.drag {
        Some(handle.cursor())
    } else if let Some(Drag::Rotate { .. }) = &app.canvas.drag {
        Some(CursorIcon::Alias)
    } else if app.canvas.drag.is_none()
        && let Some(pos) = hover_pos
    {
        let on_gradient = gradient_handles(app, rect).is_some_and(|(_, e)| e.iter().any(|e| e.distance(pos) <= HANDLE_GRAB));
        if on_gradient {
            Some(CursorIcon::Move)
        } else if let Some(hit) = handle_at(app, pos, rect) {
            Some(match hit {
                HandleHit::LineEnd(_) => CursorIcon::Crosshair,
                HandleHit::Single(_, h) if mods.command && !h.is_edge() => CursorIcon::Alias,
                HandleHit::Single(_, h) | HandleHit::Multi(h) => h.cursor(),
            })
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

fn set_text_sizing(app: &mut ScrawlApp, id: LayerId, sizing: scrawl_core::TextSizing) {
    if let Some(LayerKind::Text(t)) = app.page_mut().find_mut(id).map(|l| &mut l.kind) {
        t.sizing = sizing;
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
    // Gradient handles, then selection handles, take priority.
    if let Some((target, ends)) = gradient_handles(app, rect)
        && let Some(end) = ends.iter().position(|e| e.distance(screen) <= HANDLE_GRAB)
    {
        let to_local = app.page().world_transform(target.layer)?.inverse();
        return Some(Drag::GradientHandle { target, end, to_local });
    }
    if let Some(hit) = handle_at(app, screen, rect) {
        let page = app.page();
        return Some(match hit {
            HandleHit::Single(id, handle) if mods.command && !handle.is_edge() => {
                // ⌘-drag on a corner rotates.
                let layer = page.find(id)?;
                let center = page.world_transform(id)? * layer.frame.local_rect().center();
                Drag::Rotate { id, center, start_angle: (p - center).atan2(), start_rotation: layer.rotation }
            }
            HandleHit::Single(id, handle) => {
                // Resizing text fixes its width (side handles) or its whole frame.
                if let Some(LayerKind::Text(t)) = page.find(id).map(|l| &l.kind) {
                    let sizing = match (handle, t.sizing) {
                        (Handle::Left | Handle::Right, scrawl_core::TextSizing::Auto) => scrawl_core::TextSizing::FixedWidth,
                        (Handle::Left | Handle::Right, s) => s,
                        _ => scrawl_core::TextSizing::Fixed,
                    };
                    set_text_sizing(app, id, sizing);
                }
                let page = app.page();
                let start = page.find(id)?.clone();
                Drag::Resize { id, handle, to_local: page.world_transform(id)?.inverse(), start }
            }
            HandleHit::LineEnd(id) => {
                let start = page.find(id)?.clone();
                let end = line_endpoints(&start).map_or(0, |[a, b]| {
                    let t = page.parent_transform(id).unwrap_or_default();
                    usize::from((t * b - p).hypot() < (t * a - p).hypot())
                });
                Drag::LineEnd { id, end, parent_to_page: page.parent_transform(id)?, start }
            }
            HandleHit::Multi(handle) => {
                let ids = ops::normalize_selection(page, &app.selection);
                let starts =
                    ids.iter().filter_map(|&id| Some((page.find(id)?.clone(), page.world_transform(id)?))).collect();
                Drag::ResizeMany { handle, bounds: page.selection_bounds(&ids)?, starts }
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
            let ids = app.selection.clone();
            let start = app.page().selection_bounds(&ops::normalize_selection(app.page(), &ids));
            Some(Drag::Move { origin: p, applied: Vec2::ZERO, ids, start })
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
        Tool::Triangle => Layer::polygon(frame, 3),
        Tool::Polygon => Layer::polygon(frame, 5),
        Tool::Star => Layer::star(frame),
        Tool::Text => {
            // Dragging out a text box makes it fixed-width; a plain click switches it to auto.
            let mut l = Layer::text(origin, "");
            if let LayerKind::Text(t) = &mut l.kind {
                t.sizing = scrawl_core::TextSizing::FixedWidth;
            }
            l
        }
        Tool::Select | Tool::Pen => unreachable!("tool doesn't draw by dragging a frame"),
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
    if layer.kind.type_name() != "Line" {
        return None;
    }
    let p = &p.subpaths[0];
    let t = layer.local_transform();
    let s = |q: Point| t * Point::new(q.x * layer.frame.width, q.y * layer.frame.height);
    Some([s(p.points[0].point), s(p.points[1].point)])
}

/// A copy of `like` (keeping id, name and style) re-shaped as a line from `a` to `b`.
fn rebuild_line(like: &Layer, a: Point, b: Point) -> Layer {
    let fresh = Layer::line(a, b);
    Layer { frame: fresh.frame, kind: fresh.kind, rotation: 0.0, flip_horizontal: false, flip_vertical: false, ..like.clone() }
}

pub(crate) fn snap_angle(from: Point, to: Point) -> Point {
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

enum HandleHit {
    /// A resize handle of the single selected layer.
    Single(LayerId, Handle),
    /// An endpoint of a selected line.
    LineEnd(LayerId),
    /// A handle on the bounds of a multi-layer selection.
    Multi(Handle),
}

/// The selection handle under `screen`, if any.
fn handle_at(app: &ScrawlApp, screen: Pos2, rect: ERect) -> Option<HandleHit> {
    let page = app.page();
    let to_screen = screen_transform(app, rect);
    let near = |p: Point| to_pos(p).distance(screen) <= HANDLE_GRAB;
    if app.selection.len() > 1 {
        let ids = ops::normalize_selection(page, &app.selection);
        if ids.iter().any(|&id| page.find(id).is_none_or(|l| l.locked)) {
            return None;
        }
        let b = page.selection_bounds(&ids)?;
        return visible_handles(to_screen, b).find(|&h| near(to_screen * h.position(b))).map(HandleHit::Multi);
    }
    let [id] = app.selection[..] else { return None };
    let layer = page.find(id)?;
    if layer.locked {
        return None;
    }
    if let Some(ends) = line_endpoints(layer) {
        let t = to_screen * page.parent_transform(id)?;
        return ends.iter().any(|&e| near(t * e)).then_some(HandleHit::LineEnd(id));
    }
    let t = to_screen * page.world_transform(id)?;
    let local = layer.frame.local_rect();
    visible_handles(t, local).find(|&h| near(t * h.position(local))).map(|h| HandleHit::Single(id, h))
}

/// Edge handles are hidden when the layer is too small on screen to fit them.
fn visible_handles(t: Affine, local: Rect) -> impl Iterator<Item = Handle> {
    let w = (t * Point::new(local.x1, local.y0) - t * local.origin()).hypot();
    let h = (t * Point::new(local.x0, local.y1) - t * local.origin()).hypot();
    Handle::ALL.into_iter().filter(move |hd| !hd.is_edge() || w.min(h) > 4.0 * HANDLE_SIZE as f64)
}

/// Page space → absolute screen points.
pub(crate) fn screen_transform(app: &ScrawlApp, rect: ERect) -> Affine {
    Affine::translate((rect.min.x as f64, rect.min.y as f64)) * app.viewport().transform()
}

pub(crate) fn to_pos(p: Point) -> Pos2 {
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
    scrawl_render::render_page(&mut app.scene, page, view, &mut app.text);

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
    paint_polylines(p, &path, t, stroke);
}

/// Strokes `path` mapped through `t` (into screen space).
pub(crate) fn paint_polylines(p: &Painter, path: &BezPath, t: Affine, stroke: Stroke) {
    for (pts, closed) in polylines(path, t) {
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
    let handle = |center: Pos2, round: bool| {
        let r = ERect::from_center_size(center, egui::Vec2::splat(HANDLE_SIZE));
        let corner = if round { HANDLE_SIZE / 2.0 } else { 1.0 };
        p.rect(r, corner, Color32::WHITE, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    };
    let show_handles = app.canvas.edit.is_none()
        && app.canvas.pen.is_none()
        && app.canvas.text_edit.is_none()
        && !matches!(app.canvas.drag, Some(Drag::Move { .. } | Drag::Rotate { .. }));
    if app.selection.len() > 1
        && let Some(b) = page.selection_bounds(&app.selection)
    {
        let r = ERect::from_two_pos(to_pos(to_screen * b.origin()), to_pos(to_screen * Point::new(b.x1, b.y1)));
        p.rect_stroke(r, 0.0, thin, StrokeKind::Middle);
        if show_handles {
            visible_handles(to_screen, b).for_each(|h| handle(to_pos(to_screen * h.position(b)), false));
        }
    }
    if let [id] = app.selection[..]
        && let Some(layer) = page.find(id)
        && !layer.locked
        && show_handles
    {
        if let Some(ends) = line_endpoints(layer) {
            let t = to_screen * page.parent_transform(id).unwrap_or_default();
            ends.iter().for_each(|&e| handle(to_pos(t * e), true));
        } else if let Some(world) = page.world_transform(id) {
            let t = to_screen * world;
            let local = layer.frame.local_rect();
            visible_handles(t, local).for_each(|h| handle(to_pos(t * h.position(local)), false));
        }
    }

    // Smart guides while dragging; distances while holding ⌥.
    crate::snapping::paint_feedback(p, &app.canvas.feedback, to_screen);
    let alt = p.ctx().input(|i| i.modifiers.alt);
    if alt
        && app.canvas.drag.is_none()
        && app.canvas.edit.is_none()
        && app.canvas.text_edit.is_none()
        && let Some(sel) = page.selection_bounds(&app.selection)
    {
        let other = match app.canvas.hover.filter(|h| !app.selection.contains(h)) {
            Some(h) => Some(h),
            // Nothing else hovered: measure to the parent container.
            None => app.selection.first().and_then(|&id| page.parent(id)).flatten(),
        };
        if let Some(o) = other.and_then(|o| page.world_bounds(o)) {
            crate::snapping::paint_measurements(p, sel, o, to_screen);
        }
    }

    // Marquee.
    if let Some(Drag::Marquee { origin, current, .. }) = &app.canvas.drag {
        let r = ERect::from_two_pos(to_pos(to_screen * *origin), to_pos(to_screen * *current));
        p.rect(r, 0.0, ACCENT.gamma_multiply(0.08), thin, StrokeKind::Inside);
    }

    // Gradient handles.
    if let Some((_, [a, b])) = gradient_handles(app, rect) {
        p.line_segment([a, b], Stroke::new(3.0, Color32::from_black_alpha(80)));
        p.line_segment([a, b], Stroke::new(1.5, Color32::WHITE));
        p.circle(a, 5.0, Color32::WHITE, Stroke::new(1.5, ACCENT));
        p.circle(b, 5.0, ACCENT, Stroke::new(1.5, Color32::WHITE));
    }

    // Readouts while drawing, resizing or rotating.
    if let Some(Drag::Rotate { id, .. }) = &app.canvas.drag
        && let (Some(layer), Some(b)) = (page.find(*id), page.world_bounds(*id))
    {
        pill(p, to_pos(to_screen * Point::new(b.center().x, b.y1)), format!("{:.0}°", layer.rotation));
    }
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
        pill(p, to_pos(to_screen * Point::new(b.center().x, b.y1)), label);
    }
}

/// A small accent label centered below `anchor`.
fn pill(p: &Painter, anchor: Pos2, label: String) {
    let anchor = anchor + vec2(0.0, 10.0);
    let galley = p.layout_no_wrap(label, egui::FontId::proportional(11.0), Color32::WHITE);
    let bg = ERect::from_center_size(anchor + vec2(0.0, galley.size().y / 2.0), galley.size() + vec2(10.0, 4.0));
    p.rect_filled(bg, 3.0, ACCENT);
    p.galley(bg.center() - galley.size() / 2.0, galley, Color32::WHITE);
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
