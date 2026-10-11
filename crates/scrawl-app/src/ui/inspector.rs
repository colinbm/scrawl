//! Right sidebar: properties of the selection.

use egui::{DragValue, Id, RichText, Ui};
use scrawl_core::kurbo::Rect;
use scrawl_core::kurbo::Point;
use scrawl_core::{
    Border, BorderOptions, BorderPosition, Color, Fill, FillRule, FillType, Layer, LayerKind, LineCap, LineJoin, Marker,
    PointMode, Shadow,
};

use scrawl_core::symbols::{OverridableKind, Symbols};
use scrawl_core::{OverrideValue, SymbolId, SymbolInstance};

use crate::app::ScrawlApp;
use crate::canvas::{GradientTarget, Tool};
use crate::ui::paint::{ImageTarget, PaintTarget, Palette, paint_button};
use crate::ui::icons::{self, Icon};
use crate::ui::toolbar::ALIGNMENTS;

const FIELD_WIDTH: f32 = 64.0;

/// (name, width, height)
type Preset = (&'static str, f64, f64);

const ARTBOARD_PRESETS: &[(&str, &[Preset])] = &[
    ("Apple", &[
        ("iPhone 16", 393.0, 852.0),
        ("iPhone 16 Pro Max", 440.0, 956.0),
        ("iPhone SE", 375.0, 667.0),
        ("iPad Pro 11\"", 834.0, 1194.0),
        ("iPad Pro 13\"", 1032.0, 1376.0),
        ("MacBook Air", 1280.0, 832.0),
    ]),
    ("Android", &[("Android Compact", 412.0, 917.0), ("Android Medium", 700.0, 840.0)]),
    ("Web", &[("Desktop", 1440.0, 1024.0), ("Desktop HD", 1920.0, 1080.0), ("Tablet", 768.0, 1024.0)]),
    ("Social", &[("Square Post", 1080.0, 1080.0), ("Story", 1080.0, 1920.0)]),
];

pub fn show(app: &mut ScrawlApp, ui: &mut Ui) {
    // A floating scroll bar would cover the add (+) buttons at the right edge.
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        alignment_row(app, ui);
        ui.separator();

        if app.tool == Tool::Artboard {
            artboard_presets(app, ui);
            return;
        }
        if app.canvas.edit.is_some() {
            points_section(app, ui);
            return;
        }
        if let Some(count) = app.canvas.rotate_copies.as_ref().map(|r| r.count) {
            ui.horizontal(|ui| {
                ui.strong("Rotate Copies");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Done").clicked() {
                        crate::rotate_copies::finish(app);
                    }
                });
            });
            ui.weak("Drag the pivot on the canvas to arrange the copies.");
            ui.horizontal(|ui| {
                let mut n = count as f64;
                if num(ui, "Copies", &mut n, 0.1, "") {
                    crate::rotate_copies::set_count(app, n.round().max(1.0) as u32);
                }
            });
            return;
        }
        match app.selection[..] {
            [] => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.weak("No selection"));
            }
            [id] => {
                let Some(original) = app.page().find(id).cloned() else { return };
                let mut layer = original.clone();
                layer_properties(app, ui, &mut layer);
                if layer != original {
                    // A hand-picked color stops following its color variable.
                    let doc = &app.doc;
                    scrawl_core::components::unlink_stale_colors(&mut layer.style, |s| doc.swatch(s).map(|s| s.color));
                    let page = app.page_mut();
                    if let Some(l) = page.find_mut(id) {
                        *l = layer;
                    }
                    if let Some(Some(parent)) = page.parent(id) {
                        page.refit_ancestor_groups(parent);
                    }
                }
            }
            _ => multi_selection(app, ui),
        }
    });
}

fn alignment_row(app: &mut ScrawlApp, ui: &mut Ui) {
    let enabled = !app.selection.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (icon, label, how) in ALIGNMENTS {
                if icons::button(ui, icon, false, label).clicked() {
                    app.align(how);
                }
            }
        });
    });
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title.to_uppercase()).small().strong().weak());
    ui.add_space(2.0);
}

/// A labelled numeric field. Returns whether the value changed.
fn num(ui: &mut Ui, label: &str, v: &mut f64, speed: f64, suffix: &str) -> bool {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let changed = ui
            .add_sized([FIELD_WIDTH, 20.0], DragValue::new(v).speed(speed).max_decimals(2).suffix(suffix))
            .changed();
        ui.label(RichText::new(label).small().weak());
        changed
    })
    .inner
}

