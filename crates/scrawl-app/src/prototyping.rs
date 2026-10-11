//! Prototyping on the canvas: picking a link's destination artboard (W, or after drawing a
//! hotspot), and drawing hotspots and link connections.

use egui::{CursorIcon, Painter, Pos2, Rect as ERect, Stroke, StrokeKind, vec2};
use scrawl_core::kurbo::{CubicBez, Point, Rect, Shape};
use scrawl_core::prototype::{self, Destination, FlowLink};
use scrawl_core::{LayerId, LayerKind, Page};

use crate::app::ScrawlApp;
use crate::canvas::{screen_transform, to_pos};
use crate::preview::PROTOTYPE;

/// Starts picking a destination for the selection's links. Artboards can't link anywhere.
pub fn start_linking(app: &mut ScrawlApp) {
    let page = app.page();
    let sources: Vec<LayerId> = app.selection.iter().copied().filter(|&id| page.find(id).is_some_and(|l| !l.is_artboard())).collect();
    if sources.is_empty() {
        app.notify("Select a layer inside an artboard to add a link");
        return;
    }
    app.canvas.linking = Some(sources);
}

/// Sets (or with `None`, removes) the link of each of `ids`, keeping their animations.
pub fn set_links(app: &mut ScrawlApp, ids: &[LayerId], destination: Option<Destination>) {
    let page = app.page_mut();
    for &id in ids {
        if let Some(l) = page.find_mut(id) {
            let keep = l.flow;
            l.flow = destination.map(|destination| FlowLink { destination, ..keep.unwrap_or(FlowLink::back()) });
        }
    }
}

/// The artboard a link being picked would go to, under the pointer at `screen`.
fn target_at(app: &ScrawlApp, screen: Pos2, rect: ERect) -> Option<LayerId> {
    let sources = app.canvas.linking.as_ref()?;
    let page = app.page();
    let p = app.viewport().to_page(rect, screen);
    let target = crate::canvas::title_hit(app, screen).or_else(|| page.artboard_at(p))?;
    // A layer can't link to its own artboard, nor to a symbol master.
    let own = sources.iter().any(|&s| prototype::artboard_of(page, s) == Some(target));
    let master = page.find(target).is_some_and(|l| l.master_of().is_some());
    (!own && !master).then_some(target)
}

/// Pointer handling while picking a destination: a click on an artboard links to it, anywhere
/// else cancels.
pub fn link_pointer(app: &mut ScrawlApp, ui: &egui::Ui, response: &egui::Response, rect: ERect) {
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Alias);
    }
    if !response.clicked() {
        return;
    }
    let target = response.interact_pointer_pos().and_then(|p| target_at(app, p, rect));
    let Some(sources) = app.canvas.linking.take() else { return };
    if let Some(target) = target {
        set_links(app, &sources, Some(Destination::Artboard(target)));
        let name = app.page().find(target).map_or_else(String::new, |l| l.name.clone());
        app.notify(format!("Linked to {name}"));
    }
}

