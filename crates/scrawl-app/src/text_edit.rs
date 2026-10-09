//! Editing text layers in place on the canvas, using parley's `PlainEditor`.
//!
//! The editor keeps its own copy of the text and layout (for caret and selection geometry);
//! every change is written back to the layer, which the canvas renders as usual. While a layer is
//! being edited the text engine lays it out without its case transform, so what's drawn matches
//! the editor's layout exactly.

use egui::{Event, Key, Painter, PointerButton, Pos2, Rect as ERect, Stroke};
use scrawl_core::kurbo::{Affine, Point};
use scrawl_core::{Layer, LayerId, LayerKind, TextLayer, ops};
use scrawl_text::parley::PlainEditor;
use scrawl_text::{TextEngine, alignment, fill_style_set, wrap_width};

use crate::app::ScrawlApp;
use crate::canvas::{screen_transform, to_pos};
use crate::style::ACCENT;

pub struct TextEditing {
    pub id: LayerId,
    editor: PlainEditor<()>,
    /// The style and wrap width last applied to the editor, to resync on change.
    synced: Option<(String, Option<f32>)>,
}

fn text_of(layer: &Layer) -> Option<&TextLayer> {
    match &layer.kind {
        LayerKind::Text(t) => Some(t),
        _ => None,
    }
}

