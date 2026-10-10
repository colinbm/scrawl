//! The prototype preview: a separate window showing one artboard at a time, following links when
//! their layers are clicked, with slide transitions and vertical scrolling.

use egui::{Color32, CursorIcon, Pos2, Rect as ERect, Sense, Stroke, vec2};
use scrawl_core::kurbo::{Affine, Point, Rect, Vec2};
use scrawl_core::prototype::{self, Player};
use scrawl_core::{Layer, LayerId, PageId};

use crate::app::ScrawlApp;
use crate::gpu::Slot;
use crate::ui::icons::{self, Icon};

/// The color of prototyping links, hotspots and start points.
pub const PROTOTYPE: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x00);

/// How long link areas flash after a click that hit none, in seconds.
const FLASH_SECONDS: f64 = 0.6;

/// The preview window never opens taller than this, in points.
const MAX_OPEN_HEIGHT: f64 = 860.0;

const BAR_HEIGHT: f32 = 34.0;

pub struct Preview {
    pub page: PageId,
    pub player: Player,
    /// Outline every link area.
    pub show_hotspots: bool,
    /// When link areas started flashing (after a click on nothing), in egui time.
    flash: Option<f64>,
    /// The artboard size the window was last sized for.
    sized_for: Option<(f64, f64)>,
    scene: vello::Scene,
    /// The artboard area on screen and its scale, from the last frame.
    pub view: Option<(ERect, f64)>,
}

impl Preview {
    pub fn new(page: PageId, start: LayerId) -> Self {
        Self {
            page,
            player: Player::new(start),
            show_hotspots: false,
            flash: None,
            sized_for: None,
            scene: vello::Scene::new(),
            view: None,
        }
    }
}

/// Opens the preview at the artboard holding the selection (or the flow's start).
pub fn open(app: &mut ScrawlApp) {
    let page = app.page();
    match prototype::start_artboard(page, &app.selection) {
        Some(start) => app.preview = Some(Preview::new(app.page, start)),
        None => app.error = Some("Add an artboard to this page to preview it.".into()),
    }
}

/// Shows the preview window, if open.
pub fn show(app: &mut ScrawlApp, ctx: &egui::Context) {
    let Some(preview) = &mut app.preview else { return };
    let Some(page) = app.doc.page(preview.page) else {
        app.preview = None;
        return;
    };
    // Edits in the document window may have removed artboards.
    if !preview.player.sanitize(page) {
        match prototype::start_artboard(page, &[]) {
            Some(start) => preview.player = Player::new(start),
            None => {
                app.preview = None;
                return;
            }
        }
    }
    let Some(artboard) = page.find(preview.player.current) else { return };
    let size = (artboard.frame.width, artboard.frame.height);
    let title = format!("{} — Preview", artboard.name);
    let inner = [size.0 as f32, size.1.min(MAX_OPEN_HEIGHT) as f32 + BAR_HEIGHT];
    let builder = egui::ViewportBuilder::default().with_title(title).with_inner_size(inner).with_min_inner_size([240.0, 200.0]);
    let resize = preview.sized_for.is_some_and(|s| s != size);
    preview.sized_for = Some(size);
    let id = egui::ViewportId::from_hash_of(("scrawl-preview", app.uid));
    ctx.show_viewport_immediate(id, builder, |ui, class| {
        if resize && class != egui::ViewportClass::EmbeddedWindow {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(inner.into()));
        }
        contents(app, ui);
    });
}

fn contents(app: &mut ScrawlApp, ui: &mut egui::Ui) {
    let close = ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape));
    if close {
        app.preview = None;
        return;
    }
    let now = ui.input(|i| i.time);
    egui::Panel::top("preview_bar").exact_size(BAR_HEIGHT).show(ui, |ui| bar(app, ui, now));
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_gray(24))).show(ui, |ui| stage(app, ui, now));
}

fn bar(app: &mut ScrawlApp, ui: &mut egui::Ui, now: f64) {
    let Some(preview) = &mut app.preview else { return };
    let Some(page) = app.doc.page(preview.page) else { return };
    ui.horizontal_centered(|ui| {
        let back = ui.add_enabled(preview.player.can_go_back(), egui::Button::new("‹ Back")).on_hover_text("Go back (←)");
        let back_key = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
        if back.clicked() || back_key {
            preview.player.back(now);
        }
        let current = page.find(preview.player.current).map_or_else(String::new, |l| l.name.clone());
        egui::ComboBox::from_id_salt("preview_artboard").selected_text(current).width(180.0).show_ui(ui, |ui| {
            for ab in prototype::artboards(page) {
                let start = matches!(&ab.kind, scrawl_core::LayerKind::Artboard(a) if a.flow_home);
                let label = if start { format!("▶ {}", ab.name) } else { ab.name.clone() };
                if ui.selectable_label(ab.id == preview.player.current, label).clicked() {
                    preview.player.jump(ab.id);
                    preview.player.start = ab.id;
                }
            }
        });
        if ui.button("Restart").on_hover_text("Back to where this preview started").clicked() {
            preview.player.restart();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, Icon::Hotspot, preview.show_hotspots, "Show Hotspots").clicked() {
                preview.show_hotspots ^= true;
            }
        });
    });
}