/// Hotspots, flow start markers, link connections for the selection (or every link, when shown),
/// and the link being picked.
pub fn paint(app: &ScrawlApp, p: &Painter, rect: ERect, hover: Option<Pos2>) {
    let page = app.page();
    let to_screen = screen_transform(app, rect);

    let mut hotspots = vec![];
    page.walk(|l| {
        if matches!(l.kind, LayerKind::Hotspot) && l.visible {
            hotspots.push(l.id);
        }
    });
    for id in hotspots {
        let (Some(layer), Some(t)) = (page.find(id), page.world_transform(id)) else { continue };
        let outline = crate::canvas::polyline_points(&layer.frame.local_rect().to_path(0.1), to_screen * t);
        p.add(egui::Shape::convex_polygon(outline.clone(), PROTOTYPE.gamma_multiply(0.12), Stroke::NONE));
        let mut closed = outline;
        closed.extend(closed.first().copied());
        p.extend(egui::Shape::dashed_line(&closed, Stroke::new(1.0, PROTOTYPE), 4.0, 3.0));
    }

    // Links of the selection and everything in it, or of the whole page.
    let mut shown: Vec<(LayerId, FlowLink)> = prototype::links(page);
    if !app.show_links {
        let selected = |id: LayerId| app.selection.iter().any(|&s| s == id || page.ancestors(id).is_some_and(|a| a.contains(&s)));
        shown.retain(|&(id, _)| selected(id));
    }
    for (id, link) in shown {
        if let Some((from, to)) = prototype::connection(page, id, &link) {
            // Links that don't land on this page get a hook with where they go.
            let label = match link.destination {
                Destination::Back => "Back".to_owned(),
                Destination::Artboard(target) => prototype::find_artboard(&app.doc, target)
                    .and_then(|(pg, ab)| Some(format!("{} / {}", app.doc.page(pg)?.name, ab.name)))
                    .unwrap_or_else(|| "Missing".into()),
            };
            connection(p, from, to, to_screen, &label);
        }
    }

    // The link being picked follows the pointer, and highlights the artboard it would go to.
    if let Some(sources) = &app.canvas.linking {
        let Some(from) = page.selection_bounds(sources) else { return };
        let Some(pointer) = hover else { return };
        let target = target_at(app, pointer, rect).and_then(|t| page.world_bounds(t));
        if let Some(t) = target {
            let r = ERect::from_two_pos(to_pos(to_screen * t.origin()), to_pos(to_screen * Point::new(t.x1, t.y1)));
            p.rect_stroke(r, 0.0, Stroke::new(2.0, PROTOTYPE), StrokeKind::Outside);
            connection(p, from, Some(t), to_screen, "");
        } else {
            let end = app.viewport().to_page(rect, pointer);
            connection(p, from, Some(Rect::from_points(end, end)), to_screen, "");
        }
    }
}

/// A curve from the side of `from` facing the destination to the near side of `to`, with an
/// arrowhead. Links back get a short hook pointing left.
fn connection(p: &Painter, from: Rect, to: Option<Rect>, to_screen: scrawl_core::kurbo::Affine, label: &str) {
    let stroke = Stroke::new(1.5, PROTOTYPE);
    let Some(to) = to else {
        let start = to_screen * Point::new(from.x0, from.center().y);
        let s = to_pos(start);
        let end = s - vec2(28.0, 0.0);
        p.line_segment([s, end], stroke);
        arrowhead(p, end, vec2(-1.0, 0.0));
        p.circle_filled(s, 3.0, PROTOTYPE);
        p.text(end - vec2(4.0, 0.0), egui::Align2::RIGHT_CENTER, label, egui::FontId::proportional(10.0), PROTOTYPE);
        return;
    };
    let rightwards = to.center().x >= from.center().x;
    let (a, b) = if rightwards {
        (Point::new(from.x1, from.center().y), Point::new(to.x0, to.center().y))
    } else {
        (Point::new(from.x0, from.center().y), Point::new(to.x1, to.center().y))
    };
    let (a, b) = (to_screen * a, to_screen * b);
    let reach = ((b.x - a.x).abs() / 2.0).max(30.0) * if rightwards { 1.0 } else { -1.0 };
    let curve = CubicBez::new(a, Point::new(a.x + reach, a.y), Point::new(b.x - reach, b.y), b);
    let pts: Vec<Pos2> = (0..=24).map(|i| to_pos(scrawl_core::kurbo::ParamCurve::eval(&curve, i as f64 / 24.0))).collect();
    p.add(egui::Shape::line(pts, stroke));
    p.circle_filled(to_pos(a), 3.0, PROTOTYPE);
    arrowhead(p, to_pos(b), vec2(if rightwards { 1.0 } else { -1.0 }, 0.0));
}

fn arrowhead(p: &Painter, tip: Pos2, dir: egui::Vec2) {
    let back = tip - dir * 8.0;
    let side = vec2(-dir.y, dir.x) * 4.5;
    p.add(egui::Shape::convex_polygon(vec![tip, back + side, back - side], PROTOTYPE, Stroke::NONE));
}

/// Whether `page` has any link going to `artboard`, for the inspector.
pub fn incoming(page: &Page, artboard: LayerId) -> usize {
    prototype::links(page).iter().filter(|(_, l)| l.destination == Destination::Artboard(artboard)).count()
}