fn layer_properties(app: &mut ScrawlApp, ui: &mut Ui, layer: &mut Layer) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        icons::paint(ui.painter(), rect, icons::for_layer(&layer.kind), ui.visuals().text_color());
        ui.strong(&layer.name);
        ui.weak(layer.kind.type_name());
    });

    section(ui, "Position");
    ui.horizontal(|ui| {
        num(ui, "X", &mut layer.frame.x, 1.0, "");
        num(ui, "Y", &mut layer.frame.y, 1.0, "");
        if !layer.is_artboard() {
            let mut deg = layer.rotation;
            if num(ui, "Rotate", &mut deg, 1.0, "°") {
                layer.rotation = deg.rem_euclid(360.0);
            }
        }
    });

    section(ui, "Size");
    ui.horizontal(|ui| {
        let (w0, h0) = (layer.frame.width, layer.frame.height);
        let (mut w, mut h) = (w0, h0);
        let mut changed = num(ui, "W", &mut w, 1.0, "");
        let lock_tip = if app.lock_aspect { "Unlock aspect ratio" } else { "Lock aspect ratio" };
        ui.vertical(|ui| {
            if icons::small_button(ui, if app.lock_aspect { Icon::Lock } else { Icon::Minus }, lock_tip).clicked() {
                app.lock_aspect = !app.lock_aspect;
            }
        });
        if app.lock_aspect && changed && w0 > 0.0 {
            h = h0 * w / w0;
        }
        if num(ui, "H", &mut h, 1.0, "") {
            changed = true;
            if app.lock_aspect && h0 > 0.0 {
                w = w0 * h / h0;
            }
        }
        if changed {
            // Reframe so groups scale their children, keeping the top-left fixed.
            layer.reframe(Rect::new(0.0, 0.0, w.max(0.0), h.max(0.0)));
        }
    });
    // Inside a stack: how this child sizes itself.
    let in_stack = app.page().parent(layer.id).flatten().and_then(|p| app.page().find(p)).is_some_and(|p| p.stack().is_some());
    if in_stack {
        section(ui, "In Stack");
        ui.horizontal(|ui| {
            ui.checkbox(&mut layer.stack_item.fill, "Fill across").on_hover_text("Stretch to the stack's thickness");
            ui.checkbox(&mut layer.stack_item.grow, "Grow").on_hover_text("Share a fixed-size stack's leftover length");
        });
    }
    // Constraints matter for anything inside a group or artboard.
    if app.page().parent(layer.id).flatten().is_some() {
        section(ui, "Resizing");
        resizing_widget(ui, &mut layer.resizing);
    }
    if !layer.is_artboard() {
        ui.horizontal(|ui| {
            if icons::button(ui, Icon::FlipH, layer.flip_horizontal, "Flip Horizontal").clicked() {
                layer.flip_horizontal = !layer.flip_horizontal;
            }
            if icons::button(ui, Icon::FlipV, layer.flip_vertical, "Flip Vertical").clicked() {
                layer.flip_vertical = !layer.flip_vertical;
            }
        });
    }

    match &mut layer.kind {
        LayerKind::Rectangle(rect) => {
            section(ui, "Corners");
            let mut independent = rect.corner_radii.is_some();
            if ui.checkbox(&mut independent, "Independent corners").changed() {
                rect.corner_radii = independent.then_some([rect.corner_radius; 4]);
            }
            ui.horizontal(|ui| match &mut rect.corner_radii {
                Some(radii) => {
                    for (r, label) in radii.iter_mut().zip(["TL", "TR", "BR", "BL"]) {
                        num(ui, label, r, 0.5, "");
                        *r = r.max(0.0);
                    }
                }
                None => {
                    num(ui, "Radius", &mut rect.corner_radius, 0.5, "");
                    rect.corner_radius = rect.corner_radius.max(0.0);
                }
            });
        }
        LayerKind::Polygon(poly) => {
            section(ui, "Polygon");
            ui.horizontal(|ui| {
                let mut sides = poly.sides as f64;
                if num(ui, "Sides", &mut sides, 0.1, "") {
                    poly.sides = sides.round().clamp(3.0, 100.0) as u32;
                }
            });
        }
        LayerKind::Text(text) => {
            let range = crate::text_edit::selected_range(app);
            text_section(ui, text, &mut app.text, range);
        }
        LayerKind::Group(g) if g.shape => {
            section(ui, "Boolean");
            if ui.button("Flatten").on_hover_text("Bake the shapes into a single path").clicked() {
                app.pending_flatten = true;
            }
        }
        LayerKind::Group(g) => stack_section(ui, g),
        LayerKind::Symbol(inst) => symbol_section(app, ui, layer.id, inst),
        LayerKind::Image(img) => image_section(app, ui, &mut layer.frame, img),
        LayerKind::Star(star) => {
            section(ui, "Star");
            ui.horizontal(|ui| {
                let mut points = star.points as f64;
                if num(ui, "Points", &mut points, 0.1, "") {
                    star.points = points.round().clamp(3.0, 100.0) as u32;
                }
                let mut pct = star.inner_ratio * 100.0;
                if num(ui, "Radius", &mut pct, 0.5, "%") {
                    star.inner_ratio = (pct / 100.0).clamp(0.01, 1.0);
                }
            });
        }
        _ => {}
    }

    if let LayerKind::Artboard(ab) = &mut layer.kind
        && let Some(symbol) = ab.symbol_id
    {
        master_section(app, ui, ab, symbol);
    }
    if let LayerKind::Artboard(ab) = &mut layer.kind {
        section(ui, "Artboard");
        let mut has_bg = ab.background.is_some();
        ui.horizontal(|ui| {
            if ui.checkbox(&mut has_bg, "Background").changed() {
                ab.background = has_bg.then_some(Color::WHITE);
            }
            if let Some(bg) = &mut ab.background {
                paint_button(ui, "artboard_bg", PaintTarget::color(bg), &mut Palette::new(&mut app.doc));
            }
        });
        ui.checkbox(&mut ab.clip_content, "Clip content");
        ui.checkbox(&mut ab.resizes_content, "Adjust content on resize")
            .on_hover_text("Resize children by their constraints when the artboard is resized");
        grid_settings(app, ui, ab);
    }

    // Inside a shape group, how this shape combines with the ones below it.
    let in_shape_group = app.page().parent(layer.id).flatten().and_then(|p| app.page().find(p)).is_some_and(Layer::is_shape_group);
    if in_shape_group {
        boolean_row(ui, &mut layer.boolean_op);
    }
    let hotspot = matches!(layer.kind, LayerKind::Hotspot);
    if !hotspot {
        appearance_section(ui, layer);
    }
    let slice = matches!(layer.kind, LayerKind::Slice | LayerKind::Hotspot);
    if !layer.is_artboard() && (!layer.is_group() || layer.is_shape_group()) && !slice {
        ui.checkbox(&mut layer.is_mask, "Use as mask").on_hover_text("Clips the layers above it in the same group");
        if layer.is_mask {
            ui.horizontal(|ui| {
                ui.label("Mask");
                for mode in scrawl_core::MaskMode::ALL {
                    ui.selectable_value(&mut layer.mask_mode, mode, mode.label());
                }
            });
        }
    }
    // Below a mask in the same group, a layer can escape it (and so can everything above it).
    let under_mask = {
        let page = app.page();
        page.location(layer.id).and_then(|loc| page.siblings(loc.parent).map(|s| s[..loc.index].iter().any(|l| l.is_mask))).unwrap_or(false)
    };
    if under_mask && !layer.is_mask {
        ui.checkbox(&mut layer.breaks_mask, "Ignore underlying mask");
    }
    // Instances take their look from the master; overrides cover the rest. Shape groups have a
    // style of their own; plain groups don't.
    let styled = !layer.is_group() || layer.is_shape_group();
    if !layer.is_artboard() && styled && !layer.is_symbol_instance() && !slice {
        shared_style_row(app, ui, layer);
        style_sections(app, ui, layer);
        blur_section(ui, &mut layer.style.blur);
    }
    if !matches!(layer.kind, LayerKind::Hotspot) {
        export_section(app, ui, layer);
    }
    prototype_section(app, ui, layer);
}

/// A Gaussian blur of the layer or of what's behind it.
fn blur_section(ui: &mut Ui, blur: &mut Option<scrawl_core::Blur>) {
    list_header(ui, "Blur", || {
        if blur.is_none() {
            *blur = Some(scrawl_core::Blur::default());
        }
    });
    let mut remove = false;
    if let Some(b) = blur {
        ui.horizontal(|ui| {
            ui.checkbox(&mut b.enabled, "");
            egui::ComboBox::from_id_salt("blur_kind").selected_text(b.kind.label()).width(110.0).show_ui(ui, |ui| {
                for kind in scrawl_core::BlurKind::ALL {
                    ui.selectable_value(&mut b.kind, kind, kind.label());
                }
            });
            remove = icons::small_button(ui, Icon::Minus, "Remove blur").clicked();
        });
        ui.horizontal(|ui| {
            ui.label(if b.kind == scrawl_core::BlurKind::Zoom { "Amount" } else { "Radius" });
            ui.add(egui::Slider::new(&mut b.radius, 0.0..=100.0).max_decimals(1));
        });
        match b.kind {
            scrawl_core::BlurKind::Motion => {
                ui.horizontal(|ui| {
                    num(ui, "Angle", &mut b.angle, 1.0, "°");
                    b.angle = b.angle.rem_euclid(360.0);
                });
            }
            scrawl_core::BlurKind::Zoom => {
                ui.horizontal(|ui| {
                    let (mut x, mut y) = (b.center.x * 100.0, b.center.y * 100.0);
                    if num(ui, "Center X", &mut x, 1.0, "%") | num(ui, "Center Y", &mut y, 1.0, "%") {
                        b.center = scrawl_core::kurbo::Point::new(x / 100.0, y / 100.0);
                    }
                });
            }
            _ => {}
        }
    }
    if remove {
        *blur = None;
    }
}

fn boolean_row(ui: &mut Ui, op: &mut scrawl_core::ops::BooleanOp) {
    section(ui, "Boolean");
    ui.horizontal(|ui| {
        for o in scrawl_core::ops::BooleanOp::ALL {
            if icons::button(ui, crate::ui::toolbar::boolean_icon(o), *op == o, o.label()).clicked() {
                *op = o;
            }
        }
    });
}

fn appearance_section(ui: &mut Ui, layer: &mut Layer) {
    section(ui, "Appearance");
    ui.horizontal(|ui| {
        let mut pct = layer.opacity * 100.0;
        ui.label("Opacity");
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").max_decimals(0)).changed() {
            layer.opacity = pct / 100.0;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Blending");
        crate::ui::paint::blend_combo(ui, "layer_blend", &mut layer.blend_mode);
    });
}