/// Starts editing a text layer. With `select_all`, the existing text is selected so typing
/// replaces it.
pub fn begin(app: &mut ScrawlApp, id: LayerId, select_all: bool) {
    let Some(layer) = app.page().find(id).cloned() else { return };
    let Some(text) = text_of(&layer) else { return };
    if layer.locked {
        return;
    }
    let mut editor = PlainEditor::new(text.style.font_size as f32);
    editor.set_text(&text.text);
    app.canvas.cancel();
    app.canvas.edit = None;
    app.selection = vec![id];
    app.tool = crate::canvas::Tool::Select;
    app.text.raw_case = Some(id);
    let mut editing = TextEditing { id, editor, synced: None };
    sync(&mut editing, &layer, &mut app.text);
    {
        let mut d = editing.editor.driver(&mut app.text.font_cx, &mut app.text.layout_cx);
        if select_all { d.select_all() } else { d.move_to_text_end() }
    }
    app.canvas.text_edit = Some(editing);
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

/// Re-applies the layer's style and wrap width to the editor if they changed.
fn sync(editing: &mut TextEditing, layer: &Layer, engine: &mut TextEngine) {
    let Some(text) = text_of(layer) else { return };
    let width = wrap_width(text, layer.frame.width);
    let key = (format!("{:?}", text.style), width);
    if editing.synced.as_ref() == Some(&key) {
        return;
    }
    let styles = editing.editor.edit_styles();
    fill_style_set(styles, &text.style);
    editing.editor.set_width(width);
    editing.editor.set_alignment(alignment(text.style.alignment));
    editing.editor.refresh_layout(&mut engine.font_cx, &mut engine.layout_cx);
    editing.synced = Some(key);
}

/// Writes the editor's text back to the layer, renaming it if it still has its automatic name.
fn write_back(app: &mut ScrawlApp) {
    let Some(editing) = app.canvas.text_edit.as_ref() else { return };
    let (id, new_text) = (editing.id, editing.editor.raw_text().to_owned());
    let Some(layer) = app.page_mut().find_mut(id) else { return };
    let LayerKind::Text(t) = &mut layer.kind else { return };
    if t.text == new_text {
        return;
    }
    let auto_name = |s: &str| {
        let n: String = s.lines().next().unwrap_or("").chars().take(40).collect();
        if n.trim().is_empty() { "Text".to_owned() } else { n }
    };
    if layer.name == auto_name(&t.text) {
        layer.name = auto_name(&new_text);
    }
    t.text = new_text;
}

/// Handles keyboard, clipboard and IME input while editing. Escape ends editing.
pub fn handle_keys(app: &mut ScrawlApp, ctx: &egui::Context) {
    let Some(id) = app.canvas.text_edit.as_ref().map(|e| e.id) else { return };
    let Some(layer) = app.page().find(id).cloned() else {
        end(app);
        return;
    };
    let events = ctx.input(|i| i.events.clone());
    let mut finish = false;
    {
        let editing = app.canvas.text_edit.as_mut().expect("checked above");
        sync(editing, &layer, &mut app.text);
        let mut d = editing.editor.driver(&mut app.text.font_cx, &mut app.text.layout_cx);
        for event in &events {
            match event {
                Event::Text(s) => d.insert_or_replace_selection(s),
                Event::Paste(s) => d.insert_or_replace_selection(&s.replace("\r\n", "\n")),
                Event::Copy | Event::Cut => {
                    if let Some(sel) = d.editor.selected_text() {
                        ctx.copy_text(sel.to_owned());
                        if matches!(event, Event::Cut) {
                            d.delete_selection();
                        }
                    }
                }
                Event::Ime(egui::ImeEvent::Preedit { text, .. }) if text.is_empty() => d.clear_compose(),
                Event::Ime(egui::ImeEvent::Preedit { text, .. }) => d.set_compose(text, None),
                Event::Ime(egui::ImeEvent::Commit(s)) => {
                    d.clear_compose();
                    d.insert_or_replace_selection(s);
                }
                Event::Key { key, pressed: true, modifiers: m, .. } => {
                    // macOS conventions: ⌥ moves by word, ⌘ by line.
                    let (word, line, shift) = (m.alt || (m.ctrl && !m.mac_cmd), m.mac_cmd, m.shift);
                    match key {
                        Key::Escape => finish = true,
                        Key::Enter => d.insert_or_replace_selection("\n"),
                        Key::Tab => d.insert_or_replace_selection("\t"),
                        Key::Backspace if line => {
                            d.select_to_line_start();
                            d.delete_selection();
                        }
                        Key::Backspace if word => d.backdelete_word(),
                        Key::Backspace => d.backdelete(),
                        Key::Delete if word => d.delete_word(),
                        Key::Delete => d.delete(),
                        Key::A if m.command => d.select_all(),
                        Key::ArrowLeft => match (shift, word, line) {
                            (false, _, true) => d.move_to_line_start(),
                            (true, _, true) => d.select_to_line_start(),
                            (false, true, _) => d.move_word_left(),
                            (true, true, _) => d.select_word_left(),
                            (false, ..) => d.move_left(),
                            (true, ..) => d.select_left(),
                        },
                        Key::ArrowRight => match (shift, word, line) {
                            (false, _, true) => d.move_to_line_end(),
                            (true, _, true) => d.select_to_line_end(),
                            (false, true, _) => d.move_word_right(),
                            (true, true, _) => d.select_word_right(),
                            (false, ..) => d.move_right(),
                            (true, ..) => d.select_right(),
                        },
                        Key::ArrowUp => match (shift, line) {
                            (false, true) => d.move_to_text_start(),
                            (true, true) => d.select_to_text_start(),
                            (false, false) => d.move_up(),
                            (true, false) => d.select_up(),
                        },
                        Key::ArrowDown => match (shift, line) {
                            (false, true) => d.move_to_text_end(),
                            (true, true) => d.select_to_text_end(),
                            (false, false) => d.move_down(),
                            (true, false) => d.select_down(),
                        },
                        Key::Home => if shift { d.select_to_line_start() } else { d.move_to_line_start() },
                        Key::End => if shift { d.select_to_line_end() } else { d.move_to_line_end() },
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    write_back(app);
    if finish {
        end(app);
    }
}

/// Maps the editor's layout space to screen space.
fn layout_to_screen(app: &mut ScrawlApp, id: LayerId, rect: ERect) -> Option<Affine> {
    let page = app.doc.page(app.page)?;
    let layer = page.find(id)?;
    let world = page.world_transform(id)?;
    let (_, dy) = app.text.layer_layout(layer)?;
    Some(screen_transform(app, rect) * world * Affine::translate((0.0, dy)))
}

/// Pointer handling while editing: click to place the caret, drag to select, double-click to
/// select a word, click outside the layer to stop editing. Returns whether the pointer was used.
pub fn pointer(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) -> bool {
    let Some(id) = app.canvas.text_edit.as_ref().map(|e| e.id) else { return false };
    let Some(t) = layout_to_screen(app, id, rect) else { return false };
    let inv = t.inverse();
    let local = |p: Pos2| inv * Point::new(p.x as f64, p.y as f64);
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

    let editing = app.canvas.text_edit.as_mut().expect("checked above");
    let mut d = editing.editor.driver(&mut app.text.font_cx, &mut app.text.layout_cx);
    if let Some(p) = started.or(clicked) {
        let q = local(p);
        if mods.shift {
            d.shift_click_extension(q.x as f32, q.y as f32);
        } else {
            d.move_to_point(q.x as f32, q.y as f32);
        }
    }
    if response.dragged_by(PointerButton::Primary)
        && let Some(p) = latest
    {
        let q = local(p);
        d.extend_selection_to_point(q.x as f32, q.y as f32);
    }
    if response.double_clicked_by(PointerButton::Primary)
        && let Some(p) = response.interact_pointer_pos()
    {
        let q = local(p);
        d.select_word_at_point(q.x as f32, q.y as f32);
    }
    true
}

/// Paints the selection and caret, and tells the platform where the IME should appear.
pub fn paint(app: &mut ScrawlApp, p: &Painter, rect: ERect) {
    let Some(id) = app.canvas.text_edit.as_ref().map(|e| e.id) else { return };
    let Some(layer) = app.page().find(id).cloned() else { return };
    let Some(t) = layout_to_screen(app, id, rect) else { return };
    let editing = app.canvas.text_edit.as_mut().expect("checked above");
    sync(editing, &layer, &mut app.text);
    editing.editor.refresh_layout(&mut app.text.font_cx, &mut app.text.layout_cx);

    let quad = |x0: f64, y0: f64, x1: f64, y1: f64| -> Vec<Pos2> {
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|&(x, y)| to_pos(t * Point::new(x, y))).collect()
    };
    for (bb, _) in editing.editor.selection_geometry() {
        p.add(egui::Shape::convex_polygon(quad(bb.x0, bb.y0, bb.x1, bb.y1), ACCENT.gamma_multiply(0.3), Stroke::NONE));
    }
    if let Some(bb) = editing.editor.cursor_geometry(1.5) {
        p.add(egui::Shape::convex_polygon(quad(bb.x0, bb.y0, bb.x1, bb.y1), ACCENT, Stroke::NONE));
        let caret = ERect::from_points(&quad(bb.x0, bb.y0, bb.x1, bb.y1));
        let ime_area = editing.editor.ime_cursor_area();
        let area = ERect::from_points(&quad(ime_area.x0, ime_area.y0, ime_area.x1, ime_area.y1));
        p.ctx().output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: area,
                cursor_rect: caret,
                should_interrupt_composition: false,
            })
        });
    }
    // The text box outline.
    let world = app.page().world_transform(id).unwrap_or_default();
    let box_t = screen_transform(app, rect) * world;
    let f = layer.frame.local_rect();
    let pts: Vec<Pos2> = [(f.x0, f.y0), (f.x1, f.y0), (f.x1, f.y1), (f.x0, f.y1)].iter().map(|&(x, y)| to_pos(box_t * Point::new(x, y))).collect();
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.0, ACCENT.gamma_multiply(0.6))));
}
