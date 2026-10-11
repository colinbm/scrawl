//! Whole-app UI tests in egui's offscreen harness, rendered with wgpu (including the Vello
//! canvas). Set `SCRAWL_UI_SHOTS=dir` to save a PNG of each test's final frame.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use scrawl_core::LayerId;

use crate::workspace::Workspace;

fn harness() -> Option<Harness<'static, Workspace>> {
    // Building a wgpu renderer fails on machines without a GPU adapter; skip there.
    let h = std::panic::catch_unwind(|| {
        Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(|cc| crate::workspace::sample(cc))
    });
    h.map_err(|e| eprintln!("skipping: no wgpu renderer ({})", panic_message(&e))).ok()
}

fn panic_message(e: &Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

fn select(h: &mut Harness<'_, Workspace>, name: &str) -> LayerId {
    let app = h.state_mut().front_mut();
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
fn click_page(h: &mut Harness<'_, Workspace>, x: f64, y: f64) {
    let app = h.state().front();
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

fn page_to_screen(h: &Harness<'_, Workspace>, x: f64, y: f64) -> egui::Pos2 {
    let app = h.state().front();
    let s = app.viewport().transform() * scrawl_core::kurbo::Point::new(x, y);
    app.canvas.rect.min + egui::vec2(s.x as f32, s.y as f32)
}

/// Drags between two page-space points in small steps. Leaves the button held if `release` is
/// false, so a test can inspect the canvas mid-drag.
fn drag_page(h: &mut Harness<'_, Workspace>, from: (f64, f64), to: (f64, f64), modifiers: egui::Modifiers, release: bool) {
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

fn frame_of(h: &Harness<'_, Workspace>, id: LayerId) -> scrawl_core::Frame {
    h.state().front().page().find(id).expect("layer exists").frame
}

fn text_of(h: &Harness<'_, Workspace>, id: LayerId) -> Option<String> {
    match &h.state().front().page().find(id)?.kind {
        scrawl_core::LayerKind::Text(t) => Some(t.text.clone()),
        _ => None,
    }
}

fn layer_count(h: &Harness<'_, Workspace>) -> usize {
    let mut n = 0;
    h.state().front().page().walk(|_| n += 1);
    n
}

fn save(h: &mut Harness<'_, Workspace>, name: &str) {
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
    let target = h.state().front().gradient_edit.expect("gradient handles active");
    assert_eq!(target.layer, header);
    save(&mut h, "gradient_popover");

    // Switching to Radial updates the fill.
    h.get_by_label("Radial").click();
    h.run();
    let app = h.state().front();
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
    let shadows = h.state().front().page().find(id).unwrap().style.shadows.len();
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
    let id = h.state().front().canvas.text_edit.as_ref().map(|e| e.id).expect("editing new text");
    h.event(egui::Event::Text("Hello there".into()));
    h.run();
    save(&mut h, "text_editing");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().front().canvas.text_edit.is_none());
    assert_eq!(text_of(&h, id).as_deref(), Some("Hello there"));
    let layer = h.state().front().page().find(id).unwrap().clone();
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
    assert!(h.state().front().canvas.text_edit.is_some());
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
    assert!(h.state().front().canvas.text_edit.is_some());
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
fn two_rects(h: &mut Harness<'_, Workspace>) -> (LayerId, LayerId) {
    use scrawl_core::{Frame, Layer, ops};
    let mut doc = scrawl_core::Document::default();
    let page = &mut doc.pages[0];
    ops::insert_drawn(page, Layer::artboard("Board", Frame::new(0.0, 0.0, 400.0, 400.0)));
    let a = ops::insert_drawn(page, Layer::rectangle(Frame::new(100.0, 100.0, 100.0, 100.0)));
    let b = ops::insert_drawn(page, Layer::rectangle(Frame::new(300.0, 250.0, 50.0, 50.0)));
    h.state_mut().front_mut().set_document(doc, None);
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
    h.state_mut().front_mut().selection = vec![b];
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

fn artboard_guides(h: &Harness<'_, Workspace>) -> Vec<scrawl_core::grid::Guide> {
    match &h.state().front().page().layers[0].kind {
        scrawl_core::LayerKind::Artboard(a) => a.guides.clone(),
        _ => panic!("first layer is the artboard"),
    }
}

#[test]
fn ruler_guides_add_snap_and_remove() {
    let Some(mut h) = harness() else { return };
    let (a, b) = two_rects(&mut h);
    h.state_mut().front_mut().show_rulers = true;
    h.state_mut().front_mut().selection = vec![a];
    h.run();

    // Clicking the top ruler at x = 250 adds a vertical guide to the artboard.
    let ruler_y = h.state().front().canvas.rect.min.y + crate::rulers::RULER / 2.0;
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
    h.state_mut().front_mut().selection = vec![b];
    h.run();
    drag_page(&mut h, (325.0, 275.0), (278.0, 275.0), egui::Modifiers::NONE, true);
    assert_eq!(frame_of(&h, b).x, 250.0);

    h.state_mut().front_mut().show_grid = true;
    h.state_mut().front_mut().show_layout = true;
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
    if let Some(l) = h.state_mut().front_mut().page_mut().find_mut(id) {
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
    h.state_mut().front_mut().selection = ids.clone();
    h.run();
    h.get_by_label("Create Stack").click();
    h.run();
    h.run();
    let [gid] = h.state().front().selection[..] else { panic!("stack selected") };
    let stack = *h.state().front().page().find(gid).and_then(|l| l.stack()).expect("is a stack");
    assert_eq!(stack.direction, scrawl_core::snap::Axis::X);
    assert!(h.query_by_label("Stack layout").is_some());

    // Tighten the spacing: the tiles re-flow left to right, keeping the stack's left edge.
    let left = frame_of(&h, gid).x;
    if let Some(scrawl_core::LayerKind::Group(g)) = h.state_mut().front_mut().page_mut().find_mut(gid).map(|l| &mut l.kind) {
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
    assert_eq!(h.state().front().layer_filter.query, "tile");
    assert!(h.query_by_label("3 matches").is_some());
    // Non-matching layers are hidden; the artboard stays as context.
    assert!(h.query_by_label("Avatar").is_none());
    save(&mut h, "layer_search");
    h.get_by_label("Select All").click();
    h.run();
    let names: Vec<String> = h.state().front().selection.iter().map(|&id| h.state().front().page().find(id).unwrap().name.clone()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.iter().all(|n| n.starts_with("Tile")));

    // Type filter only.
    h.state_mut().front_mut().layer_filter = Default::default();
    h.state_mut().front_mut().layer_filter.kind = crate::ui::layers::KindFilter::Text;
    h.run();
    assert!(h.query_by_label("5 matches").is_some());
}

fn layer_named(h: &Harness<'_, Workspace>, name: &str) -> scrawl_core::Layer {
    let mut found = None;
    h.state().front().page().walk(|l| {
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
    h.state_mut().front_mut().doc.set_swatch_color(id, scrawl_core::Color::BLACK);
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
    assert!(h.query_by_label("Sunshine").is_some());
    // Simulate a hand edit through the inspector's write-back path.
    let id = layer_named(&h, "Stripe").id;
    h.state_mut().front_mut().page_mut().find_mut(id).unwrap().style.fills[0].color = scrawl_core::Color::WHITE;
    h.get_by_label("fill paint").click();
    h.run();
    h.get_by_label("Linear").click();
    h.run();
    let fill = &layer_named(&h, "Stripe").style.fills[0];
    assert_eq!(fill.swatch, None, "a color that no longer matches its variable is unlinked");
}

fn instance_text(layer: &scrawl_core::Layer) -> Vec<String> {
    let scrawl_core::LayerKind::Symbol(i) = &layer.kind else { panic!("not an instance") };
    let mut out = vec![];
    for c in &i.expanded {
        c.walk(&mut |l| {
            if let scrawl_core::LayerKind::Text(t) = &l.kind {
                out.push(t.text.clone());
            }
        });
    }
    out
}

#[test]
fn instance_overrides_edit_text_and_swap() {
    let Some(mut h) = harness() else { return };
    h.run();
    select(&mut h, "Learn more");
    for label in ["SYMBOL", "OVERRIDES", "Edit Master", "Detach", "Label"] {
        assert!(h.query_by_label(label).is_some(), "missing {label}");
    }
    assert_eq!(instance_text(&layer_named(&h, "Learn more")), ["Learn more"]);
    save(&mut h, "instance_overrides");

    // The instance with a swapped icon lists the nested symbol's overrides too.
    select(&mut h, "Icon Button (dot)");
    assert!(h.query_by_label("Icon").is_some());
    assert_eq!(instance_text(&layer_named(&h, "Icon Button (dot)")), ["Saved"]);

    // Resetting overrides restores the master's text.
    h.get_by_label("Reset").click();
    h.run();
    h.run();
    assert_eq!(instance_text(&layer_named(&h, "Icon Button (dot)")), ["Favorite"]);
}

#[test]
fn create_symbol_detach_and_edit_master() {
    let Some(mut h) = harness() else { return };
    h.run();
    let a = select(&mut h, "Tile (Normal)");
    let b = layer_named(&h, "Tile (Multiply)").id;
    h.state_mut().front_mut().selection = vec![a, b];
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::Y);
    h.run();
    h.run();
    let app = h.state().front();
    let [inst] = app.selection[..] else { panic!("instance selected") };
    let layer = app.page().find(inst).unwrap().clone();
    assert_eq!(layer.kind.type_name(), "Symbol");
    assert!(!app.page().contains(a));
    let scrawl_core::LayerKind::Symbol(i) = &layer.kind else { unreachable!() };
    assert_eq!(i.expanded.len(), 2, "instance shows the moved tiles");
    let symbol = i.symbol_id;
    assert!(app.doc.find_master(symbol).is_some());

    // Enter (like double-click) on an instance opens its master on the Symbols page.
    h.key_press(egui::Key::Enter);
    h.run();
    let app = h.state().front();
    assert_eq!(app.page().name, "Symbols");
    assert_eq!(app.selection, vec![app.doc.find_master(symbol).unwrap().1]);
    save(&mut h, "symbol_master");

    // Back on the first page, detaching leaves a group of the tiles.
    let home = h.state().front().doc.pages[0].id;
    h.state_mut().front_mut().set_page(home);
    h.state_mut().front_mut().selection = vec![inst];
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::B);
    h.run();
    let app = h.state().front();
    let group = app.page().find(app.selection[0]).unwrap();
    assert!(group.is_group());
    assert_eq!(group.children().unwrap().len(), 2);
}

#[test]
fn deleting_a_master_detaches_instances() {
    let Some(mut h) = harness() else { return };
    h.run();
    let symbols = h.state().front().doc.pages.iter().position(|p| p.name == "Symbols").unwrap();
    let page = h.state().front().doc.pages[symbols].id;
    h.state_mut().front_mut().set_page(page);
    let master = layer_named(&h, "Button").id;
    h.state_mut().front_mut().selection = vec![master];
    h.run();
    assert!(h.query_by_label("SYMBOL MASTER").is_some());
    assert!(h.query_by_label("2 instances").is_some());
    h.key_press(egui::Key::Backspace);
    h.run();
    let home = &h.state().front().doc.pages[0];
    let mut groups = 0;
    home.walk(|l| {
        if l.name == "Learn more" {
            assert!(l.is_group(), "instance became a group");
            groups += 1;
        }
    });
    assert_eq!(groups, 1);
}

#[test]
fn library_symbols_insert_and_update() {
    let Some(mut h) = harness() else { return };
    let dir = std::env::temp_dir().join(format!("scrawl-ui-library-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Kit.scrawl");
    let mut kit = scrawl_core::sample::sample_document();
    scrawl_core::io::save(&kit, &path).unwrap();

    h.state_mut().front_mut().set_document(scrawl_core::Document::default(), None);
    h.state_mut().front_mut().add_library_path(&path);
    h.state_mut().front_mut().sidebar = crate::app::Sidebar::Components;
    h.run();
    for label in ["LIBRARIES", "Kit", "Icon Button", "Tile"] {
        assert!(h.query_by_label(label).is_some(), "missing {label}");
    }
    save(&mut h, "library");

    // Using a library symbol copies it, and what it depends on, into the document.
    h.get_by_label("Icon Button").click();
    h.run();
    h.run();
    let app = h.state().front();
    let names: Vec<_> = app.doc.foreign_symbols.iter().map(|f| f.item.name.clone()).collect();
    assert!(names.contains(&"Icon Button".to_owned()) && names.contains(&"Icon/Star".to_owned()), "{names:?}");
    assert!(app.doc.foreign_swatches.iter().any(|f| f.item.name == "Brand Blue"));
    let inst = app.selection[0];
    let background = |h: &Harness<'_, Workspace>| {
        let scrawl_core::LayerKind::Symbol(i) = &h.state().front().page().find(inst).unwrap().kind else { panic!() };
        i.expanded[0].style.fills[0].color
    };
    assert_eq!(background(&h).to_hex(), "1F8FFF");

    // Changing the library offers an update, which reaches the instance.
    let brand = kit.swatches.iter().find(|s| s.name == "Brand Blue").unwrap().id;
    kit.set_swatch_color(brand, scrawl_core::Color::BLACK);
    scrawl_core::io::save(&kit, &path).unwrap();
    h.state_mut().front_mut().reload_libraries();
    h.run();
    assert!(h.query_by_label_contains("updates available").is_some());
    h.get_by_label("Update").click();
    h.run();
    h.run();
    assert_eq!(background(&h), scrawl_core::Color::BLACK);
    assert!(h.query_by_label_contains("updates available").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// An encoded PNG of `w`×`h` pixels in one color.
fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn importing_images_makes_image_layers() {
    let Some(mut h) = harness() else { return };
    h.run();
    let files = vec![("/tmp/Photo.png".to_owned(), png(64, 32, [255, 0, 0, 255])), ("bad.txt".to_owned(), b"nope".to_vec())];
    crate::import::import_files(h.state_mut().front_mut(), files, scrawl_core::kurbo::Point::new(100.0, 600.0));
    h.run();
    h.run();
    let app = h.state().front();
    assert!(app.error.as_deref().is_some_and(|e| e.contains("bad.txt")));
    let [id] = app.selection[..] else { panic!("one image added") };
    let layer = app.page().find(id).unwrap();
    assert_eq!(layer.name, "Photo");
    assert_eq!(layer.kind.type_name(), "Image");
    assert_eq!((layer.frame.width, layer.frame.height), (64.0, 32.0));
    assert_eq!(app.page().parent(id), Some(Some(app.page().layers[0].id)), "dropped into the artboard");
    h.state_mut().front_mut().error = None;
    h.run();
    assert!(h.query_by_label("64×32 px").is_some());
    save(&mut h, "image_layer");
}

#[test]
fn export_formats_slices_and_files() {
    let Some(mut h) = harness() else { return };
    h.run();
    let card = select(&mut h, "Card");
    h.get_by_label("Add export").click();
    h.run();
    h.get_by_label("Add export").click();
    h.run();
    let exports = h.state().front().page().find(card).unwrap().exports.clone();
    assert_eq!(exports.iter().map(|f| f.file_name("Card")).collect::<Vec<_>>(), ["Card.png", "Card@2x.png"]);
    save(&mut h, "export_section");

    // A slice drawn with S exports 1× PNG of whatever is under it.
    h.key_press(egui::Key::S);
    h.run();
    drag_page(&mut h, (24.0, 400.0), (144.0, 507.0), egui::Modifiers::NONE, true);
    h.run();
    let app = h.state().front();
    let [slice] = app.selection[..] else { panic!("slice selected") };
    assert_eq!(app.page().find(slice).unwrap().kind.type_name(), "Slice");

    let dir = std::env::temp_dir().join(format!("scrawl-ui-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let page = h.state().front().page;
    let written = h.state_mut().front_mut().export_to(&dir, &[(page, card), (page, slice)], true);
    if let Some(e) = &h.state().front().error {
        // No GPU adapter for the offscreen exporter: nothing more to check.
        assert!(e.contains("exporter"), "{e}");
        return;
    }
    assert_eq!(written.len(), 3);
    let size = |name: &str| image::image_dimensions(dir.join(name)).unwrap();
    let (w, h1) = size("Card.png");
    assert_eq!(size("Card@2x.png"), (w * 2, h1 * 2));
    let f = h.state().front().page().find(slice).unwrap().frame;
    assert_eq!(size("Slice.png"), (f.width.ceil() as u32, f.height.ceil() as u32));
    // The notice animates away, so step a frame rather than waiting for the UI to settle.
    h.step();
    assert!(h.query_by_label("Exported 3 files").is_some());
    std::fs::remove_dir_all(&dir).ok();
}

const LOGO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40">
    <rect id="Back" width="80" height="40" rx="8" fill="#7C5CFF"/>
    <circle id="Dot" cx="20" cy="20" r="10" fill="#FFD23F"/>
</svg>"##;

#[test]
fn svg_files_and_pasted_svg_become_layers() {
    let Some(mut h) = harness() else { return };
    h.run();
    let files = vec![("/tmp/Logo.svg".to_owned(), LOGO_SVG.as_bytes().to_vec())];
    crate::import::import_files(h.state_mut().front_mut(), files, scrawl_core::kurbo::Point::new(200.0, 600.0));
    h.run();
    let app = h.state().front();
    assert!(app.error.is_none(), "{:?}", app.error);
    let [id] = app.selection[..] else { panic!("one group added") };
    let logo = app.page().find(id).unwrap();
    assert_eq!((logo.name.as_str(), logo.kind.type_name()), ("Logo", "Group"));
    let names: Vec<_> = logo.children().unwrap().iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["Back", "Dot"]);
    let b = app.page().world_bounds(id).unwrap();
    assert!((b.center().x - 200.0).abs() < 1.0 && (b.width() - 80.0).abs() < 0.5, "{b:?}");
    save(&mut h, "svg_import");

    // Pasting SVG markup works too.
    let before = layer_count(&h);
    h.state_mut().front_mut().paste(Some(LOGO_SVG));
    h.run();
    assert_eq!(layer_count(&h), before + 3);
}

#[test]
fn opening_a_sketch_file_imports_it() {
    let Some(mut h) = harness() else { return };
    let dir = std::env::temp_dir().join(format!("scrawl-ui-sketch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Design.sketch");
    std::fs::write(&path, scrawl_core::sketch::export(&scrawl_core::sample::sample_document()).unwrap()).unwrap();

    h.state_mut().front_mut().open_path(&path);
    h.step();
    let app = h.state().front();
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(app.doc.pages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Home", "Symbols"]);
    assert!(app.file_path.is_none() && app.is_dirty(), "imported, not yet saved as Scrawl");
    assert_eq!(instance_text(&layer_named(&h, "Learn more")), ["Learn more"]);
    assert!(h.query_by_label("Imported Design.sketch").is_some());
    save(&mut h, "sketch_import");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn hotspots_link_to_the_artboard_clicked_next() {
    let Some(mut h) = harness() else { return };
    h.run();
    let buttons = layer_named(&h, "Buttons").id;
    h.key_press(egui::Key::H);
    h.run();
    drag_page(&mut h, (300.0, 700.0), (380.0, 760.0), egui::Modifiers::NONE, true);
    h.run();
    let [hotspot] = h.state().front().selection[..] else { panic!("hotspot selected") };
    assert_eq!(h.state().front().page().find(hotspot).unwrap().kind.type_name(), "Hotspot");
    assert!(h.state().front().canvas.linking.is_some(), "drawing a hotspot asks for its destination");
    save(&mut h, "hotspot_linking");
    let pos = page_to_screen(&h, 600.0, 150.0);
    click_at(&mut h, pos);
    assert!(h.query_by_label("Linked to Buttons").is_some());
    h.state_mut().front_mut().notice = None;
    h.run();
    let app = h.state().front();
    assert!(app.canvas.linking.is_none());
    let link = app.page().find(hotspot).unwrap().flow.expect("linked");
    assert_eq!(link.destination, scrawl_core::prototype::Destination::Artboard(buttons));
    assert!(h.query_by_label("PROTOTYPING").is_some());

    // W on another layer, then a click on empty canvas, cancels without linking.
    let star = select(&mut h, "Star");
    h.key_press(egui::Key::W);
    h.run();
    assert!(h.state().front().canvas.linking.is_some());
    click_page(&mut h, 450.0, 600.0);
    h.run();
    assert!(h.state().front().canvas.linking.is_none());
    assert!(h.state().front().page().find(star).unwrap().flow.is_none());
    // The inspector sets links too.
    h.get_by_value("None").click();
    h.run();
    h.get_all_by_label("Back").last().expect("Back option").click();
    h.run();
    assert_eq!(h.state().front().page().find(star).unwrap().flow, Some(scrawl_core::prototype::FlowLink::back()));
}

/// The screen position of artboard point (x, y) in the preview.
fn preview_pos(h: &Harness<'_, Workspace>, x: f32, y: f32) -> egui::Pos2 {
    let (area, scale) = h.state().front().preview.as_ref().and_then(|p| p.view).expect("preview shown");
    area.min + egui::vec2(x, y) * scale as f32
}

fn click_at(h: &mut Harness<'_, Workspace>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
    }
}

#[test]
fn preview_follows_links_and_goes_back() {
    let Some(mut h) = harness() else { return };
    h.run();
    let (phone, buttons) = (layer_named(&h, "iPhone 16").id, layer_named(&h, "Buttons").id);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::P);
    h.run_steps(3);
    let current = |h: &Harness<'_, Workspace>| h.state().front().preview.as_ref().map(|p| p.player.current);
    assert_eq!(current(&h), Some(phone), "starts at the flow's start point");

    // The card links to the Buttons artboard, sliding in from the right.
    let card = preview_pos(&h, 200.0, 300.0);
    click_at(&mut h, card);
    assert_eq!(current(&h), Some(buttons));
    assert!(h.state().front().preview.as_ref().unwrap().player.transition.is_some());
    save(&mut h, "preview_transition");
    h.run_steps(40);
    assert!(h.state().front().preview.as_ref().unwrap().player.transition.is_none());
    save(&mut h, "preview_buttons");

    // The Back hotspot along the top returns.
    let back = preview_pos(&h, 200.0, 10.0);
    click_at(&mut h, back);
    h.run_steps(40);
    assert_eq!(current(&h), Some(phone));

    // A click on no link only flashes the hotspots.
    let empty = preview_pos(&h, 200.0, 820.0);
    click_at(&mut h, empty);
    assert_eq!(current(&h), Some(phone));
    h.run_steps(60);

    // Deleting the artboard on show falls back to the start.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().front().preview.is_none(), "Escape closes the preview");
}

#[test]
fn prototype_links_round_trip_through_files() {
    let Some(mut h) = harness() else { return };
    h.run();
    let dir = std::env::temp_dir().join(format!("scrawl-ui-proto-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Proto.scrawl");
    let card = layer_named(&h, "Card");
    h.state_mut().front_mut().file_path = Some(path.clone());
    assert!(h.state_mut().front_mut().save());
    h.state_mut().front_mut().open_path(&path);
    h.run();
    assert_eq!(layer_named(&h, "Card").flow, card.flow);
    assert!(layer_named(&h, "Header").fixed_to_viewport);
    std::fs::remove_dir_all(&dir).ok();
}

/// A harness whose workspace keeps its files in a fresh temporary folder.
fn harness_with_support(tag: &str) -> Option<(Harness<'static, Workspace>, std::path::PathBuf)> {
    let support = std::env::temp_dir().join(format!("scrawl-ui-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&support).ok();
    std::fs::create_dir_all(&support).unwrap();
    let dir = support.clone();
    let h = std::panic::catch_unwind(move || {
        Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(move |cc| Workspace::new(cc, vec![], Some(dir)))
    });
    match h {
        Ok(h) => Some((h, support)),
        Err(e) => {
            eprintln!("skipping: no wgpu renderer ({})", panic_message(&e));
            None
        }
    }
}

#[test]
fn new_windows_open_focus_and_close() {
    let Some(mut h) = harness() else { return };
    h.run();
    let first = h.state().front().uid;
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
    h.run();
    assert_eq!(h.state().windows.len(), 2);
    let second = h.state().front().uid;
    assert_ne!(first, second, "the new window has focus");
    assert!(h.state().front().is_pristine());
    // The Window menu lists both.
    assert_eq!(h.state().front().menu_context.windows.len(), 2);

    // Closing the root window hands its viewport to the other one.
    h.state_mut().windows[0].requests.push(crate::commands::Request::CloseWindow);
    h.run();
    assert_eq!(h.state().windows.len(), 1);
    assert_eq!(h.state().windows[0].uid, second);
    assert_eq!(h.state().front().uid, second);
    save(&mut h, "after_close");
}

#[test]
fn opening_a_file_reuses_empty_windows_and_finds_open_ones() {
    let Some((mut h, support)) = harness_with_support("open") else { return };
    h.run();
    let path = support.join("Design.scrawl");
    scrawl_core::io::save(&scrawl_core::sample::sample_document(), &path).unwrap();
    let ws = h.state_mut();
    assert!(ws.front().is_pristine());
    ws.front_mut().requests.push(crate::commands::Request::Open(path.clone()));
    h.run();
    assert_eq!(h.state().windows.len(), 1, "the empty window took the file");
    assert_eq!(h.state().front().file_path.as_deref(), Some(path.as_path()));
    assert_eq!(h.state().prefs.recent.len(), 1, "it's a recent file");
    let menus = crate::commands::menus(h.state().front(), &h.state().front().menu_context);
    let file = menus.iter().find(|m| m.title == "File").unwrap();
    let recent = file.entries.iter().find_map(|e| match e {
        crate::commands::Entry::Submenu { label, entries, .. } if label == "Open Recent" => Some(entries.clone()),
        _ => None,
    });
    assert!(matches!(&recent.unwrap()[0], crate::commands::Entry::Item(i) if i.label == "Design.scrawl"));

    // Opening it again doesn't open a second copy; a new window then opens another file.
    h.state_mut().front_mut().requests.push(crate::commands::Request::Open(path.clone()));
    h.run();
    assert_eq!(h.state().windows.len(), 1);
    let other = support.join("Other.scrawl");
    scrawl_core::io::save(&scrawl_core::Document::default(), &other).unwrap();
    h.state_mut().front_mut().requests.push(crate::commands::Request::Open(other.clone()));
    h.run();
    assert_eq!(h.state().windows.len(), 2);
    assert_eq!(h.state().front().file_path.as_deref(), Some(other.as_path()));
    // The prefs file remembers both, newest first.
    let prefs = crate::prefs::Prefs::load(&support);
    assert!(prefs.recent[0].ends_with("Other.scrawl") && prefs.recent[1].ends_with("Design.scrawl"));
    std::fs::remove_dir_all(&support).ok();
}

#[test]
fn unsaved_changes_are_recovered_after_a_crash() {
    let Some((mut h, support)) = harness_with_support("recover") else { return };
    h.run();
    let app = h.state_mut().front_mut();
    app.set_document(scrawl_core::sample::sample_document(), None);
    app.page_mut().name = "Edited".into();
    assert!(app.is_dirty());
    crate::autosave::tick(app, &support, 1000.0);
    // The autosave is written on another thread.
    let dir = crate::autosave::dir(&support);
    for _ in 0..100 {
        if std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // "Crash": start a new workspace on the same folder without closing this one.
    let dir2 = support.clone();
    let mut h2 = Harness::builder().with_size([1440.0, 900.0]).wgpu().build_eframe(move |cc| Workspace::new(cc, vec![], Some(dir2)));
    // The "Recovered" notice animates, so step rather than run to a standstill.
    h2.step();
    let ws = h2.state();
    assert_eq!(ws.windows.len(), 1, "the recovered document replaces the empty window");
    let app = ws.front();
    assert_eq!(app.doc.pages[0].name, "Edited");
    assert!(app.is_dirty(), "recovered changes are unsaved");
    drop(h);
    std::fs::remove_dir_all(&support).ok();
}

#[test]
fn saving_keeps_versions_to_restore() {
    let Some((mut h, support)) = harness_with_support("versions") else { return };
    h.run();
    let path = support.join("Doc.scrawl");
    let app = h.state_mut().front_mut();
    app.set_document(scrawl_core::sample::sample_document(), None);
    app.file_path = Some(path.clone());
    assert!(app.save());
    app.page_mut().name = "Second".into();
    assert!(app.save());
    let versions = crate::versions::list(&support, &path);
    assert_eq!(versions.len(), 1, "the second save kept the first");
    let app = h.state_mut().front_mut();
    crate::commands::execute(app, crate::commands::Command::BrowseVersions, &egui::Context::default());
    h.run();
    assert!(h.query_by_label("Restore").is_some());
    save(&mut h, "versions");
    h.get_by_label("Restore").click();
    h.step();
    let app = h.state().front();
    assert_eq!(app.doc.pages[0].name, "Home", "back to the first save");
    assert!(app.versions.is_none());
    h.state_mut().front_mut().undo();
    assert_eq!(h.state().front().doc.pages[0].name, "Second", "restoring is undoable");
    std::fs::remove_dir_all(&support).ok();
}

#[test]
fn scripts_edit_the_document_undoably() {
    let Some(mut h) = harness() else { return };
    h.run();
    select(&mut h, "Card");
    let app = h.state_mut().front_mut();
    let src = r#"
        const [card] = scrawl.selectedLayers;
        card.name = "Script Card";
        const dot = scrawl.create("oval", { name: "Dot", x: 10, y: 10, width: 20, height: 20 });
        scrawl.insert(dot, scrawl.parentOf(card.id));
        scrawl.selection = [dot];
        console.log("made", dot.name);
        scrawl.message("Done");
    "#;
    let out = crate::plugins::run(app, src, "test.js").unwrap();
    assert_eq!(out.log, ["made Dot"]);
    assert_eq!(out.messages, ["Done"]);
    h.run();
    let dot = layer_named(&h, "Dot");
    assert_eq!(h.state().front().selection, [dot.id]);
    assert_eq!(layer_named(&h, "Script Card").kind.type_name(), "Rectangle");
    h.state_mut().front_mut().undo();
    h.run();
    assert!(h.state().front().page().find(dot.id).is_none(), "one undo reverts the whole script");

    // A failing script changes nothing and says why.
    let before = h.state().front().doc.clone();
    let err = crate::plugins::run(h.state_mut().front_mut(), "scrawl.page.layers = 5;", "bad.js").unwrap_err();
    assert!(err.0.contains("can't read"), "{}", err.0);
    assert_eq!(h.state().front().doc, before);

    // The console runs scripts too.
    h.state_mut().front_mut().requests.push(crate::commands::Request::RunScript);
    h.run();
    h.state_mut().front_mut().console.as_mut().unwrap().code = "scrawl.page.name = 'Console';".into();
    h.get_by_label("Run").click();
    h.run();
    assert_eq!(h.state().front().page().name, "Console");
    save(&mut h, "script_console");
}

#[test]
fn plugin_commands_appear_and_run() {
    let Some((mut h, support)) = harness_with_support("plugins") else { return };
    h.run();
    let dir = crate::plugins::folder(&support);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shout.js"), "// @name Shout\nfor (const p of scrawl.pages) p.name = p.name.toUpperCase();\nscrawl.message('Shouted');").unwrap();
    let app = h.state_mut().front_mut();
    app.set_document(scrawl_core::sample::sample_document(), None);
    app.requests.push(crate::commands::Request::ReloadPlugins);
    h.step();
    h.state_mut().front_mut().notice = None;
    h.run();
    let ws = h.state();
    assert_eq!(ws.front().menu_context.plugins, [("shout".to_owned(), "Shout".to_owned())]);
    h.get_by_label("Plugins").click();
    h.run();
    h.get_by_label("Shout").click();
    h.step();
    h.step();
    let names: Vec<_> = h.state().front().doc.pages.iter().map(|p| p.name.clone()).collect();
    assert_eq!(names, ["HOME", "SYMBOLS"]);
    assert!(h.query_by_label("Shouted").is_some());
    std::fs::remove_dir_all(&support).ok();
}

#[test]
fn shape_groups_stay_editable() {
    let Some(mut h) = harness() else { return };
    h.run();
    let ring = select(&mut h, "Ring");
    assert!(h.state().front().page().find(ring).unwrap().is_shape_group(), "the sample's ring is a live subtract");
    assert!(h.query_by_label("Flatten").is_some());
    assert!(h.query_by_label("FILLS").is_some(), "shape groups have their own style");
    // Click the hole: it isn't part of the ring.
    let b = h.state().front().page().world_bounds(ring).unwrap();
    let c = b.center();
    click_page(&mut h, c.x, c.y);
    assert!(!h.state().front().selection.contains(&ring), "the hole isn't the ring");
    // Pick the inner circle and union it: the hole fills in.
    let inner = h.state().front().page().find(ring).unwrap().children().unwrap()[1].id;
    h.state_mut().front_mut().selection = vec![inner];
    h.run();
    assert!(h.query_by_label("BOOLEAN").is_some());
    h.get_by_label("Union").click();
    h.run();
    assert_eq!(h.state().front().page().find(inner).unwrap().boolean_op, scrawl_core::ops::BooleanOp::Union);
    h.state_mut().front_mut().selection.clear();
    h.run();
    click_page(&mut h, c.x, c.y);
    assert_eq!(h.state().front().selection, [ring], "now the middle is part of it");
    save(&mut h, "shape_group");
    // Flatten bakes it.
    h.get_by_label("Flatten").click();
    h.run();
    assert_eq!(h.state().front().page().find(ring).unwrap().kind.type_name(), "Path");
}

fn path_of(h: &Harness<'_, Workspace>, id: LayerId) -> scrawl_core::PathShape {
    h.state().front().page().find(id).unwrap().editable_path().unwrap()
}

#[test]
fn point_editing_snaps_cuts_and_rounds() {
    let Some(mut h) = harness() else { return };
    h.run();
    let stripe = select(&mut h, "Stripe");
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().front().canvas.edit.is_some());
    let origin = h.state().front().page().world_bounds(stripe).unwrap().origin();
    // The top-left point, dragged 2 units off the artboard's left edge, snaps back onto it.
    drag_page(&mut h, (origin.x, origin.y), (origin.x + 2.0, origin.y + 30.0), egui::Modifiers::NONE, true);
    h.run();
    let p = path_of(&h, stripe);
    let world = h.state().front().page().world_transform(stripe).unwrap();
    let moved = p.subpaths[0].points.iter().map(|c| world * c.point).min_by(|a, b| (a.y - origin.y - 30.0).abs().total_cmp(&(b.y - origin.y - 30.0).abs())).unwrap();
    assert!((moved.x - origin.x).abs() < 1e-6, "snapped to the edge: {moved:?}");
    assert!((moved.y - (origin.y + 30.0)).abs() < 6.0, "within snapping distance: {moved:?}");

    // A corner radius on the selected point rounds it.
    crate::vector::set_corner_radius(h.state_mut().front_mut(), 8.0);
    h.run();
    assert!(path_of(&h, stripe).subpaths[0].points.iter().any(|c| c.corner_radius == 8.0));
    assert!(h.query_by_label("CORNER RADIUS").is_some());

    // Scissors (C) cut the bottom edge out: the path opens.
    h.key_press(egui::Key::C);
    h.run();
    assert!(h.state().front().canvas.edit.as_ref().unwrap().scissors);
    let b = h.state().front().page().world_bounds(stripe).unwrap();
    click_page(&mut h, b.center().x, b.y1);
    assert!(!path_of(&h, stripe).subpaths[0].closed, "the cut opened the path");
    save(&mut h, "scissors");
}

#[test]
fn rotate_copies_with_a_live_pivot() {
    let Some(mut h) = harness() else { return };
    h.run();
    let star = select(&mut h, "Star");
    crate::commands::execute(h.state_mut().front_mut(), crate::commands::Command::RotateCopies, &egui::Context::default());
    h.run();
    let group = h.state().front().canvas.rotate_copies.as_ref().map(|r| r.group).expect("rotating");
    let count = |h: &Harness<'_, Workspace>| h.state().front().page().find(group).and_then(|g| g.children().map(Vec::len));
    assert_eq!(count(&h), Some(6));
    assert!(h.query_by_label("Rotate Copies").is_some());
    crate::rotate_copies::set_count(h.state_mut().front_mut(), 3);
    h.run();
    let group = h.state().front().canvas.rotate_copies.as_ref().unwrap().group;
    assert_eq!(h.state().front().page().find(group).unwrap().children().unwrap().len(), 4);
    assert!(h.state().front().page().find(group).unwrap().children().unwrap().iter().any(|c| c.id == star), "the original is one of them");
    // Dragging the pivot spreads them out.
    let before = h.state().front().page().world_bounds(group).unwrap();
    let sb = h.state().front().page().find(group).unwrap().children().unwrap()[0].bounds_in(scrawl_core::kurbo::Affine::IDENTITY).unwrap();
    let parent = h.state().front().page().world_transform(group).unwrap();
    let pivot = parent * scrawl_core::kurbo::Point::new(sb.center().x, sb.y1);
    drag_page(&mut h, (pivot.x, pivot.y), (pivot.x, pivot.y + 60.0), egui::Modifiers::NONE, true);
    h.run();
    let group = h.state().front().canvas.rotate_copies.as_ref().expect("still rotating").group;
    let after = h.state().front().page().world_bounds(group).unwrap();
    assert!(after.width() > before.width() + 50.0, "{before:?} → {after:?}");
    save(&mut h, "rotate_copies");
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().front().canvas.rotate_copies.is_none());
    assert_eq!(h.state().front().selection, [group]);
}

#[test]
fn noise_fills_and_gradient_color_spaces_in_the_popover() {
    let Some(mut h) = harness() else { return };
    h.run();
    let card = select(&mut h, "Card");
    h.get_by_label("fill paint").click();
    h.run();
    h.get_by_label("Noise").click();
    h.run();
    assert_eq!(layer_named(&h, "Card").style.fills[0].fill_type, scrawl_core::FillType::Noise);
    h.get_by_label("White").click();
    h.run();
    assert_eq!(h.state().front().page().find(card).unwrap().style.fills[0].noise.kind, scrawl_core::NoiseKind::White);
    save(&mut h, "noise_popover");
    h.get_by_label("Linear").click();
    h.run();
    assert!(h.query_by_label("Blend in").is_some());
    h.get_by_label("OKLCH").click();
    h.run();
    assert_eq!(h.state().front().page().find(card).unwrap().style.fills[0].gradient.interpolation, scrawl_core::Interpolation::Oklch);
}

#[test]
fn blend_mode_combo_works_inside_the_paint_popover() {
    let Some(mut h) = harness() else { return };
    h.run();
    let card = select(&mut h, "Card");
    h.get_by_label("fill paint").click();
    h.run();
    // Real pointer clicks, not accessibility actions.
    let menu = h.get_by_label_contains("Normal ⏵").rect().center();
    click_at(&mut h, menu);
    h.run();
    let option = h.get_by_label("Multiply").rect().center();
    click_at(&mut h, option);
    h.run();
    assert_eq!(h.state().front().page().find(card).unwrap().style.fills[0].blend_mode, scrawl_core::BlendMode::Multiply);
}

fn text_layer(h: &Harness<'_, Workspace>, id: LayerId) -> scrawl_core::TextLayer {
    match &h.state().front().page().find(id).unwrap().kind {
        scrawl_core::LayerKind::Text(t) => t.clone(),
        _ => panic!("not text"),
    }
}

#[test]
fn rich_text_styles_part_of_a_layer() {
    let Some(mut h) = harness() else { return };
    h.run();
    let id = select(&mut h, "Body");
    let width_before = h.state().front().page().find(id).unwrap().frame;
    h.key_press(egui::Key::Enter);
    h.run();
    // Put the caret at the start and select the first word ("Scrawl").
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowUp);
    h.key_press_modifiers(egui::Modifiers::ALT | egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    h.run();
    assert_eq!(crate::text_edit::selected_range(h.state().front()), Some(0..6));
    assert!(h.query_by_label("Styling 6 selected characters").is_some());
    // ⌘B bolds just that word.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    h.run();
    let t = text_layer(&h, id);
    assert_eq!(t.runs.len(), 1);
    assert_eq!((t.runs[0].start, t.runs[0].end, t.runs[0].style.font_weight), (0, 6, Some(700)));
    assert_eq!(t.style.font_weight, 400, "the rest stays regular");
    // ⌘B again unbolds it, leaving no runs.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    h.run();
    assert!(text_layer(&h, id).runs.is_empty());
    // A bigger size from the inspector applies to the selection and grows the layer.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    if let Some(scrawl_core::LayerKind::Text(t)) = h.state_mut().front_mut().page_mut().find_mut(id).map(|l| &mut l.kind) {
        t.set_run_style(0..6, |s| {
            s.font_size = Some(30.0);
            s.color = Some(scrawl_core::Color::rgba(1.0, 0.0, 0.0, 1.0));
        });
    }
    h.run();
    assert!(h.state().front().page().find(id).unwrap().frame.height > width_before.height, "a taller first line");
    // Typing at the end of the word continues its style.
    h.key_press(egui::Key::ArrowRight);
    h.event(egui::Event::Text("!".into()));
    h.run();
    let t = text_layer(&h, id);
    assert!(t.text.starts_with("Scrawl! "), "{}", t.text);
    assert_eq!((t.runs[0].start, t.runs[0].end), (0, 7));
    save(&mut h, "rich_text");
    // The selection geometry follows the big run: selecting it spans more than one line's height.
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert!(text_layer(&h, id).runs.is_empty(), "the whole session was one undo step");
}

#[test]
fn clicks_find_text_moved_by_paragraph_spacing_and_lists() {
    let Some(mut h) = harness() else { return };
    h.run();
    let id = select(&mut h, "Body");
    if let Some(scrawl_core::LayerKind::Text(t)) = h.state_mut().front_mut().page_mut().find_mut(id).map(|l| &mut l.kind) {
        t.text = "first\nsecond".into();
        t.style.paragraph_spacing = 40.0;
        t.style.list = scrawl_core::text::ListStyle::Bullet;
    }
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let b = h.state().front().page().world_bounds(id).unwrap();
    // The second paragraph sits at the bottom of the (taller) frame; click near its end.
    click_page(&mut h, b.x1 - 4.0, b.y1 - 4.0);
    let sel = h.state().front().canvas.text_edit.as_ref().map(|_| ());
    assert!(sel.is_some(), "still editing");
    h.event(egui::Event::Text("!".into()));
    h.run();
    assert_eq!(text_layer(&h, id).text, "first\nsecond!", "the caret landed at the end of the second paragraph");
    save(&mut h, "paragraphs");
}

#[test]
fn text_attaches_to_a_path_and_back() {
    let Some(mut h) = harness() else { return };
    h.run();
    let text = layer_named(&h, "Underlined & spaced").id;
    let ring = layer_named(&h, "Ring").id;
    h.state_mut().front_mut().selection = vec![text, ring];
    h.run();
    crate::commands::execute(h.state_mut().front_mut(), crate::commands::Command::AttachTextToPath, &egui::Context::default());
    h.run();
    assert!(h.state().front().error.is_none(), "{:?}", h.state().front().error);
    assert_eq!(h.state().front().selection, [text]);
    assert!(text_layer(&h, text).on_path.is_some());
    // The text now covers the ring's area.
    let (tb, rb) = (h.state().front().page().world_bounds(text).unwrap(), h.state().front().page().world_bounds(ring).unwrap());
    assert!(tb.contains(rb.center()), "{tb:?} {rb:?}");
    assert!(h.query_by_label("Detach from Path").is_some());
    save(&mut h, "text_on_path");
    h.get_by_label("Detach from Path").click();
    h.run();
    assert!(text_layer(&h, text).on_path.is_none());
}

#[test]
fn smart_distribute_handles_change_the_gaps() {
    let Some(mut h) = harness() else { return };
    h.run();
    let names = ["Tile (Normal)", "Tile (Multiply)", "Tile (Difference)"];
    let ids: Vec<LayerId> = names.iter().map(|n| layer_named(&h, n).id).collect();
    h.state_mut().front_mut().selection = ids.clone();
    h.run();
    let bounds = |h: &Harness<'_, Workspace>| ids.iter().map(|&id| h.state().front().page().world_bounds(id).unwrap()).collect::<Vec<_>>();
    let b = bounds(&h);
    let gap = b[1].x0 - b[0].x1;
    // The handle sits in the middle of the first gap.
    let handle = ((b[0].x1 + b[1].x0) / 2.0, (b[0].center().y + b[1].center().y) / 2.0);
    drag_page(&mut h, handle, (handle.0 + 10.0, handle.1), egui::Modifiers::NONE, true);
    h.run();
    let after = bounds(&h);
    let gaps: Vec<f64> = after.windows(2).map(|w| w[1].x0 - w[0].x1).collect();
    assert!((gaps[0] - gaps[1]).abs() < 0.01, "still even: {gaps:?}");
    assert!((gaps[0] - (gap + 10.0)).abs() < 1.5, "{gap} → {gaps:?}");
    assert_eq!(after[0].x0, b[0].x0, "the first stays put");
    assert_eq!(h.state().front().selection, ids, "dragging a handle keeps the selection");
    save(&mut h, "smart_distribute");
}

#[test]
fn stack_padding_and_child_sizing_from_the_inspector() {
    let Some(mut h) = harness() else { return };
    h.run();
    let names = ["Tile (Normal)", "Tile (Multiply)", "Tile (Difference)"];
    let ids: Vec<LayerId> = names.iter().map(|n| layer_named(&h, n).id).collect();
    h.state_mut().front_mut().selection = ids.clone();
    h.run();
    h.state_mut().front_mut().make_stack();
    h.run();
    let [stack] = h.state().front().selection[..] else { panic!("stack selected") };
    assert!(h.query_by_label("Wrap at").is_some() && h.query_by_label("Padding").is_some());
    h.get_by_label("Wrap at").click();
    h.run();
    let page = h.state().front().page();
    let s = *page.find(stack).unwrap().stack().unwrap();
    assert_eq!(s.wrap, Some(400.0));
    // A child in the stack can fill across.
    h.state_mut().front_mut().selection = vec![ids[0]];
    h.run();
    assert!(h.query_by_label("IN STACK").is_some());
    h.get_by_label("Fill across").click();
    h.run();
    assert!(h.state().front().page().find(ids[0]).unwrap().stack_item.fill);
    save(&mut h, "stack_options");
}

#[test]
fn library_files_are_watched_and_stops_follow_variables() {
    let Some(mut h) = harness() else { return };
    h.run();
    let dir = std::env::temp_dir().join(format!("scrawl-ui-watch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lib_path = dir.join("Lib.scrawl");
    let mut lib = scrawl_core::Document::default();
    lib.add_swatch(scrawl_core::Color::rgba(1.0, 0.0, 0.0, 1.0));
    scrawl_core::io::save(&lib, &lib_path).unwrap();
    let app = h.state_mut().front_mut();
    app.add_library_path(&lib_path);
    let id = app.doc.libraries[0].id;
    assert!(app.check_libraries(1000.0).is_empty(), "nothing changed yet");
    // Edit the library elsewhere (a later modification time).
    std::thread::sleep(std::time::Duration::from_millis(20));
    lib.swatches[0].color = scrawl_core::Color::rgba(0.0, 1.0, 0.0, 1.0);
    scrawl_core::io::save(&lib, &lib_path).unwrap();
    let file = std::fs::File::options().write(true).open(&lib_path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
    let app = h.state_mut().front_mut();
    assert_eq!(app.check_libraries(2000.0), ["Lib"]);
    assert_eq!(app.library_doc(id).unwrap().swatches[0].color, scrawl_core::Color::rgba(0.0, 1.0, 0.0, 1.0));

    // A gradient stop linked to a local variable recolors when the variable does.
    let header = layer_named(&h, "Header").id;
    let app = h.state_mut().front_mut();
    let var = app.doc.add_swatch(scrawl_core::Color::rgba(0.2, 0.2, 0.2, 1.0));
    if let Some(l) = app.page_mut().find_mut(header) {
        let stop = &mut l.style.fills[0].gradient.stops[0];
        stop.swatch = Some(var);
        stop.color = scrawl_core::Color::rgba(0.2, 0.2, 0.2, 1.0);
    }
    let blue = scrawl_core::Color::rgba(0.0, 0.0, 1.0, 1.0);
    app.doc.set_swatch_color(var, blue);
    let stop = &h.state().front().page().find(header).unwrap().style.fills[0].gradient.stops[0];
    assert_eq!((stop.color, stop.swatch), (blue, Some(var)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn preview_overlays_open_and_close() {
    let Some(mut h) = harness() else { return };
    h.run();
    let card = layer_named(&h, "Card").id;
    let (phone, buttons) = (layer_named(&h, "iPhone 16").id, layer_named(&h, "Buttons").id);
    if let Some(l) = h.state_mut().front_mut().page_mut().find_mut(card) {
        let link = l.flow.as_mut().unwrap();
        link.overlay = true;
        link.animation = scrawl_core::prototype::Animation::Dissolve;
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::P);
    h.run_steps(3);
    let card_at = preview_pos(&h, 200.0, 300.0);
    click_at(&mut h, card_at);
    h.run_steps(20);
    let player = &h.state().front().preview.as_ref().unwrap().player;
    assert_eq!((player.current, player.overlay.map(|o| o.0)), (phone, Some(buttons)));
    save(&mut h, "preview_overlay");
    // A click outside the overlay (near the bottom) closes it.
    let outside = preview_pos(&h, 200.0, 830.0);
    click_at(&mut h, outside);
    h.run_steps(5);
    let player = &h.state().front().preview.as_ref().unwrap().player;
    assert!(player.overlay.is_none());
    assert_eq!(player.current, phone);
}

#[test]
fn blurs_from_the_inspector_render_on_the_canvas() {
    let Some(mut h) = harness() else { return };
    h.run();
    let star = select(&mut h, "Star");
    h.get_by_label("Add blur").click();
    h.run();
    let blur = h.state().front().page().find(star).unwrap().style.blur.expect("added");
    assert_eq!((blur.kind, blur.enabled), (scrawl_core::BlurKind::Layer, true));
    // Frost the card over the stripes behind it.
    let card = layer_named(&h, "Card").id;
    if let Some(l) = h.state_mut().front_mut().page_mut().find_mut(card) {
        l.style.fills = vec![scrawl_core::Fill::solid(scrawl_core::Color::rgba(1.0, 1.0, 1.0, 0.4))];
        l.style.blur = Some(scrawl_core::Blur { kind: scrawl_core::BlurKind::Background, ..Default::default() });
        l.frame.y = 420.0;
    }
    h.run();
    assert!(h.query_by_label("BLUR").is_some());
    save(&mut h, "blurs");
}