/// A layer's link (destination and animation) and whether it stays put when scrolling; for an
/// artboard, whether it's a start point.
fn prototype_section(app: &mut ScrawlApp, ui: &mut Ui, layer: &mut Layer) {
    use scrawl_core::prototype::{self, Animation, Destination, FlowLink};
    let page = app.page();
    if let LayerKind::Artboard(ab) = &mut layer.kind {
        if ab.symbol_id.is_some() {
            return;
        }
        section(ui, "Prototyping");
        ui.checkbox(&mut ab.flow_home, "Start point").on_hover_text("Previews of this flow start here");
        let incoming = crate::prototyping::incoming(page, layer.id);
        if incoming > 0 {
            ui.weak(format!("{incoming} link{} lead here", if incoming == 1 { "" } else { "s" }));
        }
        if ui.button("Preview").on_hover_text("⌘P").clicked() {
            app.preview = Some(crate::preview::Preview::new(app.page, layer.id));
        }
        return;
    }
    let own = prototype::artboard_of(page, layer.id);
    // This page's artboards first, then other pages' (named with their page).
    let mut targets: Vec<(scrawl_core::LayerId, String)> =
        prototype::artboards(page).into_iter().filter(|a| Some(a.id) != own).map(|a| (a.id, a.name.clone())).collect();
    for other in app.doc.pages.iter().filter(|p| p.id != page.id) {
        targets.extend(prototype::artboards(other).into_iter().map(|a| (a.id, format!("{} / {}", other.name, a.name))));
    }
    let doc = &app.doc;
    let in_artboard = own.is_some();
    section(ui, "Prototyping");
    let name_of = |d: Option<Destination>| match d {
        None => "None".to_owned(),
        Some(Destination::Back) => "Back".to_owned(),
        Some(Destination::Artboard(id)) => {
            prototype::find_artboard(doc, id).map_or_else(|| "Missing artboard".to_owned(), |(_, l)| l.name.clone())
        }
    };
    let mut dest = layer.flow.map(|f| f.destination);
    let current = name_of(dest);
    let mut pick = false;
    ui.horizontal(|ui| {
        ui.label("Link");
        egui::ComboBox::from_id_salt("link_destination").selected_text(current).width(140.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut dest, None, "None");
            ui.selectable_value(&mut dest, Some(Destination::Back), "Back");
            ui.separator();
            for (id, name) in &targets {
                ui.selectable_value(&mut dest, Some(Destination::Artboard(*id)), name);
            }
        });
        pick = icons::small_button(ui, Icon::Link, "Pick on canvas (W)").clicked();
    });
    if pick {
        app.canvas.linking = Some(vec![layer.id]);
    }
    if dest != layer.flow.map(|f| f.destination) {
        let keep = layer.flow;
        layer.flow = dest.map(|destination| FlowLink { destination, ..keep.unwrap_or(FlowLink::back()) });
    }
    if let Some(link) = &mut layer.flow {
        ui.horizontal(|ui| {
            ui.label("Animation");
            egui::ComboBox::from_id_salt("link_animation").selected_text(link.animation.label()).width(140.0).show_ui(ui, |ui| {
                for a in Animation::ALL {
                    ui.selectable_value(&mut link.animation, a, a.label());
                }
            });
        });
        if matches!(link.destination, Destination::Artboard(_)) {
            ui.checkbox(&mut link.overlay, "Open as overlay").on_hover_text("Show it over the current artboard; clicking outside closes it");
        }
    }
    if in_artboard {
        ui.checkbox(&mut layer.fixed_to_viewport, "Fix position when scrolling");
    }
}

/// Picks the shared layer or text style a layer follows, and offers to update or reset it when
/// the layer has drifted from it.
fn shared_style_row(app: &mut ScrawlApp, ui: &mut Ui, layer: &mut Layer) {
    let text = layer.is_text();
    let linked = layer.shared_style.and_then(|id| app.doc.shared_style(id)).cloned();
    let none = if text { "No Text Style" } else { "No Layer Style" };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let selected = linked.as_ref().map_or(none, |s| s.name.as_str());
        egui::ComboBox::from_id_salt("shared_style").selected_text(selected).width(150.0).show_ui(ui, |ui| {
            if ui.selectable_label(linked.is_none(), none).clicked() {
                layer.shared_style = None;
            }
            for s in styles_of_kind(&app.doc, text) {
                if ui.selectable_label(layer.shared_style == Some(s.id), &s.name).clicked() {
                    s.apply(layer);
                }
            }
        });
        if linked.is_none() {
            let tip = if text { "Create a text style from this layer" } else { "Create a layer style from this layer" };
            if ui.button("Create").on_hover_text(tip).clicked() {
                let id = app.doc.create_shared_style(layer);
                layer.shared_style = Some(id);
            }
        } else if ui.button("Detach").on_hover_text("Keep this look but stop following the style").clicked() {
            layer.shared_style = None;
        }
    });
    if let Some(s) = linked
        && !s.matches(layer)
    {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Modified").italics().weak());
            if ui.button("Update Style").on_hover_text("Apply these changes to every layer using the style").clicked() {
                app.doc.update_shared_style(s.id, layer);
            }
            if ui.button("Reset").on_hover_text("Discard changes and match the style").clicked() {
                s.apply(layer);
            }
        });
    }
}

