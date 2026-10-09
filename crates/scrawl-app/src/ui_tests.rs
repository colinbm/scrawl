//! Whole-app UI tests in egui's offscreen harness, rendered with wgpu (including the Vello
//! canvas). Set `SCRAWL_UI_SHOTS=dir` to save a PNG of each test's final frame.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use scrawl_core::LayerId;

use crate::app::ScrawlApp;

fn harness() -> Option<Harness<'static, ScrawlApp>> {
    // Building a wgpu renderer fails on machines without a GPU adapter; skip there.
    let h = std::panic::catch_unwind(|| {
        Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| {
            let mut app = ScrawlApp::new(cc, None);
            app.set_document(scrawl_core::sample::sample_document(), None);
            app
        })
    });
    h.map_err(|_| eprintln!("skipping: no wgpu renderer")).ok()
}

fn select(h: &mut Harness<'_, ScrawlApp>, name: &str) -> LayerId {
    let app = h.state_mut();
    let mut found = None;
    app.page().walk(|l| {
        if l.name == name {
            found = Some(l.id);
        }
    });
    let id = found.unwrap_or_else(|| panic!("no layer named {name}"));
    app.selection = vec![id];
    h.run();
    id
}

/// Clicks at a page-space point on the canvas.
fn click_page(h: &mut Harness<'_, ScrawlApp>, x: f64, y: f64) {
    let app = h.state();
    let s = app.viewport().transform() * scrawl_core::kurbo::Point::new(x, y);
    let pos = app.canvas.rect.min + egui::vec2(s.x as f32, s.y as f32);
    h.event(egui::Event::PointerMoved(pos));
    h.run();
    for pressed in [true, false] {
        let button = egui::PointerButton::Primary;
        h.event(egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE });
        h.run();
    }
}

fn page_to_screen(h: &Harness<'_, ScrawlApp>, x: f64, y: f64) -> egui::Pos2 {
    let app = h.state();
    let s = app.viewport().transform() * scrawl_core::kurbo::Point::new(x, y);
    app.canvas.rect.min + egui::vec2(s.x as f32, s.y as f32)
}

/// Drags between two page-space points in small steps. Leaves the button held if `release` is
/// false, so a test can inspect the canvas mid-drag.
fn drag_page(h: &mut Harness<'_, ScrawlApp>, from: (f64, f64), to: (f64, f64), modifiers: egui::Modifiers, release: bool) {
    let (a, b) = (page_to_screen(h, from.0, from.1), page_to_screen(h, to.0, to.1));
    let button = egui::PointerButton::Primary;
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.event(egui::Event::PointerMoved(a));
    h.run();
    h.event(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers });
    h.run();
    for i in 1..=8 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (i as f32 / 8.0)));
        h.run();
    }
    if release {
        h.event(egui::Event::PointerButton { pos: b, button, pressed: false, modifiers });
        h.run();
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    }
}

fn frame_of(h: &Harness<'_, ScrawlApp>, id: LayerId) -> scrawl_core::Frame {
    h.state().page().find(id).expect("layer exists").frame
}

fn text_of(h: &Harness<'_, ScrawlApp>, id: LayerId) -> Option<String> {
    match &h.state().page().find(id)?.kind {
        scrawl_core::LayerKind::Text(t) => Some(t.text.clone()),
        _ => None,
    }
}

fn layer_count(h: &Harness<'_, ScrawlApp>) -> usize {
    let mut n = 0;
    h.state().page().walk(|_| n += 1);
    n
}

