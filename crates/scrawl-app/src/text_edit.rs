//! Editing text layers in place on the canvas.
//!
//! The caret and selection are parley [`Selection`]s over the same layout the canvas renders, so
//! they line up with rich text (style runs of different sizes and fonts). Edits go straight into
//! the layer's text, moving its style runs along. While a layer is being edited the text engine
//! lays it out without its case transform, so byte offsets match what's drawn.

use std::ops::Range;

use egui::{Event, Key, Painter, PointerButton, Pos2, Rect as ERect, Stroke};
use scrawl_core::kurbo::{Affine, Point};
use scrawl_core::{Layer, LayerId, LayerKind, TextLayer, ops};
use scrawl_text::Layout;
use scrawl_text::parley::editing::{Cursor, Selection};
use scrawl_text::parley::layout::Affinity;

use crate::app::ScrawlApp;
use crate::canvas::{screen_transform, to_pos};
use crate::style::ACCENT;

pub struct TextEditing {
    pub id: LayerId,
    selection: Selection,
    /// The input method's in-progress text, as a byte range of the layer's text.
    compose: Option<Range<usize>>,
}

fn text_of(layer: &Layer) -> Option<&TextLayer> {
    match &layer.kind {
        LayerKind::Text(t) => Some(t),
        _ => None,
    }
}

/// Runs `f` with the layout of text layer `id`, as rendered.
fn with_layout<R>(app: &mut ScrawlApp, id: LayerId, f: impl FnOnce(&Layout) -> R) -> Option<R> {
    let layer = app.doc.page(app.page)?.find(id)?.clone();
    let (layout, _) = app.text.layer_layout(&layer)?;
    Some(f(layout))
}

fn caret_at(layout: &Layout, index: usize) -> Selection {
    Selection::from_byte_index(layout, index, Affinity::Downstream)
}

/// Starts editing a text layer. With `select_all`, the existing text is selected so typing
/// replaces it.
pub fn begin(app: &mut ScrawlApp, id: LayerId, select_all: bool) {
    let Some(layer) = app.page().find(id).cloned() else { return };
    let Some(text) = text_of(&layer) else { return };
    if layer.locked {
        return;
    }
    let len = text.text.len();
    app.canvas.cancel();
    app.canvas.edit = None;
    app.selection = vec![id];
    app.tool = crate::canvas::Tool::Select;
    app.text.raw_case = Some(id);
    let selection = with_layout(app, id, |l| {
        if select_all {
            caret_at(l, 0).extend(Cursor::from_byte_index(l, len, Affinity::Upstream))
        } else {
            caret_at(l, len)
        }
    });
    let Some(selection) = selection else { return };
    app.canvas.text_edit = Some(TextEditing { id, selection, compose: None });
}

/// Stops editing. Empty text layers are deleted.
pub fn end(app: &mut ScrawlApp) {
    let Some(editing) = app.canvas.text_edit.take() else { return };
    app.text.raw_case = None;
    let empty = app.page().find(editing.id).and_then(text_of).is_some_and(|t| t.text.trim().is_empty());
    if empty {
        ops::delete(app.page_mut(), &[editing.id]);
        app.selection.clear();
    }
}

/// The selected bytes of the layer being edited, if any are selected.
pub fn selected_range(app: &ScrawlApp) -> Option<Range<usize>> {
    let r = app.canvas.text_edit.as_ref()?.selection.text_range();
    (!r.is_empty()).then_some(r)
}

/// Replaces `range` of the edited text with `s` and puts the caret after it. Renames the layer
/// if it still has its automatic name. Returns the end of the inserted text.
fn replace(app: &mut ScrawlApp, range: Range<usize>, s: &str) -> usize {
    let Some(id) = app.canvas.text_edit.as_ref().map(|e| e.id) else { return range.start };
    let at = range.start + s.len();
    if let Some(layer) = app.page_mut().find_mut(id)
        && let LayerKind::Text(t) = &mut layer.kind
    {
        let auto_name = |s: &str| {
            let n: String = s.lines().next().unwrap_or("").chars().take(40).collect();
            if n.trim().is_empty() { "Text".to_owned() } else { n }
        };
        let renamed = layer.name == auto_name(&t.text);
        t.replace_range(range, s);
        if renamed {
            layer.name = auto_name(&t.text);
        }
    }
    if let Some(sel) = with_layout(app, id, |l| caret_at(l, at))
        && let Some(e) = app.canvas.text_edit.as_mut()
    {
        e.selection = sel;
    }
    at
}

