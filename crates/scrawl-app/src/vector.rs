//! Vector editing: the pen tool and point-editing mode.

use egui::{Color32, CursorIcon, Painter, PointerButton, Pos2, Rect as ERect, Stroke, StrokeKind};
use scrawl_core::kurbo::{Affine, BezPath, ParamCurveNearest, Point, Rect, Vec2};
use scrawl_core::path::HandleSide;
use scrawl_core::{CurvePoint, Frame, Layer, LayerId, LayerKind, PathShape, PointMode, PointRef, ops};

use crate::app::ScrawlApp;
use crate::canvas::{Tool, paint_polylines, screen_transform, snap_angle, to_pos};
use crate::style::ACCENT;

const POINT_SIZE: f32 = 7.0;
const GRAB: f32 = 7.0;

// ---- Point editing ----------------------------------------------------------------------------

/// Editing the points of one path layer.
pub struct VectorEdit {
    pub id: LayerId,
    pub selected: Vec<PointRef>,
    /// Scissors: clicking a segment cuts it out.
    pub scissors: bool,
    drag: Option<EditDrag>,
}

enum EditDrag {
    Points {
        start: Layer,
        shape: PathShape,
        to_local: Affine,
        origin: Point,
        refs: Vec<PointRef>,
        /// The point under the pointer, which snaps, and what it snaps to.
        grabbed: PointRef,
        snap: Option<scrawl_core::snap::Targets>,
    },
    Handle { start: Layer, shape: PathShape, to_local: Affine, r: PointRef, side: HandleSide },
    Marquee { origin: Pos2, current: Pos2, base: Vec<PointRef> },
}

impl VectorEdit {
    pub fn new(id: LayerId) -> Self {
        Self { id, selected: vec![], scissors: false, drag: None }
    }
}

/// What's under the pointer in edit mode.
enum EditHit {
    Handle(PointRef, HandleSide),
    Point(PointRef),
    /// Segment `index` of a subpath, at parameter `t`.
    Segment { subpath: usize, index: usize, t: f64 },
    Nothing,
}

/// Local → absolute screen transform and the editable shape for the layer being edited.
fn edit_context(app: &ScrawlApp, rect: ERect, id: LayerId) -> Option<(Affine, PathShape)> {
    let page = app.page();
    let shape = page.find(id)?.editable_path()?;
    Some((screen_transform(app, rect) * page.world_transform(id)?, shape))
}

fn edit_hit(edit: &VectorEdit, shape: &PathShape, t: Affine, screen: Pos2) -> EditHit {
    // Handles of selected points first, then points, then segments.
    for &r in &edit.selected {
        let Some(p) = shape.point(r) else { continue };
        for side in [HandleSide::Out, HandleSide::In] {
            if let Some(h) = p.handle(side)
                && to_pos(t * h).distance(screen) <= GRAB
            {
                return EditHit::Handle(r, side);
            }
        }
    }
    if let Some(r) = shape.point_refs().find(|&r| to_pos(t * shape.point(r).unwrap().point).distance(screen) <= GRAB) {
        return EditHit::Point(r);
    }
    let local = t.inverse() * Point::new(screen.x as f64, screen.y as f64);
    let tol = GRAB as f64 / t.as_coeffs()[0].hypot(t.as_coeffs()[1]);
    for (si, sp) in shape.subpaths.iter().enumerate() {
        for i in 0..sp.segment_count() {
            let n = sp.segment(i).nearest(local, 1e-4);
            if n.distance_sq.sqrt() <= tol {
                return EditHit::Segment { subpath: si, index: i, t: n.t };
            }
        }
    }
    EditHit::Nothing
}

/// Writes an edited absolute-local shape back onto the layer, refitting its frame.
fn commit_shape(app: &mut ScrawlApp, id: LayerId, start: &Layer, shape: &PathShape) {
    let mut layer = start.clone();
    layer.set_local_path(shape);
    let page = app.page_mut();
    if let Some(l) = page.find_mut(id) {
        *l = layer;
    }
    page.refit_ancestor_groups(id);
}