fn style_sections(app: &mut ScrawlApp, ui: &mut Ui, layer: &mut Layer) {
    let closed = layer.is_closed();
    let text_layer = layer.is_text();
    let id = layer.id;
    let mut palette = Palette::new(&mut app.doc);
    let swatches = &mut palette;
    let mut gradient_target = None;

    if closed {
        list_header(ui, "Fills", || layer.style.fills.push(Fill::solid(Color::DEFAULT_FILL)));
        let mut remove = None;
        for (i, fill) in layer.style.fills.iter_mut().enumerate().rev() {
            ui.push_id(("fill", i), |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut fill.enabled, "");
                    let target = PaintTarget {
                        fill_type: Some(&mut fill.fill_type),
                        color: &mut fill.color,
                        gradient: Some(&mut fill.gradient),
                        blend: Some(&mut fill.blend_mode),
                        swatch: Some(&mut fill.swatch),
                        image: Some(ImageTarget { id: &mut fill.image, mode: &mut fill.image_mode, scale: &mut fill.image_scale }),
                        noise: Some(&mut fill.noise),
                    };
                    if paint_button(ui, "fill", target, swatches).editing_gradient {
                        gradient_target = Some(GradientTarget { layer: id, border: false, index: i });
                    }
                    let var = variable_name(swatches, fill.swatch);
                    paint_summary(ui, fill.fill_type, &mut fill.color, &fill.gradient, var);
                    if icons::small_button(ui, Icon::Minus, "Remove fill").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            layer.style.fills.remove(i);
        }
        if let LayerKind::Path(p) = &mut layer.kind
            && p.subpaths.len() > 1
        {
            ui.horizontal(|ui| {
                ui.label("Fill rule");
                egui::ComboBox::from_id_salt("fill_rule")
                    .selected_text(match p.fill_rule {
                        FillRule::NonZero => "Non-Zero",
                        FillRule::EvenOdd => "Even-Odd",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.fill_rule, FillRule::NonZero, "Non-Zero");
                        ui.selectable_value(&mut p.fill_rule, FillRule::EvenOdd, "Even-Odd");
                    });
            });
        }
    }

    list_header(ui, "Borders", || layer.style.borders.push(Border::solid(Color::DEFAULT_BORDER, 1.0)));
    let mut remove = None;
    for (i, border) in layer.style.borders.iter_mut().enumerate().rev() {
        ui.push_id(("border", i), |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut border.enabled, "");
                let target = PaintTarget {
                    fill_type: Some(&mut border.fill_type),
                    color: &mut border.color,
                    gradient: Some(&mut border.gradient),
                    blend: None,
                    swatch: Some(&mut border.swatch),
                    image: None,
                    noise: None,
                };
                if paint_button(ui, "border", target, swatches).editing_gradient {
                    gradient_target = Some(GradientTarget { layer: id, border: true, index: i });
                }
                ui.add(DragValue::new(&mut border.thickness).speed(0.1).range(0.0..=1000.0).max_decimals(2).suffix(" px"));
                if closed && !text_layer {
                    egui::ComboBox::from_id_salt("position")
                        .selected_text(border.position.label())
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for pos in BorderPosition::ALL {
                                ui.selectable_value(&mut border.position, pos, pos.label());
                            }
                        });
                }
                if icons::small_button(ui, Icon::Minus, "Remove border").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        layer.style.borders.remove(i);
    }
    if !layer.style.borders.is_empty() {
        border_options(ui, &mut layer.style.border_options, closed);
    }

    for (title, inner) in [("Shadows", false), ("Inner Shadows", true)] {
        if inner && !closed {
            continue;
        }
        let list = if inner { &mut layer.style.inner_shadows } else { &mut layer.style.shadows };
        list_header(ui, title, || list.push(if inner { Shadow::inner() } else { Shadow::drop() }));
        let mut remove = None;
        for (i, shadow) in list.iter_mut().enumerate().rev() {
            ui.push_id((title, i), |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut shadow.enabled, "");
                    let target = PaintTarget { swatch: Some(&mut shadow.swatch), ..PaintTarget::color(&mut shadow.color) };
                    paint_button(ui, "shadow", target, swatches);
                    if let Some(name) = variable_name(swatches, shadow.swatch) {
                        crate::ui::paint::variable_label(ui, &name);
                    }
                    if icons::small_button(ui, Icon::Minus, "Remove shadow").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    let w = 44.0;
                    for (label, v) in [("X", &mut shadow.offset_x), ("Y", &mut shadow.offset_y), ("Blur", &mut shadow.blur), ("Spread", &mut shadow.spread)] {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add_sized([w, 20.0], DragValue::new(v).speed(0.25).max_decimals(1));
                            ui.label(RichText::new(label).small().weak());
                        });
                    }
                    shadow.blur = shadow.blur.max(0.0);
                });
            });
        }
        if let Some(i) = remove {
            list.remove(i);
        }
    }

    // Keep the canvas's gradient handles in sync with whichever gradient popover is open.
    if gradient_target.is_some() {
        app.gradient_edit = gradient_target;
    }
}

/// The name of the color variable `link` points to.
fn variable_name(palette: &Palette<'_>, link: Option<scrawl_core::SwatchId>) -> Option<String> {
    palette.find(link?).map(|v| v.name.clone())
}

/// A compact description next to a paint swatch: hex and opacity for colors (or the color
/// variable's name), the gradient kind otherwise.
fn paint_summary(ui: &mut Ui, fill_type: FillType, color: &mut Color, gradient: &scrawl_core::Gradient, variable: Option<String>) {
    match (fill_type, variable) {
        (FillType::Color, Some(name)) => {
            crate::ui::paint::variable_label(ui, &name).on_hover_text("Color variable");
        }
        (FillType::Color, None) => {
            hex_field(ui, color);
            alpha_field(ui, color);
        }
        (FillType::Gradient, _) => {
            ui.label(format!("{} gradient", gradient.kind.label()));
        }
        (FillType::Image, _) => {
            ui.label("Image");
        }
        (FillType::Noise, _) => {
            ui.label("Noise");
        }
    }
}

fn border_options(ui: &mut Ui, opts: &mut BorderOptions, closed: bool) {
    egui::CollapsingHeader::new(RichText::new("Border options").small()).id_salt("border_options").show(ui, |ui| {
        ui.horizontal(|ui| {
            let mut dash = opts.dash.first().copied().unwrap_or(0.0);
            let mut gap = opts.dash.get(1).copied().unwrap_or(0.0);
            let a = num(ui, "Dash", &mut dash, 0.25, "");
            let b = num(ui, "Gap", &mut gap, 0.25, "");
            if a || b {
                opts.dash = match (dash.max(0.0), gap.max(0.0)) {
                    (d, _) if d <= 0.0 => vec![],
                    (d, g) if g <= 0.0 => vec![d],
                    (d, g) => vec![d, g],
                };
            }
        });
        ui.horizontal(|ui| {
            ui.label("Ends");
            for (cap, label) in [(LineCap::Butt, "Butt"), (LineCap::Round, "Round"), (LineCap::Square, "Square")] {
                ui.selectable_value(&mut opts.cap, cap, label);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Joins");
            for (join, label) in [(LineJoin::Miter, "Miter"), (LineJoin::Round, "Round"), (LineJoin::Bevel, "Bevel")] {
                ui.selectable_value(&mut opts.join, join, label);
            }
        });
        if !closed {
            for (label, marker) in [("Start", &mut opts.start_marker), ("End", &mut opts.end_marker)] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    egui::ComboBox::from_id_salt(label).selected_text(marker.label()).width(120.0).show_ui(ui, |ui| {
                        for m in Marker::ALL {
                            ui.selectable_value(marker, m, m.label());
                        }
                    });
                });
            }
        }
    });
}

fn list_header(ui: &mut Ui, title: &str, add: impl FnOnce()) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title.to_uppercase()).small().strong().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let noun = title.trim_end_matches('s').to_lowercase();
            if icons::small_button(ui, Icon::Plus, &format!("Add {noun}")).clicked() {
                add();
            }
        });
    });
}

/// Hex text field. The edit buffer lives in egui memory while focused so partial input isn't
/// clobbered by re-formatting every frame.
pub fn hex_field(ui: &mut Ui, color: &mut Color) {
    let id = ui.id().with("hex");
    let mut buf: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| color.with_alpha(1.0).to_hex());
    let resp = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(58.0).char_limit(7).font(egui::TextStyle::Monospace));
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if (resp.lost_focus() || resp.changed())
        && let Some(c) = Color::from_hex(&buf).filter(|_| buf.trim_start_matches('#').len() == 6)
    {
        *color = c.with_alpha(color.a);
    }
}

pub fn alpha_field(ui: &mut Ui, color: &mut Color) {
    let mut pct = (color.a * 100.0).round();
    if ui.add(DragValue::new(&mut pct).range(0.0..=100.0).suffix("%").speed(1.0)).changed() {
        color.a = pct / 100.0;
    }
}