/// Moves or extends the selection with `f`, given the layout.
fn navigate(app: &mut ScrawlApp, f: impl FnOnce(&Selection, &Layout) -> Selection) {
    let Some(e) = app.canvas.text_edit.as_ref() else { return };
    let (id, sel) = (e.id, e.selection);
    if let Some(new) = with_layout(app, id, |l| f(&sel, l))
        && let Some(e) = app.canvas.text_edit.as_mut()
    {
        e.selection = new;
    }
}

/// Deletes the selection, or else the text from the caret to where `extend` moves it.
fn delete(app: &mut ScrawlApp, extend: impl FnOnce(&Selection, &Layout) -> Selection) {
    let Some(e) = app.canvas.text_edit.as_ref() else { return };
    let (id, sel) = (e.id, e.selection);
    let range = if sel.is_collapsed() { with_layout(app, id, |l| extend(&sel, l).text_range()) } else { Some(sel.text_range()) };
    if let Some(r) = range.filter(|r| !r.is_empty()) {
        replace(app, r, "");
    }
}

/// Toggles a character style on the selected text, as ⌘B, ⌘I and ⌘U do. Where the result
/// matches the layer's own style, the override is dropped.
fn toggle(app: &mut ScrawlApp, get: fn(&scrawl_core::TextStyle) -> bool, set: fn(&mut scrawl_core::text::RunStyle, Option<bool>)) {
    let (Some(range), Some(id)) = (selected_range(app), app.canvas.text_edit.as_ref().map(|e| e.id)) else { return };
    if let Some(LayerKind::Text(t)) = app.page_mut().find_mut(id).map(|l| &mut l.kind) {
        let on = !get(&t.style_at(range.start).0);
        let value = (on != get(&t.style)).then_some(on);
        t.set_run_style(range, |s| set(s, value));
    }
}