fn current_layer(app: &ScrawlApp, id: LayerId) -> Option<(Layer, PathShape, Affine)> {
    let page = app.page();
    let layer = page.find(id)?.clone();
    let shape = layer.editable_path()?;
    Some((layer, shape, page.world_transform(id)?.inverse()))
}

pub fn edit_pointer(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) {
    let Some(id) = app.canvas.edit.as_ref().map(|e| e.id) else { return };
    let Some((t, shape)) = edit_context(app, rect, id) else {
        app.canvas.edit = None;
        return;
    };
    let vp = app.viewport();
    let (mods, press_origin, latest) =
        ui.input(|i| (i.modifiers, i.pointer.press_origin(), i.pointer.interact_pos().or(i.pointer.hover_pos())));

    if response.drag_started_by(PointerButton::Primary)
        && let Some(screen) = press_origin
        && let Some((layer, shape, to_local)) = current_layer(app, id)
    {
        let local = to_local * vp.to_page(rect, screen);
        let hit = edit_hit(app.canvas.edit.as_ref().expect("checked above"), &shape, t, screen);
        // Point drags snap to what's around (and the path's other points).
        let moving = |edit: &VectorEdit, r: PointRef| {
            if edit.selected.contains(&r) {
                edit.selected.clone()
            } else if mods.shift {
                [edit.selected.clone(), vec![r]].concat()
            } else {
                vec![r]
            }
        };
        let snap = match hit {
            EditHit::Point(r) if app.snap_to_layers => {
                let refs = moving(app.canvas.edit.as_ref().expect("checked above"), r);
                Some(point_targets(app, id, &shape, &refs, to_local.inverse()))
            }
            _ => None,
        };
        let edit = app.canvas.edit.as_mut().expect("checked above");
        edit.drag = match hit {
            EditHit::Handle(r, side) => Some(EditDrag::Handle { start: layer, shape, to_local, r, side }),
            EditHit::Point(r) => {
                edit.selected = moving(edit, r);
                let refs = edit.selected.clone();
                Some(EditDrag::Points { start: layer, shape, to_local, origin: local, refs, grabbed: r, snap })
            }
            EditHit::Segment { .. } | EditHit::Nothing => {
                let base = if mods.shift { edit.selected.clone() } else { vec![] };
                edit.selected = base.clone();
                Some(EditDrag::Marquee { origin: screen, current: screen, base })
            }
        };
    }

    if response.dragged()
        && let Some(pos) = latest
    {
        let p = vp.to_page(rect, pos);
        let threshold = crate::snapping::THRESHOLD / vp.zoom;
        let edit = app.canvas.edit.as_mut().expect("checked above");
        let mut write: Option<(Layer, PathShape)> = None;
        let mut feedback = scrawl_core::snap::Feedback::default();
        match &mut edit.drag {
            Some(EditDrag::Points { start, shape, to_local, origin, refs, grabbed, snap }) => {
                let mut d = *to_local * p - *origin;
                let mut axes = (true, true);
                if mods.shift {
                    if d.x.abs() > d.y.abs() {
                        d.y = 0.0;
                        axes.1 = false;
                    } else {
                        d.x = 0.0;
                        axes.0 = false;
                    }
                }
                // The grabbed point snaps (in page space) to other layers, guides and the path's
                // own points; ⌃ suspends it.
                if let (Some(t), Some(cp), false) = (snap.as_ref(), shape.point(*grabbed), mods.ctrl) {
                    let to_page = to_local.inverse();
                    let at = to_page * (cp.point + d);
                    let (snapped, fb) = scrawl_core::snap::snap_point(at, t, threshold, axes);
                    d = *to_local * snapped - cp.point;
                    feedback = fb;
                }
                let mut s = shape.clone();
                for &r in refs.iter() {
                    if let Some(cp) = s.point_mut(r) {
                        cp.translate(d);
                    }
                }
                write = Some((start.clone(), s));
            }
            Some(EditDrag::Handle { start, shape, to_local, r, side }) => {
                let mut s = shape.clone();
                if let Some(cp) = s.point_mut(*r) {
                    let mut to = *to_local * p;
                    if mods.shift {
                        to = snap_angle(cp.point, to);
                    }
                    cp.set_handle(*side, to);
                }
                write = Some((start.clone(), s));
            }
            Some(EditDrag::Marquee { origin, current, base }) => {
                *current = pos;
                let r = ERect::from_two_pos(*origin, pos);
                let mut sel = base.clone();
                for pr in shape.point_refs() {
                    if r.contains(to_pos(t * shape.point(pr).unwrap().point)) && !sel.contains(&pr) {
                        sel.push(pr);
                    }
                }
                edit.selected = sel;
            }
            None => {}
        }
        app.canvas.feedback = feedback;
        if let Some((start, s)) = write {
            commit_shape(app, id, &start, &s);
        }
    }

    if response.drag_stopped()
        && let Some(edit) = app.canvas.edit.as_mut()
    {
        edit.drag = None;
        app.canvas.feedback = Default::default();
    }

    if response.clicked_by(PointerButton::Primary)
        && let Some(screen) = response.interact_pointer_pos()
    {
        let edit = app.canvas.edit.as_mut().expect("checked above");
        match edit_hit(edit, &shape, t, screen) {
            EditHit::Point(r) | EditHit::Handle(r, _) => {
                if mods.shift {
                    if let Some(i) = edit.selected.iter().position(|&s| s == r) {
                        edit.selected.remove(i);
                    } else {
                        edit.selected.push(r);
                    }
                } else {
                    edit.selected = vec![r];
                }
            }
            EditHit::Segment { subpath, index, .. } if edit.scissors => {
                if let Some((start, mut s, _)) = current_layer(app, id) {
                    let pieces = s.subpaths[subpath].cut(index);
                    s.subpaths.splice(subpath..=subpath, pieces);
                    if let Some(edit) = app.canvas.edit.as_mut() {
                        edit.selected.clear();
                    }
                    if s.subpaths.is_empty() {
                        app.exit_vector_edit();
                        ops::delete(app.page_mut(), &[id]);
                    } else {
                        // An open path can't have a fill that reads as a closed shape; keep its
                        // style as it is.
                        commit_shape(app, id, &start, &s);
                    }
                }
            }
            EditHit::Segment { subpath, index, t: param } => {
                // Clicking a segment inserts a point there.
                if let Some((start, mut s, _)) = current_layer(app, id) {
                    let point = s.subpaths[subpath].split_segment(index, param);
                    commit_shape(app, id, &start, &s);
                    if let Some(edit) = app.canvas.edit.as_mut() {
                        edit.selected = vec![PointRef { subpath, point }];
                    }
                }
            }
            EditHit::Nothing => {
                if !mods.shift {
                    edit.selected.clear();
                }
            }
        }
    }

    if response.double_clicked_by(PointerButton::Primary)
        && let Some(screen) = response.interact_pointer_pos()
    {
        let edit = app.canvas.edit.as_ref().expect("checked above");
        match edit_hit(edit, &shape, t, screen) {
            EditHit::Point(r) => {
                // Toggle between a sharp corner and a smooth curve.
                let mode = shape.point(r).map_or(PointMode::Straight, |p| p.mode);
                let next = if mode == PointMode::Straight { PointMode::Mirrored } else { PointMode::Straight };
                if let Some(edit) = app.canvas.edit.as_mut() {
                    edit.selected = vec![r];
                }
                set_point_mode(app, next);
            }
            EditHit::Nothing => app.exit_vector_edit(),
            _ => {}
        }
    }

    // Cursor.
    if let (Some(pos), Some(edit)) = (response.hover_pos(), app.canvas.edit.as_ref())
        && edit.drag.is_none()
    {
        let icon = match edit_hit(edit, &shape, t, pos) {
            EditHit::Point(_) | EditHit::Handle(..) => CursorIcon::Move,
            EditHit::Segment { .. } if edit.scissors => CursorIcon::Crosshair,
            EditHit::Segment { .. } => CursorIcon::Copy,
            EditHit::Nothing => CursorIcon::Default,
        };
        ui.ctx().set_cursor_icon(icon);
    }
}