fn multi_selection(app: &mut ScrawlApp, ui: &mut Ui) {
    ui.add_space(4.0);
    ui.strong(format!("{} layers selected", app.selection.len()));
    section(ui, "Arrange");
    ui.horizontal(|ui| {
        let three = app.selection.len() >= 3;
        if ui.add_enabled(three, egui::Button::new("Distribute ↔")).on_hover_text("Distribute Horizontally").clicked() {
            app.distribute(scrawl_core::snap::Axis::X);
        }
        if ui.add_enabled(three, egui::Button::new("Distribute ↕")).on_hover_text("Distribute Vertically").clicked() {
            app.distribute(scrawl_core::snap::Axis::Y);
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Tidy").on_hover_text("Arrange into rows with even spacing").clicked() {
            app.tidy();
        }
        if ui.button("Create Stack").clicked() {
            app.make_stack();
        }
    });
    section(ui, "Appearance");
    let page = app.page();
    let first = app.selection.iter().find_map(|&id| page.find(id)).map_or(1.0, |l| l.opacity);
    let mut pct = first * 100.0;
    ui.horizontal(|ui| {
        ui.label("Opacity");
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").max_decimals(0)).changed() {
            let sel = app.selection.clone();
            let page = app.page_mut();
            for id in sel {
                if let Some(l) = page.find_mut(id) {
                    l.opacity = pct / 100.0;
                }
            }
        }
    });
    let page = app.page();
    let mut blend = app.selection.iter().find_map(|&id| page.find(id)).map_or_else(Default::default, |l| l.blend_mode);
    let before = blend;
    ui.horizontal(|ui| {
        ui.label("Blending");
        crate::ui::paint::blend_combo(ui, "multi_blend", &mut blend);
    });
    if blend != before {
        let sel = app.selection.clone();
        let page = app.page_mut();
        for id in sel {
            if let Some(l) = page.find_mut(id) {
                l.blend_mode = blend;
            }
        }
    }
}

fn artboard_presets(app: &mut ScrawlApp, ui: &mut Ui) {
    ui.add_space(4.0);
    ui.weak("Drag on the canvas to draw an artboard, or pick a preset:");
    for (group, presets) in ARTBOARD_PRESETS {
        egui::CollapsingHeader::new(*group).id_salt(Id::new(("presets", *group))).default_open(*group == "Apple").show(ui, |ui| {
            for &(name, w, h) in *presets {
                let resp = ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::Button::new(egui::Atoms::new((name, egui::Atom::grow(), RichText::new(format!("{w:.0}×{h:.0}")).weak()))).frame_when_inactive(false),
                );
                if resp.clicked() {
                    app.insert_artboard_preset(name, w, h);
                }
            }
        });
    }
}

fn points_section(app: &mut ScrawlApp, ui: &mut Ui) {
    let Some(edit) = app.canvas.edit.as_ref() else { return };
    let (id, selected) = (edit.id, edit.selected.clone());
    let page = app.page();
    let shape = page.find(id).and_then(Layer::editable_path);
    let origin = page.parent_transform(id).map_or(Point::ORIGIN, |t| t * Point::ORIGIN);

    ui.horizontal(|ui| {
        ui.strong("Editing points");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Done").clicked() {
                app.exit_vector_edit();
            }
        });
    });
    let scissors = app.canvas.edit.as_ref().is_some_and(|e| e.scissors);
    ui.weak(if scissors {
        "Scissors: click a segment to cut it out."
    } else {
        "Click a segment to add a point. Double-click a point to toggle curve. Keys 1–4 set the point type."
    });
    let mut on = scissors;
    if ui.toggle_value(&mut on, "✂ Scissors").on_hover_text("Cut segments out of the path (C)").changed()
        && let Some(e) = app.canvas.edit.as_mut()
    {
        e.scissors = on;
    }
    if selected.is_empty() {
        ui.add_space(8.0);
        ui.weak("No points selected");
        return;
    }

    section(ui, "Point");
    if let Some(b) = crate::vector::selected_points_page(app) {
        ui.horizontal(|ui| {
            let (mut x, mut y) = (b.x0 - origin.x, b.y0 - origin.y);
            let cx = num(ui, "X", &mut x, 1.0, "");
            let cy = num(ui, "Y", &mut y, 1.0, "");
            if cx || cy {
                crate::vector::move_selected_points_to(app, Point::new(x + origin.x, y + origin.y));
            }
        });
    }

    section(ui, "Corner Radius");
    let radius = shape.as_ref().and_then(|s| selected.first().and_then(|&r| s.point(r)).map(|p| p.corner_radius)).unwrap_or(0.0);
    ui.horizontal(|ui| {
        let mut r = radius;
        if num(ui, "Radius", &mut r, 0.5, "") {
            crate::vector::set_corner_radius(app, r);
        }
    });

    section(ui, "Point Type");
    let current: Option<PointMode> = shape.as_ref().and_then(|s| {
        let modes: Vec<_> = selected.iter().filter_map(|&r| s.point(r).map(|p| p.mode)).collect();
        modes.first().copied().filter(|m| modes.iter().all(|x| x == m))
    });
    ui.horizontal_wrapped(|ui| {
        for (i, mode) in [PointMode::Straight, PointMode::Mirrored, PointMode::Disconnected, PointMode::Asymmetric].into_iter().enumerate() {
            let resp = ui.selectable_label(current == Some(mode), mode.label()).on_hover_text(format!("Key {}", i + 1));
            if resp.clicked() {
                crate::vector::set_point_mode(app, mode);
            }
        }
    });
}

