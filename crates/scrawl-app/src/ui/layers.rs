//! Left sidebar: pages and the layer tree.

use egui::{FontId, Id, Rect, Sense, Stroke, Ui, pos2, vec2};
use scrawl_core::{Layer, LayerId, LayerKind, PageId, ops};

use crate::app::{RenameTarget, ScrawlApp};
use crate::style::ACCENT;
use crate::ui::icons::{self, Icon};

const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;

struct Row {
    id: LayerId,
    depth: usize,
    name: String,
    icon: Icon,
    container: bool,
    expanded: bool,
    visible: bool,
    locked: bool,
    /// Shown only as the ancestor of a search match.
    dim: bool,
}

/// Which layer types the layer search shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KindFilter {
    #[default]
    All,
    Text,
    Shapes,
    Groups,
    Artboards,
    Symbols,
}

impl KindFilter {
    const ALL: [KindFilter; 6] =
        [KindFilter::All, KindFilter::Text, KindFilter::Shapes, KindFilter::Groups, KindFilter::Artboards, KindFilter::Symbols];

    fn label(self) -> &'static str {
        match self {
            KindFilter::All => "All",
            KindFilter::Text => "Text",
            KindFilter::Shapes => "Shapes",
            KindFilter::Groups => "Groups",
            KindFilter::Artboards => "Artboards",
            KindFilter::Symbols => "Symbols",
        }
    }

    fn accepts(self, kind: &LayerKind) -> bool {
        match self {
            KindFilter::All => true,
            KindFilter::Text => matches!(kind, LayerKind::Text(_)),
            KindFilter::Groups => matches!(kind, LayerKind::Group(_)),
            KindFilter::Artboards => matches!(kind, LayerKind::Artboard(_)),
            KindFilter::Symbols => matches!(kind, LayerKind::Symbol(_)) || matches!(kind, LayerKind::Artboard(a) if a.symbol_id.is_some()),
            KindFilter::Shapes => {
                !matches!(kind, LayerKind::Text(_) | LayerKind::Group(_) | LayerKind::Artboard(_) | LayerKind::Symbol(_) | LayerKind::Image(_))
            }
        }
    }
}

/// The layer list's search state.
#[derive(Clone, Debug, Default)]
pub struct LayerFilter {
    pub query: String,
    pub kind: KindFilter,
    /// Focus the search field on the next frame (⌘F).
    pub focus: bool,
}

impl LayerFilter {
    pub fn active(&self) -> bool {
        !self.query.trim().is_empty() || self.kind != KindFilter::All
    }

    /// Whether `l` matches by name (or text content) and type.
    pub fn matches(&self, l: &Layer) -> bool {
        let q = self.query.trim().to_lowercase();
        let text = match &l.kind {
            LayerKind::Text(t) => t.text.to_lowercase().contains(&q),
            _ => false,
        };
        self.kind.accepts(&l.kind) && (q.is_empty() || l.name.to_lowercase().contains(&q) || text)
    }
}

enum Action {
    Select { id: LayerId, toggle: bool },
    ToggleExpanded(LayerId),
    ToggleVisible(LayerId),
    ToggleLocked(LayerId),
    StartRename(LayerId),
    Drop { id: LayerId, target: DropTarget },
}

#[derive(Clone, Copy)]
enum DropTarget {
    /// Directly above `0` in the list (i.e. in front of it).
    Above(LayerId),
    /// Directly below `0` in the list (i.e. behind it).
    Below(LayerId),
    /// Inside container `0`, at the top.
    Into(LayerId),
}

pub fn show(app: &mut ScrawlApp, ui: &mut Ui) {
    pages_section(app, ui);
    ui.separator();
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("LAYERS").small().strong().weak());
    });
    search_bar(app, ui);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| layer_tree(app, ui));
}