/// What a point drag snaps to: the usual layer and guide targets, plus the path's own points that
/// aren't moving. `to_page` maps the path's local space to page space.
fn point_targets(app: &ScrawlApp, id: LayerId, shape: &PathShape, moving: &[PointRef], to_page: Affine) -> scrawl_core::snap::Targets {
    let mut t = crate::snapping::targets(app, &[id], None);
    for r in shape.point_refs().filter(|r| !moving.contains(r)) {
        let p = to_page * shape.point(r).expect("listed").point;
        t.add_line(scrawl_core::snap::Axis::X, p.x, (p.y, p.y));
        t.add_line(scrawl_core::snap::Axis::Y, p.y, (p.x, p.x));
    }
    t
}

/// Sets the corner radius of the selected points.
pub fn set_corner_radius(app: &mut ScrawlApp, radius: f64) {
    edit_selected_points(app, |shape, r| {
        if let Some(p) = shape.point_mut(r) {
            p.corner_radius = radius.max(0.0);
        }
    });
}

/// Applies `f` to the selected points (absolute local coordinates) and writes the result back.
fn edit_selected_points(app: &mut ScrawlApp, f: impl Fn(&mut PathShape, PointRef)) {
    let Some(edit) = app.canvas.edit.as_ref() else { return };
    let (id, refs) = (edit.id, edit.selected.clone());
    let Some((start, mut shape, _)) = current_layer(app, id) else { return };
    for r in refs {
        f(&mut shape, r);
    }
    commit_shape(app, id, &start, &shape);
}