/// Text properties. With `range` (text selected while editing), character properties apply to
/// that range as style runs; paragraph properties always apply to the whole layer.
fn text_section(ui: &mut Ui, text: &mut scrawl_core::TextLayer, engine: &mut scrawl_text::TextEngine, range: Option<std::ops::Range<usize>>) {
    use scrawl_core::text::{WEIGHT_NAMES, weight_name};
    use scrawl_core::{TextAlign, TextSizing, TextTransform, VerticalAlign};
    let (effective, run_color) = match &range {
        Some(r) => {
            let (mut s, color) = text.style_at(r.start);
            s.list = text.list_at(text.paragraph_range(r.start).start);
            (s, color)
        }
        None => (text.style.clone(), None),
    };
    let mut working = effective.clone();
    let style = &mut working;
    section(ui, "Text");
    let mut detach = false;
    if let Some(on) = &mut text.on_path {
        ui.horizontal(|ui| {
            num(ui, "Start", &mut on.offset, 0.5, "");
            on.offset = on.offset.max(0.0);
            detach = ui.button("Detach from Path").clicked();
        });
    }
    if detach {
        text.on_path = None;
        text.sizing = TextSizing::Auto;
    }
    if let Some(r) = &range {
        let n = text.text.get(r.clone()).map_or(0, |s| s.chars().count());
        ui.weak(format!("Styling {n} selected character{}", if n == 1 { "" } else { "s" }));
        let mut color = run_color;
        ui.horizontal(|ui| {
            ui.label("Color");
            let mut c = crate::ui::paint::to_color32(color.unwrap_or(scrawl_core::Color::BLACK));
            if ui.color_edit_button_srgba(&mut c).changed() {
                let [r, g, b, a] = c.to_srgba_unmultiplied();
                color = Some(scrawl_core::Color::from_rgba8(r, g, b, a));
            }
            if color.is_some() && ui.small_button("Use layer fill").clicked() {
                color = None;
            }
        });
        if color != run_color {
            text.set_run_style(r.clone(), |s| s.color = color);
        }
    }

    // Typeface picker with search.
    ui.horizontal(|ui| {
        ui.label("Typeface");
        let current = if style.font_family.is_empty() { "System".to_owned() } else { style.font_family.clone() };
        let resp = ui.add(egui::Button::new(current).truncate().min_size(egui::vec2(150.0, 0.0)));
        let search_id = ui.id().with("font_search");
        egui::Popup::from_toggle_button_response(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(220.0);
                let mut query: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
                let edit = ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search fonts"));
                edit.request_focus();
                ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
                let q = query.to_lowercase();
                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    if q.is_empty() && ui.selectable_label(style.font_family.is_empty(), "System").clicked() {
                        style.font_family.clear();
                        egui::Popup::close_all(ui.ctx());
                    }
                    for name in engine.family_names().iter().filter(|n| n.to_lowercase().contains(&q)) {
                        if ui.selectable_label(&style.font_family == name, name).clicked() {
                            style.font_family = name.clone();
                            egui::Popup::close_all(ui.ctx());
                        }
                    }
                });
            });
    });

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("weight").selected_text(weight_name(style.font_weight)).width(110.0).show_ui(ui, |ui| {
            for (w, name) in WEIGHT_NAMES {
                ui.selectable_value(&mut style.font_weight, w, name);
            }
        });
        ui.toggle_value(&mut style.italic, RichText::new("Italic").italics());
    });

    ui.horizontal(|ui| {
        num(ui, "Size", &mut style.font_size, 0.25, "");
        style.font_size = style.font_size.clamp(1.0, 2000.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let mut lh = style.line_height.unwrap_or(0.0);
            let drag = DragValue::new(&mut lh)
                .speed(0.25)
                .range(0.0..=5000.0)
                .custom_formatter(|v, _| if v <= 0.0 { "Auto".into() } else { format!("{v:.1}") });
            if ui.add_sized([FIELD_WIDTH, 20.0], drag).changed() {
                style.line_height = (lh > 0.0).then_some(lh);
            }
            ui.label(RichText::new("Line").small().weak());
        });
        num(ui, "Character", &mut style.letter_spacing, 0.05, "");
    });
    ui.horizontal(|ui| {
        num(ui, "Paragraph", &mut style.paragraph_spacing, 0.25, "");
        style.paragraph_spacing = style.paragraph_spacing.max(0.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for list in scrawl_core::text::ListStyle::ALL {
                    ui.selectable_value(&mut style.list, list, list.label());
                }
            });
            ui.label(RichText::new("List").small().weak());
        });
    });
    egui::CollapsingHeader::new("OpenType features").id_salt("ot_features").show(ui, |ui| {
        for (tag, label, _) in scrawl_core::text::COMMON_FEATURES {
            let mut on = style.feature_on(tag);
            if ui.checkbox(&mut on, label).on_hover_text(tag).changed() {
                style.set_feature(tag, on);
            }
        }
    });

    ui.horizontal(|ui| {
        for (align, label) in [(TextAlign::Left, "Left"), (TextAlign::Center, "Center"), (TextAlign::Right, "Right"), (TextAlign::Justify, "Justify")] {
            ui.selectable_value(&mut style.alignment, align, label);
        }
    });
    ui.horizontal(|ui| {
        for sizing in TextSizing::ALL {
            ui.selectable_value(&mut text.sizing, sizing, sizing.label());
        }
    });
    if text.sizing == TextSizing::Fixed {
        ui.horizontal(|ui| {
            ui.label("Vertical");
            for (v, label) in [(VerticalAlign::Top, "Top"), (VerticalAlign::Middle, "Middle"), (VerticalAlign::Bottom, "Bottom")] {
                ui.selectable_value(&mut style.vertical_alignment, v, label);
            }
        });
    }
    ui.horizontal(|ui| {
        ui.toggle_value(&mut style.underline, RichText::new("Underline").underline());
        ui.toggle_value(&mut style.strikethrough, RichText::new("Strike").strikethrough());
        egui::ComboBox::from_id_salt("transform")
            .selected_text(match style.transform {
                TextTransform::None => "Aa",
                TextTransform::Uppercase => "AA",
                TextTransform::Lowercase => "aa",
            })
            .width(48.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut style.transform, TextTransform::None, "None");
                ui.selectable_value(&mut style.transform, TextTransform::Uppercase, "Uppercase");
                ui.selectable_value(&mut style.transform, TextTransform::Lowercase, "Lowercase");
            });
    });

    match range {
        None => {
            // Lists and features set for the whole layer replace those of its parts.
            if working.list != text.style.list {
                text.set_run_style(0..text.text.len(), |s| s.list = None);
            }
            if working.features != text.style.features {
                text.set_run_style(0..text.text.len(), |s| s.features = None);
            }
            text.style = working;
        }
        Some(r) => {
            // Paragraph properties belong to the layer, except lists, which belong to the
            // paragraphs the range touches.
            text.style.line_height = working.line_height;
            text.style.alignment = working.alignment;
            text.style.vertical_alignment = working.vertical_alignment;
            text.style.transform = working.transform;
            text.style.paragraph_spacing = working.paragraph_spacing;
            if working.list != effective.list {
                let (list, base) = (working.list, text.style.list);
                text.set_paragraph_style(r.clone(), |s| s.list = (list != base).then_some(list));
            }
            // Changed character properties become overrides on the range; values matching the
            // layer's style need none.
            macro_rules! apply {
                ($($field:ident),*) => {$(
                    if working.$field != effective.$field {
                        let v = working.$field.clone();
                        let base = text.style.$field.clone();
                        text.set_run_style(r.clone(), |s| s.$field = (v != base).then(|| v.clone()));
                    }
                )*};
            }
            apply!(font_family, font_size, font_weight, italic, letter_spacing, underline, strikethrough, features);
        }
    }
}

fn grid_settings(app: &mut ScrawlApp, ui: &mut Ui, ab: &mut scrawl_core::Artboard) {
    use scrawl_core::grid::{Columns, Rows};
    section(ui, "Layout");
    ui.horizontal(|ui| {
        let mut on = ab.layout.columns.is_some();
        if ui.checkbox(&mut on, "Columns").changed() {
            ab.layout.columns = on.then(Columns::default);
            app.show_layout |= on;
        }
    });
    if let Some(c) = &mut ab.layout.columns {
        ui.horizontal(|ui| {
            let mut count = c.count as f64;
            if num(ui, "Count", &mut count, 0.1, "") {
                c.count = count.clamp(1.0, 100.0).round() as u32;
            }
            num(ui, "Gutter", &mut c.gutter, 0.5, "");
            num(ui, "Margin", &mut c.margin, 0.5, "");
            c.gutter = c.gutter.max(0.0);
            c.margin = c.margin.max(0.0);
        });
    }
    ui.horizontal(|ui| {
        let mut on = ab.layout.rows.is_some();
        if ui.checkbox(&mut on, "Rows").changed() {
            ab.layout.rows = on.then(Rows::default);
            app.show_layout |= on;
        }
    });
    if let Some(r) = &mut ab.layout.rows {
        ui.horizontal(|ui| {
            num(ui, "Height", &mut r.height, 0.5, "");
            num(ui, "Gutter", &mut r.gutter, 0.5, "");
            r.height = r.height.max(1.0);
            r.gutter = r.gutter.max(0.0);
        });
    }
    section(ui, "Grid");
    ui.horizontal(|ui| {
        num(ui, "Size", &mut ab.grid.size, 0.5, "");
        ab.grid.size = ab.grid.size.max(1.0);
        let mut every = ab.grid.thick_every as f64;
        if num(ui, "Thick every", &mut every, 0.1, "") {
            ab.grid.thick_every = every.clamp(0.0, 100.0).round() as u32;
        }
    });
    if !app.show_grid || !app.show_layout {
        ui.weak("⌃G shows the grid, ⌃L the layout");
    }
}