fn pages_section(app: &mut ScrawlApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("PAGES").small().strong().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small_button(ui, Icon::Plus, "Add Page").clicked() {
                app.add_page();
            }
        });
    });
    let pages: Vec<(PageId, String)> = app.doc.pages.iter().map(|p| (p.id, p.name.clone())).collect();
    let can_delete = pages.len() > 1;
    for (id, name) in pages {
        if let Some((RenameTarget::Page(pid), buf)) = &mut app.renaming
            && *pid == id
        {
            if let Some(new_name) = rename_field(ui, buf) {
                if let (Some(n), Some(page)) = (new_name, app.doc.page_mut(id)) {
                    page.name = n;
                }
                app.renaming = None;
            }
            continue;
        }
        let resp = ui.add_sized([ui.available_width(), ROW_HEIGHT - 2.0], egui::Button::selectable(app.page == id, name.as_str()).frame_when_inactive(false));
        if resp.clicked() {
            app.set_page(id);
        }
        if resp.double_clicked() {
            app.renaming = Some((RenameTarget::Page(id), name.clone()));
        }
        resp.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                app.renaming = Some((RenameTarget::Page(id), name.clone()));
            }
            if ui.add_enabled(can_delete, egui::Button::new("Delete Page")).clicked() {
                app.delete_page(id);
            }
        });
    }
}

/// A focused single-line editor. Returns `Some(Some(name))` to commit, `Some(None)` to cancel.
fn rename_field(ui: &mut Ui, buf: &mut String) -> Option<Option<String>> {
    let resp = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY));
    if !resp.has_focus() && !resp.lost_focus() {
        resp.request_focus();
        // Select all on first show so typing replaces the name.
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(buf.chars().count()),
            )));
            state.store(ui.ctx(), resp.id);
        }
    }
    if resp.lost_focus() {
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let name = buf.trim().to_owned();
        return Some((!cancelled && !name.is_empty()).then_some(name));
    }
    None
}

fn search_bar(app: &mut ScrawlApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let f = &mut app.layer_filter;
        let resp = ui.add(
            egui::TextEdit::singleline(&mut f.query).hint_text("Search layers").desired_width(ui.available_width() - 70.0),
        );
        if std::mem::take(&mut f.focus) {
            resp.request_focus();
        }
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            f.query.clear();
            resp.surrender_focus();
        }
        egui::ComboBox::from_id_salt("layer_kind_filter").selected_text(f.kind.label()).width(60.0).show_ui(ui, |ui| {
            for k in KindFilter::ALL {
                ui.selectable_value(&mut f.kind, k, k.label());
            }
        });
    });
    if app.layer_filter.active() {
        let mut matches = vec![];
        app.page().walk(|l| {
            if app.layer_filter.matches(l) {
                matches.push(l.id);
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.weak(match matches.len() {
                1 => "1 match".to_owned(),
                n => format!("{n} matches"),
            });
            if !matches.is_empty() && ui.small_button("Select All").clicked() {
                app.selection = ops::normalize_selection(app.page(), &matches);
            }
            if ui.small_button("Clear").clicked() {
                app.layer_filter = LayerFilter::default();
            }
        });
    }
}

/// Rows for layers matching the search, with their ancestors (dimmed) for context. Returns
/// whether anything in `layers` matched.
fn collect_filtered(layers: &[Layer], depth: usize, filter: &LayerFilter, out: &mut Vec<Row>) -> bool {
    let mut any = false;
    for l in layers.iter().rev() {
        let mut below = vec![];
        let inner = l.children().is_some_and(|c| collect_filtered(c, depth + 1, filter, &mut below));
        let hit = filter.matches(l);
        if hit || inner {
            out.push(Row {
                id: l.id,
                depth,
                name: l.name.clone(),
                icon: if l.is_mask { Icon::Mask } else { icons::for_layer(&l.kind) },
                container: inner,
                expanded: inner,
                visible: l.visible,
                locked: l.locked,
                dim: !hit,
            });
            out.append(&mut below);
            any = true;
        }
    }
    any
}

fn collect_rows(layers: &[Layer], depth: usize, app: &ScrawlApp, out: &mut Vec<Row>) {
    for l in layers.iter().rev() {
        let container = l.children().is_some_and(|c| !c.is_empty());
        let expanded = container && app.expanded.contains(&l.id);
        out.push(Row {
            id: l.id,
            depth,
            name: l.name.clone(),
            icon: if l.is_mask { Icon::Mask } else { icons::for_layer(&l.kind) },
            container,
            expanded,
            visible: l.visible,
            locked: l.locked,
            dim: false,
        });
        if expanded {
            collect_rows(l.children().expect("container"), depth + 1, app, out);
        }
    }
}

