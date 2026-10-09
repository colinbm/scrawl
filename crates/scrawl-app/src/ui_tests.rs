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