/// The constraint control: click the edges to pin, the inner lines to fix the size.
fn resizing_widget(ui: &mut Ui, r: &mut scrawl_core::constraints::Resizing) {
    use egui::{Pos2, Rect as ERect, Sense, Stroke, vec2};
    ui.horizontal(|ui| {
        let (outer, _) = ui.allocate_exact_size(vec2(80.0, 80.0), Sense::hover());
        let inner = outer.shrink(19.0);
        let weak = ui.visuals().weak_text_color();
        let p = ui.painter();
        p.rect_stroke(outer, 3.0, Stroke::new(1.0, weak.gamma_multiply(0.5)), egui::StrokeKind::Inside);
        p.rect_stroke(inner, 2.0, Stroke::new(1.0, weak), egui::StrokeKind::Inside);
        let c = outer.center();
        let toggles: [(&mut bool, [Pos2; 2], &str); 6] = [
            (&mut r.top, [Pos2::new(c.x, outer.top() + 4.0), Pos2::new(c.x, inner.top() - 3.0)], "Pin to top edge"),
            (&mut r.bottom, [Pos2::new(c.x, inner.bottom() + 3.0), Pos2::new(c.x, outer.bottom() - 4.0)], "Pin to bottom edge"),
            (&mut r.left, [Pos2::new(outer.left() + 4.0, c.y), Pos2::new(inner.left() - 3.0, c.y)], "Pin to left edge"),
            (&mut r.right, [Pos2::new(inner.right() + 3.0, c.y), Pos2::new(outer.right() - 4.0, c.y)], "Pin to right edge"),
            (&mut r.fix_width, [Pos2::new(inner.left() + 4.0, c.y), Pos2::new(inner.right() - 4.0, c.y)], "Fix width"),
            (&mut r.fix_height, [Pos2::new(c.x, inner.top() + 4.0), Pos2::new(c.x, inner.bottom() - 4.0)], "Fix height"),
        ];
        for (i, (on, [a, b], tip)) in toggles.into_iter().enumerate() {
            let hit = ERect::from_two_pos(a, b).expand(5.0);
            let resp = ui.interact(hit, ui.id().with(("resizing", i)), Sense::click()).on_hover_text(tip);
            if resp.clicked() {
                *on = !*on;
            }
            let color = if *on { crate::style::ACCENT } else if resp.hovered() { ui.visuals().text_color() } else { weak.gamma_multiply(0.6) };
            let width = if *on { 2.0 } else { 1.0 };
            ui.painter().line_segment([a, b], Stroke::new(width, color));
            // Arrowheads on the size lines.
            if i >= 4 {
                let d = (b - a).normalized() * 3.0;
                let n = vec2(-d.y, d.x);
                for (end, dir) in [(a, d), (b, -d)] {
                    ui.painter().line_segment([end, end + dir + n], Stroke::new(width, color));
                    ui.painter().line_segment([end, end + dir - n], Stroke::new(width, color));
                }
            }
        }
        ui.vertical(|ui| {
            let summary = |pins: (bool, bool), fix: bool| match (pins, fix) {
                ((true, true), false) => "Stretch",
                ((true, _), _) => "Pinned start",
                ((false, true), _) => "Pinned end",
                ((false, false), true) => "Fixed, centered",
                ((false, false), false) => "Scale",
            };
            ui.weak(format!("H: {}", summary((r.left, r.right), r.fix_width)));
            ui.weak(format!("V: {}", summary((r.top, r.bottom), r.fix_height)));
            if !r.is_default() && ui.small_button("Reset").clicked() {
                *r = Default::default();
            }
        });
    });
}

/// Symbols by name, local and from libraries, for symbol pickers.
fn master_names(app: &ScrawlApp) -> Vec<(SymbolId, String)> {
    app.doc.all_symbols()
}

/// Shared text or layer styles, local then from libraries.
fn styles_of_kind(doc: &scrawl_core::Document, text: bool) -> Vec<&scrawl_core::SharedStyle> {
    let local = if text { &doc.text_styles } else { &doc.layer_styles };
    local.iter().chain(doc.foreign_styles.iter().map(|f| &f.item).filter(|s| s.is_text() == text)).collect()
}

fn symbol_section(app: &mut ScrawlApp, ui: &mut Ui, id: scrawl_core::LayerId, inst: &mut SymbolInstance) {
    let masters = master_names(app);
    let name_of = |s: SymbolId| masters.iter().find(|(m, _)| *m == s).map_or("Missing symbol".to_owned(), |(_, n)| n.clone());
    section(ui, "Symbol");
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("instance_symbol").selected_text(name_of(inst.symbol_id)).width(150.0).show_ui(ui, |ui| {
            for (symbol, name) in &masters {
                ui.selectable_value(&mut inst.symbol_id, *symbol, name);
            }
        });
    });
    ui.horizontal(|ui| {
        if ui.button("Edit Master").clicked() {
            app.edit_master(inst.symbol_id);
        }
        if ui.button("Detach").on_hover_text("Replace with a group of its layers (⌥⌘B)").clicked() {
            app.selection = vec![id];
            app.detach_selection();
        }
    });

    let items = Symbols::new(&app.doc).overridables(inst);
    if items.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        section(ui, "Overrides");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!inst.overrides.is_empty(), egui::Button::new("Reset").small()).clicked() {
                inst.overrides.clear();
            }
        });
    });
    for item in items {
        ui.push_id(&item.path, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(item.depth as f32 * 10.0);
                ui.add_sized([72.0, 18.0], egui::Label::new(RichText::new(&item.name).small()).truncate());
                let current = inst.override_for(&item.path).cloned();
                // Choosing the default value clears the override.
                let mut set = |value: OverrideValue, default: bool| inst.set_override(item.path.clone(), (!default).then_some(value));
                match &item.kind {
                    OverridableKind::Text(default) => {
                        let mut text = match current {
                            Some(OverrideValue::Text(t)) => t,
                            _ => default.clone(),
                        };
                        let edit = egui::TextEdit::singleline(&mut text).hint_text(default.as_str()).desired_width(f32::INFINITY);
                        if ui.add(edit).changed() {
                            let is_default = text == *default;
                            set(OverrideValue::Text(text), is_default);
                        }
                    }
                    OverridableKind::Symbol(default) => {
                        let shown = match current {
                            Some(OverrideValue::Symbol(s)) => s,
                            _ => Some(*default),
                        };
                        let label = shown.map_or("None".to_owned(), name_of);
                        egui::ComboBox::from_id_salt("swap").selected_text(label).width(120.0).show_ui(ui, |ui| {
                            if ui.selectable_label(shown.is_none(), "None").clicked() {
                                set(OverrideValue::Symbol(None), false);
                            }
                            for (symbol, name) in &masters {
                                if ui.selectable_label(shown == Some(*symbol), name).clicked() {
                                    set(OverrideValue::Symbol(Some(*symbol)), symbol == default);
                                }
                            }
                        });
                    }
                    OverridableKind::Image(default) => {
                        let overridden = matches!(current, Some(OverrideValue::Image(_)));
                        let assets: Vec<String> = app.doc.assets.iter().map(|(id, _)| id.clone()).collect();
                        let label = if overridden { "Replaced" } else { "Master's image" };
                        egui::ComboBox::from_id_salt("image").selected_text(label).width(120.0).show_ui(ui, |ui| {
                            if ui.selectable_label(!overridden, "Master's image").clicked() {
                                inst.set_override(item.path.clone(), None);
                            }
                            for (i, id) in assets.iter().enumerate() {
                                let chosen = matches!(&current, Some(OverrideValue::Image(c)) if c == id);
                                if Some(id) != default.as_ref() && ui.selectable_label(chosen, format!("Image {}", i + 1)).clicked() {
                                    inst.set_override(item.path.clone(), Some(OverrideValue::Image(id.clone())));
                                }
                            }
                        });
                    }
                    OverridableKind::Color(default) => {
                        let shown = match current {
                            Some(OverrideValue::Color(c)) => c,
                            _ => *default,
                        };
                        let mut c = crate::ui::paint::to_color32(shown);
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            let [r, g, b, a] = c.to_srgba_unmultiplied();
                            let color = scrawl_core::Color::from_rgba8(r, g, b, a);
                            set(OverrideValue::Color(color), color == *default);
                        }
                    }
                    OverridableKind::Style { style, text } => {
                        let shown = match current {
                            Some(OverrideValue::Style(s)) => s,
                            _ => *style,
                        };
                        let list = styles_of_kind(&app.doc, *text);
                        let label = list.iter().find(|s| s.id == shown).map_or("Missing style", |s| s.name.as_str());
                        egui::ComboBox::from_id_salt("style").selected_text(label).width(120.0).show_ui(ui, |ui| {
                            for s in list {
                                if ui.selectable_label(shown == s.id, &s.name).clicked() {
                                    set(OverrideValue::Style(s.id), s.id == *style);
                                }
                            }
                        });
                    }
                }
            });
        });
    }
}