fn layer_tree(app: &mut ScrawlApp, ui: &mut Ui) {
    // Reveal newly selected layers by expanding their ancestors.
    let reveal_id = Id::new("layers_revealed_selection");
    let last: Vec<LayerId> = ui.data(|d| d.get_temp(reveal_id)).unwrap_or_default();
    if last != app.selection {
        for &id in &app.selection {
            app.expanded.extend(app.page().ancestors(id).unwrap_or_default());
        }
        ui.data_mut(|d| d.insert_temp(reveal_id, app.selection.clone()));
    }

    let mut rows = vec![];
    let filtering = app.layer_filter.active();
    if filtering {
        collect_filtered(&app.page().layers, 0, &app.layer_filter, &mut rows);
        if rows.is_empty() {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.weak("No matching layers"));
            return;
        }
    } else {
        collect_rows(&app.page().layers, 0, app, &mut rows);
    }
    if rows.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| ui.weak("Press A to draw an artboard,\nor R for a rectangle."));
        return;
    }

    let drag_id = Id::new("layer_drag");
    let dragging: Option<LayerId> = ui.data(|d| d.get_temp(drag_id));
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut actions = vec![];
    let mut drop: Option<(DropTarget, Rect, f32)> = None;
    let mut drag_stopped: Option<LayerId> = None;
    let visuals = ui.visuals().clone();

    for row in &rows {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click_and_drag());
        let selected = app.selection.contains(&row.id);
        let hovered_canvas = app.canvas.hover == Some(row.id);
        let p = ui.painter().clone();
        let p = &p;
        if selected {
            p.rect_filled(rect, 0.0, visuals.selection.bg_fill);
        } else if resp.hovered() || hovered_canvas {
            p.rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.0, ACCENT.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        }
        let text_color = if !row.visible || row.dim { visuals.weak_text_color() } else if selected { visuals.selection.stroke.color } else { visuals.text_color() };

        let x = rect.left() + 6.0 + row.depth as f32 * INDENT;
        let disclosure = Rect::from_min_size(pos2(x, rect.top()), vec2(12.0, ROW_HEIGHT));
        if row.container {
            let c = disclosure.center();
            let pts = if row.expanded {
                vec![c + vec2(-3.5, -2.0), c + vec2(3.5, -2.0), c + vec2(0.0, 2.5)]
            } else {
                vec![c + vec2(-2.0, -3.5), c + vec2(2.5, 0.0), c + vec2(-2.0, 3.5)]
            };
            p.add(egui::Shape::convex_polygon(pts, visuals.weak_text_color(), Stroke::NONE));
        }
        let icon_rect = Rect::from_center_size(pos2(x + 22.0, rect.center().y), vec2(14.0, 14.0));
        icons::paint(p, icon_rect, row.icon, text_color);

        let name_x = x + 34.0;
        let renaming_this = matches!(&app.renaming, Some((RenameTarget::Layer(id), _)) if *id == row.id);
        if renaming_this {
            let edit_rect = Rect::from_min_max(pos2(name_x - 4.0, rect.top() + 1.0), pos2(rect.right() - 4.0, rect.bottom() - 1.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect));
            if let Some((_, buf)) = &mut app.renaming
                && let Some(result) = rename_field(&mut child, buf)
            {
                if let (Some(name), Some(l)) = (result, app.page_mut().find_mut(row.id)) {
                    l.name = name;
                }
                app.renaming = None;
            }
        } else {
            let galley = p.layout_no_wrap(row.name.clone(), FontId::proportional(13.0), text_color);
            p.galley(pos2(name_x, rect.center().y - galley.size().y / 2.0), galley, text_color);
        }

        // Trailing toggles: always shown when non-default, otherwise on hover.
        let eye = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(18.0, 18.0));
        let lock = eye.translate(vec2(-20.0, 0.0));
        let row_hovered = resp.hovered() || pointer.is_some_and(|p| rect.contains(p));
        if !row.visible || row_hovered {
            let icon = if row.visible { Icon::Eye } else { Icon::EyeClosed };
            icons::paint(p, eye.shrink(2.0), icon, visuals.weak_text_color());
        }
        if row.locked || row_hovered {
            let color = if row.locked { visuals.text_color() } else { visuals.weak_text_color().gamma_multiply(0.5) };
            icons::paint(p, lock.shrink(3.0), Icon::Lock, color);
        }

        if resp.clicked() {
            let pos = resp.interact_pointer_pos().unwrap_or_default();
            let mods = ui.input(|i| i.modifiers);
            if eye.contains(pos) {
                actions.push(Action::ToggleVisible(row.id));
            } else if lock.contains(pos) {
                actions.push(Action::ToggleLocked(row.id));
            } else if row.container && !filtering && disclosure.expand2(vec2(4.0, 0.0)).contains(pos) {
                actions.push(Action::ToggleExpanded(row.id));
            } else {
                actions.push(Action::Select { id: row.id, toggle: mods.shift || mods.command });
            }
        }
        if resp.double_clicked() {
            actions.push(Action::StartRename(row.id));
        }
        // Reordering needs the full tree, so it's off while searching.
        if resp.drag_started() && !filtering {
            ui.data_mut(|d| d.insert_temp(drag_id, row.id));
        }

        // Drop target under the pointer while dragging another row.
        if let (Some(src), Some(ptr)) = (dragging, pointer)
            && src != row.id
            && rect.contains(ptr)
        {
            let t = (ptr.y - rect.top()) / ROW_HEIGHT;
            let x0 = name_x - 12.0;
            let target = if row.container && (0.25..0.75).contains(&t) {
                (DropTarget::Into(row.id), rect, 0.0)
            } else if t < 0.5 {
                (DropTarget::Above(row.id), rect, x0)
            } else if row.expanded {
                (DropTarget::Into(row.id), rect, 0.0)
            } else {
                (DropTarget::Below(row.id), rect, x0)
            };
            drop = Some(target);
        }
        if resp.drag_stopped() {
            drag_stopped = Some(row.id);
        }
    }

    // Resolved after the loop because the target row may come after the dragged one.
    if let Some(id) = drag_stopped {
        ui.data_mut(|d| d.remove::<LayerId>(drag_id));
        if let Some((target, _, _)) = drop {
            actions.push(Action::Drop { id, target });
        }
    }

    if let Some((target, rect, x0)) = drop {
        let p = ui.painter();
        let stroke = Stroke::new(2.0, ACCENT);
        match target {
            DropTarget::Into(_) => {
                p.rect_stroke(rect.shrink(1.0), 2.0, stroke, egui::StrokeKind::Inside);
            }
            DropTarget::Above(_) => {
                p.line_segment([pos2(x0, rect.top()), pos2(rect.right(), rect.top())], stroke);
            }
            DropTarget::Below(_) => {
                p.line_segment([pos2(x0, rect.bottom()), pos2(rect.right(), rect.bottom())], stroke);
            }
        }
    }
    if dragging.is_some() && !ui.input(|i| i.pointer.any_down()) {
        ui.data_mut(|d| d.remove::<LayerId>(drag_id));
    }

    for action in actions {
        apply(app, action);
    }
}