/// Handles keyboard, clipboard and IME input while editing. Escape ends editing.
pub fn handle_keys(app: &mut ScrawlApp, ctx: &egui::Context) {
    let Some(id) = app.canvas.text_edit.as_ref().map(|e| e.id) else { return };
    if app.page().find(id).is_none() {
        end(app);
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let mut finish = false;
    let selected = |app: &ScrawlApp| app.canvas.text_edit.as_ref().map_or(0..0, |e| e.selection.text_range());
    for event in &events {
        match event {
            Event::Text(s) => {
                let r = selected(app);
                replace(app, r, s);
            }
            Event::Paste(s) => {
                let r = selected(app);
                replace(app, r, &s.replace("\r\n", "\n"));
            }
            Event::Copy | Event::Cut => {
                let r = selected(app);
                let text = app.page().find(id).and_then(text_of).map(|t| t.text[r.clone()].to_owned());
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    ctx.copy_text(text);
                    if matches!(event, Event::Cut) {
                        replace(app, r, "");
                    }
                }
            }
            Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                let r = app.canvas.text_edit.as_ref().and_then(|e| e.compose.clone()).unwrap_or_else(|| selected(app));
                let start = r.start;
                let end = replace(app, r, text);
                if let Some(e) = app.canvas.text_edit.as_mut() {
                    e.compose = (!text.is_empty()).then_some(start..end);
                }
            }
            Event::Ime(egui::ImeEvent::Commit(s)) => {
                let r = app.canvas.text_edit.as_mut().and_then(|e| e.compose.take()).unwrap_or_else(|| selected(app));
                replace(app, r, s);
            }
            Event::Key { key, pressed: true, modifiers: m, .. } => {
                // macOS conventions: ⌥ moves by word, ⌘ by line.
                let line = m.mac_cmd || (cfg!(target_os = "macos") && m.command);
                let (word, shift) = (m.alt || (m.ctrl && !line), m.shift);
                match key {
                    Key::Escape => finish = true,
                    Key::Enter => {
                        let r = selected(app);
                        replace(app, r, "\n");
                    }
                    Key::Tab => {
                        let r = selected(app);
                        replace(app, r, "\t");
                    }
                    Key::B if m.command => toggle(app, |s| s.font_weight >= 600, |r, on| r.font_weight = on.map(|b| if b { 700 } else { 400 })),
                    Key::I if m.command => toggle(app, |s| s.italic, |r, on| r.italic = on),
                    Key::U if m.command => toggle(app, |s| s.underline, |r, on| r.underline = on),
                    Key::Backspace if line => delete(app, |s, l| s.line_start(l, true)),
                    Key::Backspace if word => delete(app, |s, l| s.previous_visual_word(l, true)),
                    Key::Backspace => delete(app, |s, l| s.previous_visual(l, true)),
                    Key::Delete if word => delete(app, |s, l| s.next_visual_word(l, true)),
                    Key::Delete => delete(app, |s, l| s.next_visual(l, true)),
                    Key::A if m.command => {
                        let len = app.page().find(id).and_then(text_of).map_or(0, |t| t.text.len());
                        navigate(app, |_, l| caret_at(l, 0).extend(Cursor::from_byte_index(l, len, Affinity::Upstream)));
                    }
                    Key::ArrowLeft => navigate(app, |s, l| match (word, line) {
                        (_, true) => s.line_start(l, shift),
                        (true, _) => s.previous_visual_word(l, shift),
                        _ if !shift && !s.is_collapsed() => caret_at(l, s.text_range().start),
                        _ => s.previous_visual(l, shift),
                    }),
                    Key::ArrowRight => navigate(app, |s, l| match (word, line) {
                        (_, true) => s.line_end(l, shift),
                        (true, _) => s.next_visual_word(l, shift),
                        _ if !shift && !s.is_collapsed() => caret_at(l, s.text_range().end),
                        _ => s.next_visual(l, shift),
                    }),
                    Key::ArrowUp => navigate(app, |s, l| match (line, shift) {
                        (true, true) => s.extend(Cursor::from_byte_index(l, 0, Affinity::Downstream)),
                        (true, false) => caret_at(l, 0),
                        _ => s.previous_line(l, shift),
                    }),
                    Key::ArrowDown => {
                        let len = app.page().find(id).and_then(text_of).map_or(0, |t| t.text.len());
                        navigate(app, |s, l| match (line, shift) {
                            (true, true) => s.extend(Cursor::from_byte_index(l, len, Affinity::Upstream)),
                            (true, false) => caret_at(l, len),
                            _ => s.next_line(l, shift),
                        })
                    }
                    Key::Home => navigate(app, |s, l| s.line_start(l, shift)),
                    Key::End => navigate(app, |s, l| s.line_end(l, shift)),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if finish {
        end(app);
    }
}

/// Maps the layout's space to screen space.
fn layout_to_screen(app: &mut ScrawlApp, id: LayerId, rect: ERect) -> Option<Affine> {
    let page = app.doc.page(app.page)?;
    let layer = page.find(id)?.clone();
    let world = page.world_transform(id)?;
    let (_, dy) = app.text.layer_layout(&layer)?;
    Some(screen_transform(app, rect) * world * Affine::translate((0.0, dy)))
}

/// Pointer handling while editing: click to place the caret, drag to select, double-click to
/// select a word, click outside the layer to stop editing. Returns whether the pointer was used.
pub fn pointer(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) -> bool {
    let Some(id) = app.canvas.text_edit.as_ref().map(|e| e.id) else { return false };
    let Some(t) = layout_to_screen(app, id, rect) else { return false };
    let inv = t.inverse();
    let local = |p: Pos2| {
        let q = inv * Point::new(p.x as f64, p.y as f64);
        (q.x as f32, q.y as f32)
    };
    let (press, latest, mods) = ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos(), i.modifiers));
    let frame = app.page().find(id).map(|l| l.frame.local_rect().inflate(4.0, 4.0));
    let inside = |p: Pos2| {
        let world = app.page().world_transform(id).unwrap_or_default();
        let q = (screen_transform(app, rect) * world).inverse() * Point::new(p.x as f64, p.y as f64);
        frame.is_some_and(|f| f.contains(q))
    };

    let started = response.drag_started_by(PointerButton::Primary).then_some(press).flatten();
    let clicked = response.clicked_by(PointerButton::Primary).then(|| response.interact_pointer_pos()).flatten();
    if let Some(p) = started.or(clicked)
        && !inside(p)
    {
        // Clicking elsewhere ends editing; the click itself is then handled normally.
        end(app);
        return false;
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    if let Some(p) = started.or(clicked) {
        let (x, y) = local(p);
        navigate(app, |s, l| if mods.shift { s.shift_click_extension(l, x, y) } else { Selection::from_point(l, x, y) });
    }
    if response.dragged_by(PointerButton::Primary)
        && let Some(p) = latest
    {
        let (x, y) = local(p);
        navigate(app, |s, l| s.extend_to_point(l, x, y));
    }
    if response.double_clicked_by(PointerButton::Primary)
        && let Some(p) = response.interact_pointer_pos()
    {
        let (x, y) = local(p);
        navigate(app, |_, l| Selection::word_from_point(l, x, y));
    }
    true
}

/// Paints the selection and caret, and tells the platform where the IME should appear.
pub fn paint(app: &mut ScrawlApp, p: &Painter, rect: ERect) {
    let Some(id) = app.canvas.text_edit.as_ref().map(|e| e.id) else { return };
    let Some(layer) = app.page().find(id).cloned() else { return };
    let Some(t) = layout_to_screen(app, id, rect) else { return };
    // Restyling from the inspector moves things; keep the selection on the same bytes.
    navigate(app, |s, l| s.refresh(l));
    let Some(sel) = app.canvas.text_edit.as_ref().map(|e| e.selection) else { return };
    let Some((boxes, caret, line_box)) = with_layout(app, id, |l| (sel.geometry(l), sel.focus().geometry(l, 1.5), sel.focus().geometry(l, 1.0))) else {
        return;
    };

    let quad = |x0: f64, y0: f64, x1: f64, y1: f64| -> Vec<Pos2> {
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|&(x, y)| to_pos(t * Point::new(x, y))).collect()
    };
    for (bb, _) in boxes {
        p.add(egui::Shape::convex_polygon(quad(bb.x0, bb.y0, bb.x1, bb.y1), ACCENT.gamma_multiply(0.3), Stroke::NONE));
    }
    if sel.is_collapsed() {
        let bb = caret;
        p.add(egui::Shape::convex_polygon(quad(bb.x0, bb.y0, bb.x1, bb.y1), ACCENT, Stroke::NONE));
    }
    let caret_rect = ERect::from_points(&quad(line_box.x0, line_box.y0, line_box.x1, line_box.y1));
    p.ctx().output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect: caret_rect,
            cursor_rect: caret_rect,
            should_interrupt_composition: false,
        })
    });
    // Underline the input method's in-progress text.
    if let Some(r) = app.canvas.text_edit.as_ref().and_then(|e| e.compose.clone())
        && let Some(boxes) = with_layout(app, id, |l| caret_at(l, r.start).extend(Cursor::from_byte_index(l, r.end, Affinity::Upstream)).geometry(l))
    {
        for (bb, _) in boxes {
            let (a, b) = (to_pos(t * Point::new(bb.x0, bb.y1)), to_pos(t * Point::new(bb.x1, bb.y1)));
            p.line_segment([a, b], Stroke::new(1.5, ACCENT));
        }
    }
    // The text box outline.
    let world = app.page().world_transform(id).unwrap_or_default();
    let box_t = screen_transform(app, rect) * world;
    let f = layer.frame.local_rect();
    let pts: Vec<Pos2> = [(f.x0, f.y0), (f.x1, f.y0), (f.x1, f.y1), (f.x0, f.y1)].iter().map(|&(x, y)| to_pos(box_t * Point::new(x, y))).collect();
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.0, ACCENT.gamma_multiply(0.6))));
}