/// Export formats: scale, suffix and file type per row, and a button to export now.
fn export_section(app: &mut ScrawlApp, ui: &mut Ui, layer: &mut Layer) {
    use scrawl_core::exportable::{ExportFormat, FileFormat};
    list_header(ui, "Export", || layer.exports.push(ExportFormat::after(layer.exports.last())));
    let mut remove = None;
    for (i, f) in layer.exports.iter_mut().enumerate() {
        ui.push_id(("export", i), |ui| {
            ui.horizontal(|ui| {
                let drag = DragValue::new(&mut f.scale).range(0.1..=16.0).speed(0.05).max_decimals(2).suffix("x");
                ui.add_sized([48.0, 20.0], drag);
                ui.add(egui::TextEdit::singleline(&mut f.suffix).hint_text("Suffix").desired_width(64.0));
                egui::ComboBox::from_id_salt("format").selected_text(f.format.label()).width(56.0).show_ui(ui, |ui| {
                    for format in FileFormat::ALL {
                        ui.selectable_value(&mut f.format, format, format.label());
                    }
                });
                if icons::small_button(ui, Icon::Minus, "Remove export format").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        layer.exports.remove(i);
    }
    if !layer.exports.is_empty() && ui.button(format!("Export {}…", layer.name)).clicked() {
        // Export what's being edited, so a just-added format counts.
        let (id, edited) = (layer.id, layer.clone());
        if let Some(l) = app.page_mut().find_mut(id) {
            *l = edited;
        }
        app.export_layers(&[id]);
    }
}

fn image_section(app: &mut ScrawlApp, ui: &mut Ui, frame: &mut scrawl_core::Frame, img: &mut scrawl_core::ImageLayer) {
    section(ui, "Image");
    let size = app.images.size(&img.image);
    ui.horizontal(|ui| {
        match size {
            Some((w, h)) => ui.weak(format!("{w}×{h} px")),
            None => ui.colored_label(ui.visuals().warn_fg_color, "Missing image"),
        };
        if let Some((w, h)) = size
            && ui.button("Original Size").clicked()
        {
            frame.width = w as f64;
            frame.height = h as f64;
        }
        if ui.button("Replace…").clicked()
            && let Some(path) = rfd::FileDialog::new().add_filter("Images", &crate::import::IMAGE_EXTENSIONS).pick_file()
        {
            match std::fs::read(&path).ok().and_then(|b| app.doc.assets.add_image(b)) {
                Some(id) => img.image = id,
                None => app.error = Some(format!("Couldn't read {} as an image", path.display())),
            }
        }
    });
}

fn master_section(app: &mut ScrawlApp, ui: &mut Ui, ab: &mut scrawl_core::Artboard, symbol: SymbolId) {
    section(ui, "Symbol Master");
    let mut count = 0;
    for page in &app.doc.pages {
        page.walk(|l| {
            if matches!(&l.kind, LayerKind::Symbol(i) if i.symbol_id == symbol) {
                count += 1;
            }
        });
    }
    ui.weak(match count {
        1 => "1 instance".to_owned(),
        n => format!("{n} instances"),
    });
    ui.horizontal(|ui| {
        if ui.button("Insert Instance").clicked() {
            app.insert_symbol(symbol);
        }
        if ui.button("Convert to Artboard").on_hover_text("Detach every instance and make this a plain artboard").clicked() {
            scrawl_core::symbols::detach_all(&mut app.doc, symbol);
            ab.symbol_id = None;
        }
    });
    // Which overrides instances may use.
    let items = Symbols::new(&app.doc).all_overridables(symbol);
    if items.is_empty() {
        return;
    }
    section(ui, "Allow Overrides");
    for item in items {
        let property = item.kind.property();
        let key = scrawl_core::symbols::OverrideRef { path: item.path.clone(), property: property.into() };
        let mut allowed = !ab.disabled_overrides.contains(&key);
        ui.push_id((&item.path, property), |ui| {
            ui.horizontal(|ui| {
                ui.add_space(item.depth as f32 * 10.0);
                if ui.checkbox(&mut allowed, format!("{} ({property})", item.name)).changed() {
                    if allowed {
                        ab.disabled_overrides.retain(|d| *d != key);
                    } else {
                        ab.disabled_overrides.push(key.clone());
                    }
                }
            });
        });
    }
}

fn stack_section(ui: &mut Ui, g: &mut scrawl_core::Group) {
    use scrawl_core::layout::{Stack, StackAlign};
    use scrawl_core::snap::Axis;
    section(ui, "Stack");
    let mut on = g.stack.is_some();
    if ui.checkbox(&mut on, "Stack layout").on_hover_text("Lay out the children in the layer list's order").changed() {
        g.stack = on.then(Stack::default);
    }
    let Some(stack) = &mut g.stack else { return };
    ui.horizontal(|ui| {
        ui.selectable_value(&mut stack.direction, Axis::Y, "Vertical");
        ui.selectable_value(&mut stack.direction, Axis::X, "Horizontal");
    });
    ui.horizontal(|ui| {
        num(ui, "Spacing", &mut stack.spacing, 0.5, "");
        let (start, end) = if stack.direction == Axis::Y { ("Left", "Right") } else { ("Top", "Bottom") };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut stack.align, StackAlign::Start, start);
                ui.selectable_value(&mut stack.align, StackAlign::Center, "Center");
                ui.selectable_value(&mut stack.align, StackAlign::End, end);
            });
            ui.label(RichText::new("Align").small().weak());
        });
    });
    ui.horizontal(|ui| {
        for (i, label) in ["Top", "Right", "Bottom", "Left"].iter().enumerate() {
            let mut v = stack.padding[i];
            ui.push_id(i, |ui| {
                if num_small(ui, label, &mut v) {
                    stack.padding[i] = v.max(0.0);
                }
            });
        }
    });
    ui.weak("Padding");
    let main = if stack.direction == Axis::X { "Width" } else { "Height" };
    ui.horizontal(|ui| {
        let mut wraps = stack.wrap.is_some();
        if ui.checkbox(&mut wraps, "Wrap at").changed() {
            stack.wrap = wraps.then_some(400.0);
        }
        if let Some(w) = &mut stack.wrap {
            ui.add(DragValue::new(w).range(1.0..=100_000.0).speed(1.0));
        }
    });
    ui.horizontal(|ui| {
        let mut fixed = stack.length.is_some();
        if ui.checkbox(&mut fixed, format!("Fixed {}", main.to_lowercase())).on_hover_text("Children set to Grow share what's left").changed() {
            stack.length = fixed.then_some(300.0);
        }
        if let Some(l) = &mut stack.length {
            ui.add(DragValue::new(l).range(1.0..=100_000.0).speed(1.0));
        }
    });
}

/// A compact numeric field with a label under it, for rows of four.
fn num_small(ui: &mut Ui, label: &str, v: &mut f64) -> bool {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let changed = ui.add_sized([48.0, 20.0], DragValue::new(v).speed(0.5).max_decimals(1)).changed();
        ui.label(RichText::new(label).small().weak());
        changed
    })
    .inner
}