fn apply(app: &mut ScrawlApp, action: Action) {
    match action {
        Action::Select { id, toggle } => {
            if toggle {
                if let Some(i) = app.selection.iter().position(|&s| s == id) {
                    app.selection.remove(i);
                } else {
                    app.selection.push(id);
                }
            } else {
                app.selection = vec![id];
            }
        }
        Action::ToggleExpanded(id) => {
            if !app.expanded.remove(&id) {
                app.expanded.insert(id);
            }
        }
        Action::ToggleVisible(id) => {
            if let Some(l) = app.page_mut().find_mut(id) {
                l.visible = !l.visible;
            }
        }
        Action::ToggleLocked(id) => {
            if let Some(l) = app.page_mut().find_mut(id) {
                l.locked = !l.locked;
            }
            app.selection.retain(|&s| s != id);
        }
        Action::StartRename(id) => {
            if let Some(l) = app.page().find(id) {
                app.renaming = Some((RenameTarget::Layer(id), l.name.clone()));
            }
        }
        Action::Drop { id, target } => {
            let page = app.page_mut();
            let ok = match target {
                DropTarget::Into(parent) => {
                    let len = page.find(parent).and_then(|p| p.children()).map_or(0, Vec::len);
                    ops::move_to(page, id, Some(parent), len)
                }
                DropTarget::Above(other) | DropTarget::Below(other) => match page.location(other) {
                    Some(loc) => {
                        let index = if matches!(target, DropTarget::Above(_)) { loc.index + 1 } else { loc.index };
                        ops::move_to(page, id, loc.parent, index)
                    }
                    None => false,
                },
            };
            if ok {
                app.selection = vec![id];
                if let DropTarget::Into(parent) = target {
                    app.expanded.insert(parent);
                }
            }
        }
    }
}