fn save(h: &mut Harness<'_, ScrawlApp>, name: &str) {
    if let Some(dir) = std::env::var_os("SCRAWL_UI_SHOTS") {
        let image = h.render().expect("render");
        image.save(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("save");
    }
}

#[test]
fn inspector_shows_style_sections() {
    let Some(mut h) = harness() else { return };
    h.run();
    select(&mut h, "Card");
    for label in ["FILLS", "BORDERS", "SHADOWS", "INNER SHADOWS", "Blending"] {
        assert!(h.query_by_label(label).is_some(), "missing {label}");
    }
    save(&mut h, "inspector_card");
}

#[test]
fn gradient_popover_shows_canvas_handles() {
    let Some(mut h) = harness() else { return };
    h.run();
    let header = select(&mut h, "Header");
    h.get_by_label("fill paint").click();
    h.run();
    for label in ["Solid", "Linear", "Radial", "Angular", "Remove Stop", "DOCUMENT COLORS"] {
        assert!(h.query_by_label(label).is_some(), "missing {label}");
    }
    let target = h.state().gradient_edit.expect("gradient handles active");
    assert_eq!(target.layer, header);
    save(&mut h, "gradient_popover");

    // Switching to Radial updates the fill.
    h.get_by_label("Radial").click();
    h.run();
    let app = h.state();
    let kind = app.page().find(header).unwrap().style.fills[0].gradient.kind;
    assert_eq!(kind, scrawl_core::GradientKind::Radial);
}

#[test]
fn shadow_rows_add_and_edit() {
    let Some(mut h) = harness() else { return };
    h.run();
    let id = select(&mut h, "Stripe");
    h.get_by_label("Add shadow").click();
    h.run();
    let shadows = h.state().page().find(id).unwrap().style.shadows.len();
    assert_eq!(shadows, 1);
    save(&mut h, "shadow_row");
}

#[test]
fn text_tool_types_and_finishes() {
    let Some(mut h) = harness() else { return };
    h.run();
    let before = layer_count(&h);
    h.key_press(egui::Key::T);
    click_page(&mut h, 480.0, 100.0);
    let id = h.state().canvas.text_edit.as_ref().map(|e| e.id).expect("editing new text");
    h.event(egui::Event::Text("Hello there".into()));
    h.run();
    save(&mut h, "text_editing");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().canvas.text_edit.is_none());
    assert_eq!(text_of(&h, id).as_deref(), Some("Hello there"));
    let layer = h.state().page().find(id).unwrap().clone();
    assert_eq!(layer.name, "Hello there");
    assert!(layer.frame.width > 40.0, "auto width fits text: {:?}", layer.frame);
    assert_eq!(layer_count(&h), before + 1);
}