pub fn set_point_mode(app: &mut ScrawlApp, mode: PointMode) {
    edit_selected_points(app, |shape, r| {
        let Some(sp) = shape.subpaths.get(r.subpath) else { return };
        let (prev, next) = sp.neighbors(r.point);
        if let Some(p) = shape.point_mut(r) {
            p.set_mode(mode, prev, next);
        }
    });
}

pub fn nudge_points(app: &mut ScrawlApp, d: Vec2) {
    let to_local = app.canvas.edit.as_ref().and_then(|e| app.page().world_transform(e.id)).map(|t| t.inverse());
    let Some(inv) = to_local else { return };
    let local_d = inv * Point::new(d.x, d.y) - inv * Point::ORIGIN;
    edit_selected_points(app, |shape, r| {
        if let Some(p) = shape.point_mut(r) {
            p.translate(local_d);
        }
    });
}

pub fn select_all_points(app: &mut ScrawlApp) {
    let Some(id) = app.canvas.edit.as_ref().map(|e| e.id) else { return };
    let refs: Vec<_> = app.page().find(id).and_then(Layer::editable_path).map(|s| s.point_refs().collect()).unwrap_or_default();
    if let Some(edit) = app.canvas.edit.as_mut() {
        edit.selected = refs;
    }
}

/// Deletes the selected points. Subpaths left with fewer than two points are removed, and the
/// layer itself if nothing is left.
pub fn delete_points(app: &mut ScrawlApp) {
    let Some(edit) = app.canvas.edit.as_mut() else { return };
    let (id, mut refs) = (edit.id, std::mem::take(&mut edit.selected));
    let Some((start, mut shape, _)) = current_layer(app, id) else { return };
    refs.sort();
    for r in refs.iter().rev() {
        if let Some(sp) = shape.subpaths.get_mut(r.subpath)
            && r.point < sp.points.len()
        {
            sp.points.remove(r.point);
        }
    }
    shape.subpaths.retain(|sp| sp.points.len() >= 2);
    for sp in &mut shape.subpaths {
        if sp.points.len() < 3 {
            sp.closed = false;
        }
    }
    if shape.subpaths.is_empty() {
        app.exit_vector_edit();
        ops::delete(app.page_mut(), &[id]);
    } else {
        commit_shape(app, id, &start, &shape);
    }
}