/// The artboard (or the two artboards of a transition), clicks on links, and scrolling.
fn stage(app: &mut ScrawlApp, ui: &mut egui::Ui, now: f64) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click());
    let Some(rs) = app.ensure_gpu() else { return };
    let Some(preview) = &mut app.preview else { return };
    let Some(page) = app.doc.page(preview.page) else { return };
    let Some(artboard) = page.find(preview.player.current) else { return };

    // Fit the artboard's width; taller artboards scroll.
    let (w, h) = (artboard.frame.width.max(1.0), artboard.frame.height.max(1.0));
    let scale = (rect.width() as f64 / w).min(1.0);
    let view_h = (rect.height() as f64 / scale).min(h);
    let origin = Pos2::new(rect.center().x - (w * scale / 2.0) as f32, rect.top());
    let area = ERect::from_min_size(origin, vec2((w * scale) as f32, (view_h * scale) as f32));
    preview.view = Some((area, scale));

    let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
    if response.hovered() && scroll != 0.0 && preview.player.transition.is_none() {
        let max = prototype::max_scroll(artboard, view_h);
        preview.player.scroll = (preview.player.scroll - scroll / scale).clamp(0.0, max);
    }
    let to_viewport = |pos: Pos2| Point::new(((pos.x - origin.x) / scale as f32) as f64, ((pos.y - origin.y) / scale as f32) as f64);

    // Clicks follow links; a click on nothing flashes the link areas.
    let animating = preview.player.tick(now);
    let hovered_link = response.hover_pos().and_then(|p| prototype::link_at(artboard, to_viewport(p), preview.player.scroll));
    if !animating && hovered_link.is_some_and(|l| prototype::resolves(page, &l)) {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if response.clicked() && !animating {
        match response.interact_pointer_pos().and_then(|p| prototype::link_at(artboard, to_viewport(p), preview.player.scroll)) {
            Some(link) if preview.player.follow(page, link, now) => {}
            _ => preview.flash = Some(now),
        }
    }

    // Draw the current artboard, or both sides of a transition.
    let ppp = ui.ctx().pixels_per_point() as f64;
    let (pw, ph) = ((rect.width() as f64 * ppp).round() as u32, (rect.height() as f64 * ppp).round() as u32);
    let pixels = Affine::scale(ppp) * Affine::translate(((origin.x - rect.min.x) as f64, 0.0)) * Affine::scale(scale);
    let viewport = Vec2::new(w, view_h);
    let scene = &mut preview.scene;
    scene.reset();
    scene.push_clip_layer(vello::peniko::Fill::NonZero, pixels, &Rect::new(0.0, 0.0, w, view_h));
    match preview.player.transition {
        Some(t) if page.find(t.from).is_some() => {
            let frames = t.frames(now, viewport);
            let from = page.find(t.from).expect("checked");
            let mut sides = [(from, frames.from, 0.0), (artboard, frames.to, preview.player.scroll)];
            if frames.from_on_top {
                sides.reverse();
            }
            for (ab, offset, scroll) in sides {
                draw_artboard(scene, ab, pixels * Affine::translate(offset), scroll, &mut app.text, &app.images);
            }
            ui.ctx().request_repaint();
        }
        _ => draw_artboard(scene, artboard, pixels, preview.player.scroll, &mut app.text, &app.images),
    }
    scene.pop_layer();
    let gpu = app.gpu.as_mut().expect("ensured above");
    if let Some(tex) = gpu.render(&rs, Slot::Preview, scene, pw, ph, vello::peniko::Color::new([0.094, 0.094, 0.094, 1.0])) {
        let uv = ERect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        ui.painter().image(tex, rect, uv, Color32::WHITE);
    }

    // Link areas, when asked for or flashing.
    let flash = preview.flash.map(|t| now - t).filter(|&dt| dt < FLASH_SECONDS);
    if flash.is_some() {
        ui.ctx().request_repaint();
    }
    if (preview.show_hotspots || flash.is_some()) && preview.player.transition.is_none() {
        let alpha = flash.map_or(1.0, |dt| (1.0 - dt / FLASH_SECONDS) as f32);
        let to_screen = Affine::translate((origin.x as f64, origin.y as f64)) * Affine::scale(scale);
        let painter = ui.painter_at(area);
        for a in prototype::link_areas(artboard, preview.player.scroll) {
            let r = (to_screen * a.outline.clone()).control_box();
            let r = ERect::from_min_max(Pos2::new(r.x0 as f32, r.y0 as f32), Pos2::new(r.x1 as f32, r.y1 as f32));
            painter.rect(r, 2.0, PROTOTYPE.gamma_multiply(0.25 * alpha), Stroke::new(1.5, PROTOTYPE.gamma_multiply(alpha)), egui::StrokeKind::Inside);
        }
    }
}

/// Renders `artboard` with its top-left at the origin of `view`, scrolled by `scroll`. Children
/// fixed to the viewport don't scroll and draw on top.
fn draw_artboard(scene: &mut vello::Scene, artboard: &Layer, view: Affine, scroll: f64, text: &mut scrawl_render::TextEngine, images: &scrawl_render::ImageCache) {
    let mut flowing = artboard.clone();
    flowing.frame.x = 0.0;
    flowing.frame.y = -scroll;
    let mut fixed = vec![];
    if let Some(children) = flowing.children_mut() {
        let (f, rest): (Vec<Layer>, Vec<Layer>) = std::mem::take(children).into_iter().partition(|l| l.fixed_to_viewport);
        *children = rest;
        fixed = f;
    }
    flowing.rotation = 0.0;
    // Over white, so a see-through background doesn't show the other artboard of a transition.
    let backdrop = Rect::new(0.0, -scroll, artboard.frame.width, artboard.frame.height - scroll);
    scene.fill(vello::peniko::Fill::NonZero, view, vello::peniko::Color::WHITE, None, &backdrop);
    scrawl_render::render_layers(scene, std::slice::from_ref(&flowing), view, text, images);
    scrawl_render::render_layers(scene, &fixed, view, text, images);
}