#[test]
fn empty_text_is_discarded() {
    let Some(mut h) = harness() else { return };
    h.run();
    let before = layer_count(&h);
    h.key_press(egui::Key::T);
    click_page(&mut h, 480.0, 100.0);
    assert!(h.state().canvas.text_edit.is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(layer_count(&h), before);
}

#[test]
fn edit_existing_text_is_one_undo_step() {
    let Some(mut h) = harness() else { return };
    h.run();
    let id = select(&mut h, "Good morning");
    assert!(h.query_by_label("Typeface").is_some());
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().canvas.text_edit.is_some());
    // Enter selects everything, so typing replaces it.
    for c in ["H", "e", "y"] {
        h.event(egui::Event::Text(c.into()));
        h.run();
    }
    // ⌥⇧← selects the word; Backspace deletes it.
    h.key_press_modifiers(egui::Modifiers::ALT | egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
    h.event(egui::Event::Text("Yo".into()));
    h.run();
    assert_eq!(text_of(&h, id).as_deref(), Some("Yo"));
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert_eq!(text_of(&h, id).as_deref(), Some("Good morning"));
}

/// Replaces the document with an artboard holding two rectangles, A and B, returning their ids.
fn two_rects(h: &mut Harness<'_, ScrawlApp>) -> (LayerId, LayerId) {
    use scrawl_core::{Frame, Layer, ops};
    let mut doc = scrawl_core::Document::default();
    let page = &mut doc.pages[0];
    ops::insert_drawn(page, Layer::artboard("Board", Frame::new(0.0, 0.0, 400.0, 400.0)));
    let a = ops::insert_drawn(page, Layer::rectangle(Frame::new(100.0, 100.0, 100.0, 100.0)));
    let b = ops::insert_drawn(page, Layer::rectangle(Frame::new(300.0, 250.0, 50.0, 50.0)));
    h.state_mut().set_document(doc, None);
    h.run();
    h.run();
    (a, b)
}

#[test]
fn moving_snaps_to_nearby_edges() {
    let Some(mut h) = harness() else { return };
    let (_, b) = two_rects(&mut h);
    // B's left edge dragged from 300 to 203 snaps to A's right edge (200).
    drag_page(&mut h, (325.0, 275.0), (228.0, 275.0), egui::Modifiers::NONE, false);
    save(&mut h, "snap_guides");
    h.event(egui::Event::PointerButton {
        pos: page_to_screen(&h, 228.0, 275.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(frame_of(&h, b).x, 200.0);
    assert_eq!(frame_of(&h, b).y, 250.0);

    // Holding ⌃ turns snapping off.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert_eq!(frame_of(&h, b).x, 300.0);
    drag_page(&mut h, (325.0, 275.0), (228.0, 275.0), egui::Modifiers::CTRL, true);
    assert_eq!(frame_of(&h, b).x, 203.0);
}

#[test]
fn resizing_snaps_the_dragged_edge() {
    let Some(mut h) = harness() else { return };
    let (_, b) = two_rects(&mut h);
    h.state_mut().selection = vec![b];
    h.run();
    // Drag B's bottom-right handle from (350, 300) to (397, 203): snaps to the artboard's right
    // edge (400) and A's bottom edge (200).
    drag_page(&mut h, (350.0, 300.0), (397.0, 203.0), egui::Modifiers::NONE, true);
    let f = frame_of(&h, b);
    assert_eq!((f.x, f.y, f.width, f.height), (300.0, 200.0, 100.0, 50.0), "{f:?}");
}

#[test]
fn option_shows_measurements() {
    let Some(mut h) = harness() else { return };
    h.run();
    select(&mut h, "Avatar");
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    let star = page_to_screen(&h, 306.0, 240.0);
    h.event(egui::Event::PointerMoved(star));
    h.run();
    h.run();
    save(&mut h, "measurements");
}

fn artboard_guides(h: &Harness<'_, ScrawlApp>) -> Vec<scrawl_core::grid::Guide> {
    match &h.state().page().layers[0].kind {
        scrawl_core::LayerKind::Artboard(a) => a.guides.clone(),
        _ => panic!("first layer is the artboard"),
    }
}

#[test]
fn ruler_guides_add_snap_and_remove() {
    let Some(mut h) = harness() else { return };
    let (a, b) = two_rects(&mut h);
    h.state_mut().show_rulers = true;
    h.state_mut().selection = vec![a];
    h.run();

    // Clicking the top ruler at x = 250 adds a vertical guide to the artboard.
    let ruler_y = h.state().canvas.rect.min.y + crate::rulers::RULER / 2.0;
    let x = page_to_screen(&h, 250.0, 0.0).x;
    h.event(egui::Event::PointerMoved(egui::pos2(x, ruler_y)));
    h.run();
    for pressed in [true, false] {
        let button = egui::PointerButton::Primary;
        h.event(egui::Event::PointerButton { pos: egui::pos2(x, ruler_y), button, pressed, modifiers: egui::Modifiers::NONE });
        h.run();
    }
    let guides = artboard_guides(&h);
    assert_eq!(guides.len(), 1);
    assert_eq!((guides[0].axis, guides[0].pos), (scrawl_core::snap::Axis::X, 250.0));

    // B's left edge dragged to 253 snaps to the guide.
    h.state_mut().selection = vec![b];
    h.run();
    drag_page(&mut h, (325.0, 275.0), (278.0, 275.0), egui::Modifiers::NONE, true);
    assert_eq!(frame_of(&h, b).x, 250.0);

    h.state_mut().show_grid = true;
    h.state_mut().show_layout = true;
    h.run();
    save(&mut h, "rulers_grid_layout");

    // Dragging the guide's marker down off the ruler removes it.
    let (from, to) = (egui::pos2(x, ruler_y), egui::pos2(x, ruler_y + 200.0));
    let button = egui::PointerButton::Primary;
    h.event(egui::Event::PointerMoved(from));
    h.run();
    h.event(egui::Event::PointerButton { pos: from, button, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run();
    for i in 1..=6 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
        h.run();
    }
    h.event(egui::Event::PointerButton { pos: to, button, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run();
    assert!(artboard_guides(&h).is_empty());
}

#[test]
fn resizing_section_for_contained_layers() {
    let Some(mut h) = harness() else { return };
    h.run();
    let id = select(&mut h, "Avatar");
    assert!(h.query_by_label("RESIZING").is_some());
    if let Some(l) = h.state_mut().page_mut().find_mut(id) {
        l.resizing = scrawl_core::constraints::Resizing { left: true, top: true, fix_width: true, ..Default::default() };
    }
    h.run();
    assert!(h.query_by_label("H: Pinned start").is_some());
    save(&mut h, "resizing_widget");
    // Artboards themselves have no parent, so no constraints.
    select(&mut h, "iPhone 16");
    assert!(h.query_by_label("RESIZING").is_none());
    assert!(h.query_by_label("Adjust content on resize").is_some());
}

#[test]
fn create_stack_from_selection() {
    let Some(mut h) = harness() else { return };
    h.run();
    let ids: Vec<LayerId> = ["Tile (Normal)", "Tile (Multiply)", "Tile (Difference)"].iter().map(|n| select(&mut h, n)).collect();
    h.state_mut().selection = ids.clone();
    h.run();
    h.get_by_label("Create Stack").click();
    h.run();
    h.run();
    let [gid] = h.state().selection[..] else { panic!("stack selected") };
    let stack = *h.state().page().find(gid).and_then(|l| l.stack()).expect("is a stack");
    assert_eq!(stack.direction, scrawl_core::snap::Axis::X);
    assert!(h.query_by_label("Stack layout").is_some());

    // Tighten the spacing: the tiles re-flow left to right, keeping the stack's left edge.
    let left = frame_of(&h, gid).x;
    if let Some(scrawl_core::LayerKind::Group(g)) = h.state_mut().page_mut().find_mut(gid).map(|l| &mut l.kind) {
        g.stack.as_mut().unwrap().spacing = 4.0;
    }
    h.run();
    let xs: Vec<f64> = ids.iter().map(|&id| frame_of(&h, id).x).collect();
    let w = frame_of(&h, ids[0]).width;
    assert_eq!(frame_of(&h, gid).x, left);
    assert_eq!(xs, vec![0.0, w + 4.0, 2.0 * (w + 4.0)]);
    save(&mut h, "stack");
}

#[test]
fn layer_search_filters_and_selects() {
    let Some(mut h) = harness() else { return };
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run();
    // ⌘F focused the field, so typing goes there.
    h.event(egui::Event::Text("tile".into()));
    h.run();
    h.run();
    assert_eq!(h.state().layer_filter.query, "tile");
    assert!(h.query_by_label("3 matches").is_some());
    // Non-matching layers are hidden; the artboard stays as context.
    assert!(h.query_by_label("Avatar").is_none());
    save(&mut h, "layer_search");
    h.get_by_label("Select All").click();
    h.run();
    let names: Vec<String> = h.state().selection.iter().map(|&id| h.state().page().find(id).unwrap().name.clone()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.iter().all(|n| n.starts_with("Tile")));

    // Type filter only.
    h.state_mut().layer_filter = Default::default();
    h.state_mut().layer_filter.kind = crate::ui::layers::KindFilter::Text;
    h.run();
    assert!(h.query_by_label("5 matches").is_some());
}

fn layer_named(h: &Harness<'_, ScrawlApp>, name: &str) -> scrawl_core::Layer {
    let mut found = None;
    h.state().page().walk(|l| {
        if l.name == name {
            found = Some(l.clone());
        }
    });
    found.unwrap_or_else(|| panic!("no layer named {name}"))
}

#[test]
fn shared_layer_style_update_reaches_other_layers() {
    let Some(mut h) = harness() else { return };
    h.run();
    select(&mut h, "Tile (Normal)");
    assert!(h.query_by_label("Modified").is_none());
    h.get_by_label("Add shadow").click();
    h.run();
    h.run();
    assert!(h.query_by_label("Modified").is_some());
    save(&mut h, "shared_style_modified");
    h.get_by_label("Update Style").click();
    h.run();
    let other = layer_named(&h, "Tile (Multiply)");
    assert_eq!(other.style.shadows.len(), 1, "update pushed to every tile");
    assert!(h.query_by_label("Modified").is_none());

    // Reset discards a local change.
    h.get_by_label("Add shadow").click();
    h.run();
    h.run();
    h.get_by_label("Reset").click();
    h.run();
    assert_eq!(layer_named(&h, "Tile (Normal)").style.shadows.len(), 1);
}

#[test]
fn components_tab_applies_styles_and_variables() {
    let Some(mut h) = harness() else { return };
    h.run();
    h.get_by_label("Components").click();
    h.run();
    for label in ["TEXT STYLES", "LAYER STYLES", "COLOR VARIABLES", "Title", "Tile", "Sunshine", "Brand Blue"] {
        assert!(h.query_by_label_contains(label).is_some(), "missing {label}");
    }
    save(&mut h, "components_tab");

    select(&mut h, "Card");
    h.get_by_label_contains("Sunshine").click();
    h.run();
    let card = layer_named(&h, "Card");
    let fill = card.style.fills.last().unwrap();
    assert_eq!(fill.color.to_hex(), "FFD23F");
    assert!(fill.swatch.is_some());

    // Editing the variable recolors every linked fill, including the stripe.
    let id = fill.swatch.unwrap();
    h.state_mut().doc.set_swatch_color(id, scrawl_core::Color::BLACK);
    h.run();
    assert_eq!(layer_named(&h, "Card").style.fills.last().unwrap().color, scrawl_core::Color::BLACK);
    assert_eq!(layer_named(&h, "Stripe").style.fills[0].color, scrawl_core::Color::BLACK);

    // Text styles apply typography.
    select(&mut h, "Body");
    h.get_by_label("Title").click();
    h.run();
    let scrawl_core::LayerKind::Text(t) = layer_named(&h, "Body").kind else { panic!() };
    assert_eq!(t.style.font_size, 32.0);
}

#[test]
fn picking_a_color_unlinks_the_variable() {
    let Some(mut h) = harness() else { return };
    h.run();
    select(&mut h, "Stripe");
    assert!(h.query_by_label("◆ Sunshine").is_some());
    // Simulate a hand edit through the inspector's write-back path.
    let id = layer_named(&h, "Stripe").id;
    h.state_mut().page_mut().find_mut(id).unwrap().style.fills[0].color = scrawl_core::Color::WHITE;
    h.get_by_label("fill paint").click();
    h.run();
    h.get_by_label("Linear").click();
    h.run();
    let fill = &layer_named(&h, "Stripe").style.fills[0];
    assert_eq!(fill.swatch, None, "a color that no longer matches its variable is unlinked");
}