pub fn paint_edit(app: &ScrawlApp, p: &Painter, rect: ERect) {
    let Some(edit) = app.canvas.edit.as_ref() else { return };
    let Some((t, shape)) = edit_context(app, rect, edit.id) else { return };
    paint_polylines(p, &shape.to_absolute_bez_path(), t, Stroke::new(1.0, ACCENT));

    // Handles of selected points.
    let handle_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.8));
    for &r in &edit.selected {
        let Some(cp) = shape.point(r) else { continue };
        let at = to_pos(t * cp.point);
        for h in [cp.curve_to, cp.curve_from].into_iter().flatten() {
            let hp = to_pos(t * h);
            p.line_segment([at, hp], handle_stroke);
            p.circle(hp, 3.5, Color32::WHITE, Stroke::new(1.0, ACCENT));
        }
    }
    for r in shape.point_refs() {
        let cp = shape.point(r).unwrap();
        let selected = edit.selected.contains(&r);
        let pr = ERect::from_center_size(to_pos(t * cp.point), egui::Vec2::splat(POINT_SIZE));
        let fill = if selected { ACCENT } else { Color32::WHITE };
        let rounding = if cp.mode == PointMode::Straight { 1.0 } else { POINT_SIZE / 2.0 };
        p.rect(pr, rounding, fill, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    }

    if let Some(EditDrag::Marquee { origin, current, .. }) = &edit.drag {
        p.rect(ERect::from_two_pos(*origin, *current), 0.0, ACCENT.gamma_multiply(0.08), Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    }
}

// ---- Pen tool ---------------------------------------------------------------------------------

/// A path being drawn with the pen. Points are kept in the parent's space.
pub struct PenState {
    id: LayerId,
    parent_to_page: Affine,
    points: Vec<CurvePoint>,
    /// Whether the last point is being dragged out into a curve.
    dragging: bool,
}

pub fn pen_pointer(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) {
    let vp = app.viewport();
    let (mods, press_origin, latest) =
        ui.input(|i| (i.modifiers, i.pointer.press_origin(), i.pointer.interact_pos().or(i.pointer.hover_pos())));
    ui.ctx().set_cursor_icon(if near_first_point(app, latest, rect) { CursorIcon::Cell } else { CursorIcon::Crosshair });

    // A press places a point; drag starts report where the button went down, clicks where it
    // was released (egui clears the press origin on release).
    let pressed_at = if response.drag_started_by(PointerButton::Primary) {
        press_origin
    } else if response.clicked_by(PointerButton::Primary) {
        response.interact_pointer_pos()
    } else {
        None
    };
    if let Some(screen) = pressed_at {
        if near_first_point(app, Some(screen), rect) {
            finish_pen(app, true);
            return;
        }
        let mut p = vp.to_page(rect, screen);
        p = Point::new(p.x.round(), p.y.round());
        if mods.shift
            && let Some(last) = last_point_page(app)
        {
            p = snap_angle(last, p);
        }
        add_pen_point(app, p);
        if let Some(pen) = app.canvas.pen.as_mut() {
            pen.dragging = response.drag_started_by(PointerButton::Primary);
        }
    }

    if response.dragged()
        && let Some(pos) = latest
        && let Some(pen) = app.canvas.pen.as_mut()
        && pen.dragging
    {
        // Dragging after placing a point pulls out mirrored curve handles.
        let mut out = pen.parent_to_page.inverse() * vp.to_page(rect, pos);
        if let Some(last) = pen.points.last_mut() {
            if mods.shift {
                out = snap_angle(last.point, out);
            }
            *last = if (out - last.point).hypot() > 0.5 { CurvePoint::smooth(last.point, out) } else { CurvePoint::corner(last.point) };
        }
        rebuild_pen_layer(app, false);
    }
    if response.drag_stopped()
        && let Some(pen) = app.canvas.pen.as_mut()
    {
        pen.dragging = false;
    }
    if response.double_clicked_by(PointerButton::Primary) {
        finish_pen(app, false);
    }
}

fn last_point_page(app: &ScrawlApp) -> Option<Point> {
    let pen = app.canvas.pen.as_ref()?;
    Some(pen.parent_to_page * pen.points.last()?.point)
}

fn near_first_point(app: &ScrawlApp, screen: Option<Pos2>, rect: ERect) -> bool {
    let (Some(pen), Some(screen)) = (app.canvas.pen.as_ref(), screen) else { return false };
    if pen.points.len() < 2 {
        return false;
    }
    let t = screen_transform(app, rect) * pen.parent_to_page;
    to_pos(t * pen.points[0].point).distance(screen) <= GRAB + 2.0
}

fn add_pen_point(app: &mut ScrawlApp, p: Point) {
    if app.canvas.pen.is_none() {
        let mut layer = Layer::path(&PathShape::single(vec![CurvePoint::corner(p)], false));
        layer.frame = Frame::new(p.x, p.y, 0.0, 0.0);
        let page = app.page_mut();
        let id = ops::insert_drawn(page, layer);
        let parent_to_page = page.parent_transform(id).unwrap_or_default();
        app.selection = vec![id];
        app.canvas.pen = Some(PenState { id, parent_to_page, points: vec![], dragging: false });
    }
    let pen = app.canvas.pen.as_mut().expect("created above");
    pen.points.push(CurvePoint::corner(pen.parent_to_page.inverse() * p));
    rebuild_pen_layer(app, false);
}

fn rebuild_pen_layer(app: &mut ScrawlApp, closed: bool) {
    let Some(pen) = app.canvas.pen.as_ref() else { return };
    let (id, shape) = (pen.id, PathShape::single(pen.points.clone(), closed));
    let (bounds, norm) = shape.fit();
    if let Some(l) = app.page_mut().find_mut(id) {
        l.frame = Frame::from_rect(bounds);
        l.kind = LayerKind::Path(norm);
    }
}

/// Ends the current pen path, closing it if requested. Paths with fewer than two points are
/// discarded.
pub fn finish_pen(app: &mut ScrawlApp, close: bool) {
    let Some(pen) = app.canvas.pen.as_ref() else { return };
    let (id, n) = (pen.id, pen.points.len());
    if n < 2 {
        ops::delete(app.page_mut(), &[id]);
        app.selection.clear();
    } else {
        rebuild_pen_layer(app, close && n >= 3);
        app.selection = vec![id];
    }
    app.canvas.pen = None;
    app.tool = Tool::Select;
}

pub fn pen_active(app: &ScrawlApp) -> bool {
    app.canvas.pen.is_some()
}

pub fn paint_pen(app: &ScrawlApp, p: &Painter, rect: ERect, hover: Option<Pos2>) {
    let Some(pen) = app.canvas.pen.as_ref() else { return };
    let t = screen_transform(app, rect) * pen.parent_to_page;
    // Rubber band from the last point to the pointer.
    if let (Some(last), Some(h)) = (pen.points.last(), hover)
        && !pen.dragging
    {
        let mut band = BezPath::new();
        band.move_to(last.point);
        let target = t.inverse() * Point::new(h.x as f64, h.y as f64);
        match last.curve_from {
            Some(c) => band.curve_to(c, target, target),
            None => band.line_to(target),
        }
        paint_polylines(p, &band, t, Stroke::new(1.0, ACCENT.gamma_multiply(0.7)));
    }
    if let Some(last) = pen.points.last() {
        let at = to_pos(t * last.point);
        for h in [last.curve_to, last.curve_from].into_iter().flatten() {
            p.line_segment([at, to_pos(t * h)], Stroke::new(1.0, ACCENT));
            p.circle(to_pos(t * h), 3.5, Color32::WHITE, Stroke::new(1.0, ACCENT));
        }
    }
    for (i, cp) in pen.points.iter().enumerate() {
        let r = ERect::from_center_size(to_pos(t * cp.point), egui::Vec2::splat(POINT_SIZE));
        let closing = i == 0 && near_first_point(app, hover, rect);
        let fill = if closing { ACCENT } else { Color32::WHITE };
        p.rect(r, 1.0, fill, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    }
}

/// Bounds of the selected points in page space, for the inspector.
pub fn selected_points_page(app: &ScrawlApp) -> Option<Rect> {
    let edit = app.canvas.edit.as_ref()?;
    let page = app.page();
    let shape = page.find(edit.id)?.editable_path()?;
    let t = page.world_transform(edit.id)?;
    edit.selected
        .iter()
        .filter_map(|&r| shape.point(r))
        .map(|cp| t * cp.point)
        .fold(None, |acc: Option<Rect>, p| Some(acc.map_or(Rect::from_points(p, p), |r| r.union_pt(p))))
}

/// Moves the selected points so their bounds start at `to` (page space).
pub fn move_selected_points_to(app: &mut ScrawlApp, to: Point) {
    if let Some(b) = selected_points_page(app) {
        nudge_points(app, to - b.origin());
    }
}
