//! Reading and writing Sketch documents (`.sketch`), following Sketch's open file format
//! (<https://github.com/sketch-hq/sketch-file-format>): a zip of `document.json`, `meta.json`,
//! `user.json`, one JSON file per page under `pages/`, and images under `images/`.
//!
//! Scrawl's model mirrors Sketch's (frames in parent space, points normalized to the frame,
//! rotation and flips about the center, shared styles, symbols and swatches), so most things map
//! one to one. The differences:
//! - Sketch stores rotation counter-clockwise; Scrawl's is clockwise.
//! - Text has one style per layer here, so only the first run of an attributed string is kept.
//! - Shape groups (live boolean operations) are flattened into a single path on import; paths
//!   with several subpaths are written as shape groups.
//! - Library components keep their library's name, but not its file, which Sketch doesn't store.
//! - Things Scrawl doesn't model (blurs, noise fills, prototyping, layouts) are dropped on import
//!   and written with neutral defaults.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use kurbo::{BezPath, Point};
use serde_json::{Map, Value, json};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::constraints::Resizing;
use crate::exportable::{ExportFormat, FileFormat};
use crate::library::{Foreign, LibraryId, LibraryRef};
use crate::text::{TextAlign, TextLayer, TextSizing, TextStyle, TextTransform, VerticalAlign, WEIGHT_NAMES};
use crate::{
    Artboard, BlendMode, Border, BorderOptions, BorderPosition, Color, CurvePoint, Document, Fill, FillRule, FillType, Frame, Gradient,
    GradientKind, GradientStop, Group, ImageLayer, ImageMode, Layer, LayerId, LayerKind, LineCap, LineJoin, Marker, Override, OverrideValue,
    Page, PageId, PathShape, PointMode, Polygon, Rectangle, Shadow, SharedStyle, Star, StyleId, Subpath, Swatch, SwatchId, SymbolId,
    SymbolInstance,
};

pub const FILE_EXTENSION: &str = "sketch";

// ---- Small helpers ---------------------------------------------------------------------------

fn num(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn num_or(v: &Value, k: &str, default: f64) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(default)
}

fn int(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64))).unwrap_or(0)
}

fn flag(v: &Value, k: &str) -> bool {
    v.get(k).is_some_and(|x| x.as_bool().unwrap_or_else(|| x.as_i64().is_some_and(|i| i != 0)))
}

fn flag_or(v: &Value, k: &str, default: bool) -> bool {
    if v.get(k).is_some() { flag(v, k) } else { default }
}

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Parses Sketch's `"{x, y}"` point strings.
fn parse_point(s: &str) -> Point {
    let mut it = s.trim_matches(|c| c == '{' || c == '}').split(',').map(|p| p.trim().parse::<f64>().unwrap_or(0.0));
    Point::new(it.next().unwrap_or(0.0), it.next().unwrap_or(0.0))
}

fn point_str(p: Point) -> String {
    format!("{{{}, {}}}", p.x, p.y)
}

/// A UUID from a Sketch object id. Sketch ids are usually UUIDs; others are hashed to one, so
/// references between objects still line up.
fn uuid_of(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap_or_else(|_| {
        let fnv = |offset: u64| s.bytes().fold(offset, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
        Uuid::from_u64_pair(fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142))
    })
}

fn id_str(u: Uuid) -> String {
    u.to_string().to_uppercase()
}

fn new_id() -> String {
    id_str(Uuid::new_v4())
}

// ---- Import ----------------------------------------------------------------------------------

/// Reads a `.sketch` file into a document.
pub fn import(bytes: &[u8]) -> Result<Document, String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a Sketch file: {e}"))?;
    let mut entry = |name: &str| -> Option<Vec<u8>> {
        let mut f = zip.by_name(name).ok()?;
        let mut buf = vec![];
        f.read_to_end(&mut buf).ok()?;
        Some(buf)
    };
    let json = |bytes: Option<Vec<u8>>, what: &str| -> Result<Value, String> {
        let bytes = bytes.ok_or_else(|| format!("missing {what}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("corrupt {what}: {e}"))
    };
    let document = json(entry("document.json"), "document.json")?;

    let mut cx = Importer { doc: Document { pages: vec![], ..Default::default() }, images: HashMap::new() };
    for r in list(&document, "pages") {
        let path = text(r, "_ref");
        let name = if path.ends_with(".json") { path.to_owned() } else { format!("{path}.json") };
        let page = json(entry(&name), &name)?;
        let mut p = Page::new(text(&page, "name"));
        p.id = PageId(uuid_of(text(&page, "do_objectID")));
        p.layers = list(&page, "layers").iter().filter_map(|l| cx.layer(l, &mut entry)).collect();
        cx.doc.pages.push(p);
    }
    if cx.doc.pages.is_empty() {
        cx.doc.pages.push(Page::new("Page 1"));
    }

    for s in list(&document["layerStyles"], "objects") {
        let style = cx.shared_style(s, false, &mut entry);
        cx.doc.layer_styles.push(style);
    }
    for s in list(&document["layerTextStyles"], "objects") {
        let style = cx.shared_style(s, true, &mut entry);
        cx.doc.text_styles.push(style);
    }
    for s in list(&document["sharedSwatches"], "objects") {
        cx.doc.swatches.push(swatch(s));
    }

    // Library components: their libraries' files aren't recorded, only names.
    let library = |doc: &mut Document, v: &Value| -> LibraryId {
        let id = LibraryId(uuid_of(text(v, "libraryID")));
        if doc.library(id).is_none() {
            doc.libraries.push(LibraryRef { id, name: text(v, "sourceLibraryName").to_owned(), path: String::new() });
        }
        id
    };
    for f in list(&document, "foreignSymbols") {
        let lib = library(&mut cx.doc, f);
        if let Some(mut master) = cx.layer(&f["symbolMaster"], &mut entry) {
            master.frame.x = 0.0;
            master.frame.y = 0.0;
            cx.doc.foreign_symbols.push(Foreign { library: lib, item: master });
        }
    }
    for (key, is_text) in [("foreignLayerStyles", false), ("foreignTextStyles", true)] {
        for f in list(&document, key) {
            let lib = library(&mut cx.doc, f);
            let item = cx.shared_style(&f["localSharedStyle"], is_text, &mut entry);
            cx.doc.foreign_styles.push(Foreign { library: lib, item });
        }
    }
    for f in list(&document, "foreignSwatches") {
        let lib = library(&mut cx.doc, f);
        cx.doc.foreign_swatches.push(Foreign { library: lib, item: swatch(&f["localSwatch"]) });
    }

    let mut doc = cx.doc;
    crate::symbols::sync_instances(&mut doc, &mut |_| {});
    Ok(doc)
}

fn swatch(v: &Value) -> Swatch {
    Swatch { id: SwatchId(uuid_of(text(v, "do_objectID"))), name: text(v, "name").to_owned(), color: color_in(&v["value"]) }
}

fn color_in(v: &Value) -> Color {
    Color::rgba(num(v, "red") as f32, num(v, "green") as f32, num(v, "blue") as f32, num_or(v, "alpha", 1.0) as f32)
}

fn swatch_of(v: &Value) -> Option<SwatchId> {
    let s = text(v, "swatchID");
    (!s.is_empty()).then(|| SwatchId(uuid_of(s)))
}

fn blend_in(v: &Value) -> BlendMode {
    let i = int(&v["contextSettings"], "blendMode");
    usize::try_from(i).ok().and_then(|i| BlendMode::ALL.get(i).copied()).unwrap_or_default()
}

fn marker_in(i: i64) -> Marker {
    usize::try_from(i).ok().and_then(|i| Marker::ALL.get(i).copied()).unwrap_or_default()
}

fn gradient_in(v: &Value) -> Gradient {
    // Sketch writes gradients for solid fills too; some older files leave them empty.
    if list(v, "stops").len() < 2 {
        return Gradient::default();
    }
    Gradient {
        kind: match int(v, "gradientType") {
            1 => GradientKind::Radial,
            2 => GradientKind::Angular,
            _ => GradientKind::Linear,
        },
        from: parse_point(text(v, "from")),
        to: parse_point(text(v, "to")),
        stops: list(v, "stops").iter().map(|s| GradientStop { position: num(s, "position"), color: color_in(&s["color"]) }).collect(),
    }
}

/// Sketch's resizing constraint: a bit mask where a cleared bit means "fixed".
fn resizing_in(mask: i64) -> Resizing {
    let fixed = |bit: i64| mask & bit == 0;
    Resizing { right: fixed(1), fix_width: fixed(2), left: fixed(4), bottom: fixed(8), fix_height: fixed(16), top: fixed(32) }
}

fn resizing_out(r: &Resizing) -> i64 {
    [(r.right, 1), (r.fix_width, 2), (r.left, 4), (r.bottom, 8), (r.fix_height, 16), (r.top, 32)]
        .iter()
        .fold(63, |mask, &(on, bit)| if on { mask & !bit } else { mask })
}

/// Family, weight and italic from a PostScript font name like `HelveticaNeue-BoldItalic`.
fn font_in(name: &str) -> (String, u16, bool) {
    let (family, style) = name.split_once('-').unwrap_or((name, "Regular"));
    let style_lc = style.to_lowercase();
    let italic = style_lc.contains("italic") || style_lc.contains("oblique");
    let weight_word = style_lc.replace("italic", "").replace("oblique", "");
    let weight = match weight_word.as_str() {
        "thin" | "hairline" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" | "heavy" => 800,
        "black" => 900,
        _ => 400,
    };
    // System fonts (".SFNS", "SFPro…") mean "the platform UI font".
    if family.starts_with('.') || family.starts_with("SFPro") || family.starts_with("SFNS") {
        return (String::new(), weight, italic);
    }
    // PostScript names drop spaces: "HelveticaNeue" → "Helvetica Neue".
    let mut spaced = String::new();
    let mut prev_lower = false;
    for c in family.chars() {
        if c.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = c.is_lowercase();
        spaced.push(c);
    }
    (spaced, weight, italic)
}

fn font_out(style: &TextStyle) -> String {
    let family = if style.font_family.is_empty() { ".SFNS".to_owned() } else { style.font_family.replace(' ', "") };
    let weight = WEIGHT_NAMES.iter().min_by_key(|(w, _)| w.abs_diff(style.font_weight)).map_or("Regular", |(_, n)| n).replace(' ', "");
    let suffix = match (weight.as_str(), style.italic) {
        ("Regular", true) => "Italic".to_owned(),
        (w, true) => format!("{w}Italic"),
        (w, false) => w.to_owned(),
    };
    format!("{family}-{suffix}")
}

/// Typography (and text color) from Sketch's string attributes.
fn text_style_in(a: &Value) -> (TextStyle, Option<Color>) {
    let font = &a["MSAttributedStringFontAttribute"]["attributes"];
    let (font_family, font_weight, italic) = font_in(text(font, "name"));
    let para = &a["paragraphStyle"];
    let line = num(para, "maximumLineHeight");
    let style = TextStyle {
        font_family,
        font_size: num_or(font, "size", 16.0),
        font_weight,
        italic,
        line_height: (line > 0.0).then_some(line),
        letter_spacing: num(a, "kerning"),
        alignment: match int(para, "alignment") {
            1 => TextAlign::Right,
            2 => TextAlign::Center,
            3 => TextAlign::Justify,
            _ => TextAlign::Left,
        },
        vertical_alignment: match int(a, "textStyleVerticalAlignmentKey") {
            1 => VerticalAlign::Middle,
            2 => VerticalAlign::Bottom,
            _ => VerticalAlign::Top,
        },
        underline: int(a, "underlineStyle") != 0,
        strikethrough: int(a, "strikethroughStyle") != 0,
        transform: match int(a, "MSAttributedStringTextTransformAttribute") {
            1 => TextTransform::Uppercase,
            2 => TextTransform::Lowercase,
            _ => TextTransform::None,
        },
    };
    let color = a.get("MSAttributedStringColorAttribute").map(color_in);
    (style, color)
}

struct Importer {
    doc: Document,
    /// Image references (`images/…`) to asset ids.
    images: HashMap<String, String>,
}

impl Importer {
    /// The asset for a Sketch image reference, read from the archive on first use.
    fn image(&mut self, r: &Value, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) -> Option<String> {
        let path = text(r, "_ref");
        if let Some(id) = self.images.get(path) {
            return Some(id.clone());
        }
        let bytes = entry(path).or_else(|| entry(&format!("{path}.png")))?;
        let id = self.doc.assets.add_image(bytes)?;
        self.images.insert(path.to_owned(), id.clone());
        Some(id)
    }

    fn style(&mut self, v: &Value, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) -> crate::Style {
        let shadow = |s: &Value| Shadow {
            enabled: flag_or(s, "isEnabled", true),
            color: color_in(&s["color"]),
            offset_x: num(s, "offsetX"),
            offset_y: num(s, "offsetY"),
            blur: num(s, "blurRadius"),
            spread: num(s, "spread"),
            swatch: swatch_of(&s["color"]),
        };
        let mut fills = vec![];
        for f in list(v, "fills") {
            let image = if int(f, "fillType") == 4 { self.image(&f["image"], entry) } else { None };
            fills.push(Fill {
                enabled: flag_or(f, "isEnabled", true),
                fill_type: match int(f, "fillType") {
                    1 => FillType::Gradient,
                    4 => FillType::Image,
                    _ => FillType::Color,
                },
                color: color_in(&f["color"]),
                gradient: gradient_in(&f["gradient"]),
                blend_mode: blend_in(f),
                swatch: swatch_of(&f["color"]),
                image,
                image_mode: match int(f, "patternFillType") {
                    0 => ImageMode::Tile,
                    2 => ImageMode::Stretch,
                    3 => ImageMode::Fit,
                    _ => ImageMode::Fill,
                },
                image_scale: num_or(f, "patternTileScale", 1.0),
            });
        }
        let opts = &v["borderOptions"];
        crate::Style {
            fills,
            borders: list(v, "borders")
                .iter()
                .map(|b| Border {
                    enabled: flag_or(b, "isEnabled", true),
                    fill_type: if int(b, "fillType") == 1 { FillType::Gradient } else { FillType::Color },
                    color: color_in(&b["color"]),
                    gradient: gradient_in(&b["gradient"]),
                    thickness: num_or(b, "thickness", 1.0),
                    position: match int(b, "position") {
                        1 => BorderPosition::Inside,
                        2 => BorderPosition::Outside,
                        _ => BorderPosition::Center,
                    },
                    swatch: swatch_of(&b["color"]),
                })
                .collect(),
            shadows: list(v, "shadows").iter().map(shadow).collect(),
            inner_shadows: list(v, "innerShadows").iter().map(shadow).collect(),
            border_options: BorderOptions {
                dash: list(opts, "dashPattern").iter().filter_map(Value::as_f64).collect(),
                cap: match int(opts, "lineCapStyle") {
                    1 => LineCap::Round,
                    2 => LineCap::Square,
                    _ => LineCap::Butt,
                },
                join: match int(opts, "lineJoinStyle") {
                    1 => LineJoin::Round,
                    2 => LineJoin::Bevel,
                    _ => LineJoin::Miter,
                },
                start_marker: marker_in(int(v, "startMarkerType")),
                end_marker: marker_in(int(v, "endMarkerType")),
            },
        }
    }

    fn shared_style(&mut self, v: &Value, is_text: bool, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) -> SharedStyle {
        let value = &v["value"];
        let mut style = self.style(value, entry);
        let text_style = is_text.then(|| {
            let (ts, color) = text_style_in(&value["textStyle"]["encodedAttributes"]);
            if style.fills.iter().all(|f| !f.enabled) {
                style.fills = vec![Fill::solid(color.unwrap_or(Color::BLACK))];
            }
            ts
        });
        SharedStyle { id: StyleId(uuid_of(text(v, "do_objectID"))), name: text(v, "name").to_owned(), style, text: text_style }
    }

    fn children(&mut self, v: &Value, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) -> Vec<Layer> {
        list(v, "layers").iter().filter_map(|l| self.layer(l, entry)).collect()
    }

    fn layer(&mut self, v: &Value, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) -> Option<Layer> {
        let f = &v["frame"];
        let frame = Frame::new(num(f, "x"), num(f, "y"), num(f, "width"), num(f, "height"));
        let class = text(v, "_class");
        let kind = match class {
            "artboard" | "symbolMaster" => LayerKind::Artboard(Artboard {
                children: self.children(v, entry),
                background: flag(v, "hasBackgroundColor").then(|| color_in(&v["backgroundColor"])),
                clip_content: true,
                resizes_content: flag(v, "resizesContent"),
                symbol_id: (class == "symbolMaster").then(|| SymbolId(uuid_of(text(v, "symbolID")))),
                ..Artboard::new()
            }),
            "group" => LayerKind::Group(Group { children: self.children(v, entry), stack: None }),
            "shapeGroup" => return self.shape_group(v, frame, entry),
            "rectangle" => {
                let radii: Vec<f64> = list(v, "points").iter().map(|p| num(p, "cornerRadius")).collect();
                let fixed = num(v, "fixedRadius");
                match radii[..] {
                    [a, b, c, d] if !(a == b && b == c && c == d) => {
                        LayerKind::Rectangle(Rectangle { corner_radius: a, corner_radii: Some([a, b, c, d]) })
                    }
                    [a, ..] => LayerKind::Rectangle(Rectangle { corner_radius: a, corner_radii: None }),
                    [] => LayerKind::Rectangle(Rectangle { corner_radius: fixed, corner_radii: None }),
                }
            }
            "oval" => LayerKind::Oval,
            "triangle" => LayerKind::Polygon(Polygon { sides: 3 }),
            "polygon" => LayerKind::Polygon(Polygon { sides: int(v, "numberOfPoints").max(3) as u32 }),
            "star" => LayerKind::Star(Star { points: int(v, "numberOfPoints").max(3) as u32, inner_ratio: num_or(v, "radius", 0.5) }),
            "shapePath" => LayerKind::Path(PathShape {
                subpaths: vec![subpath_in(v)],
                fill_rule: if int(&v["style"], "windingRule") == 1 { FillRule::EvenOdd } else { FillRule::NonZero },
            }),
            "text" => {
                let s = &v["attributedString"];
                // The first run's attributes, over the layer's text style (which has the rest).
                let mut attrs = v["style"]["textStyle"]["encodedAttributes"].as_object().cloned().unwrap_or_default();
                if let Some(run) = list(s, "attributes").first().and_then(|a| a["attributes"].as_object()) {
                    attrs.extend(run.clone());
                }
                let (style, color) = text_style_in(&Value::Object(attrs));
                let sizing = match int(v, "textBehaviour") {
                    1 => TextSizing::FixedWidth,
                    2 => TextSizing::Fixed,
                    _ => TextSizing::Auto,
                };
                let mut layer = self.common(v, frame, LayerKind::Text(TextLayer { text: text(s, "string").to_owned(), style, sizing }), entry);
                // The text color lives in the string attributes; a clear one means no fill.
                let color = color.unwrap_or(Color::BLACK);
                if layer.style.fills.iter().all(|f| !f.enabled) && color.a > 0.0 {
                    layer.style.fills = vec![Fill::solid(color)];
                }
                return Some(layer);
            }
            "bitmap" => LayerKind::Image(ImageLayer { image: self.image(&v["image"], entry)? }),
            "symbolInstance" => LayerKind::Symbol(SymbolInstance {
                symbol_id: SymbolId(uuid_of(text(v, "symbolID"))),
                overrides: list(v, "overrideValues").iter().filter_map(override_in).collect(),
                expanded: vec![],
            }),
            "slice" => LayerKind::Slice,
            _ => return None,
        };
        Some(self.common(v, frame, kind, entry))
    }

    /// Properties every layer has.
    fn common(&mut self, v: &Value, frame: Frame, kind: LayerKind, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) -> Layer {
        let mut l = Layer::new(text(v, "name"), frame, kind);
        l.id = LayerId(uuid_of(text(v, "do_objectID")));
        l.visible = flag_or(v, "isVisible", true);
        l.locked = flag(v, "isLocked");
        l.rotation = (-num(v, "rotation")).rem_euclid(360.0);
        l.flip_horizontal = flag(v, "isFlippedHorizontal");
        l.flip_vertical = flag(v, "isFlippedVertical");
        let style = &v["style"];
        l.opacity = num_or(&style["contextSettings"], "opacity", 1.0);
        l.blend_mode = blend_in(style);
        l.is_mask = flag(v, "hasClippingMask");
        l.resizing = resizing_in(v.get("resizingConstraint").and_then(Value::as_i64).unwrap_or(63));
        l.style = self.style(style, entry);
        let shared = text(v, "sharedStyleID");
        l.shared_style = (!shared.is_empty()).then(|| StyleId(uuid_of(shared)));
        l.exports = list(&v["exportOptions"], "exportFormats")
            .iter()
            .map(|e| ExportFormat {
                scale: num_or(e, "scale", 1.0),
                suffix: text(e, "name").to_owned(),
                format: match text(e, "fileFormat") {
                    "jpg" | "jpeg" => FileFormat::Jpg,
                    "webp" => FileFormat::Webp,
                    "svg" => FileFormat::Svg,
                    "pdf" => FileFormat::Pdf,
                    _ => FileFormat::Png,
                },
            })
            .collect();
        l
    }

    /// A shape group's children combined by their boolean operations into one path layer.
    fn shape_group(&mut self, v: &Value, frame: Frame, entry: &mut impl FnMut(&str) -> Option<Vec<u8>>) -> Option<Layer> {
        let mut combined: Option<BezPath> = None;
        for c in list(v, "layers") {
            let Some(child) = self.layer(c, entry) else { continue };
            let path = match child.kind {
                // Nested shape groups arrive as paths already.
                LayerKind::Path(_) | LayerKind::Rectangle(_) | LayerKind::Oval | LayerKind::Polygon(_) | LayerKind::Star(_) => {
                    child.local_transform() * child.local_path()?
                }
                _ => continue,
            };
            combined = Some(match (combined, int(c, "booleanOperation")) {
                (None, _) => path,
                (Some(mut acc), op) if !(0..=3).contains(&op) => {
                    acc.extend(path.elements().iter().copied());
                    acc
                }
                (Some(acc), op) => {
                    let op = [crate::ops::BooleanOp::Union, crate::ops::BooleanOp::Subtract, crate::ops::BooleanOp::Intersect, crate::ops::BooleanOp::Difference]
                        [op as usize];
                    crate::ops::path_boolean(&acc, &path, op.sweep_op()).unwrap_or(acc)
                }
            });
        }
        let path = combined?;
        let mut shape = PathShape::from_bez_path(&path);
        shape.fill_rule = if int(&v["style"], "windingRule") == 1 { FillRule::EvenOdd } else { FillRule::NonZero };
        let (bounds, norm) = shape.fit();
        let inner = Frame::new(frame.x + bounds.x0, frame.y + bounds.y0, bounds.width(), bounds.height());
        Some(self.common(v, inner, LayerKind::Path(norm), entry))
    }
}

fn subpath_in(v: &Value) -> Subpath {
    let points = list(v, "points")
        .iter()
        .map(|p| {
            let point = parse_point(text(p, "point"));
            let from = parse_point(text(p, "curveFrom"));
            let to = parse_point(text(p, "curveTo"));
            // Older files lack the flags; a handle away from its point is a curve.
            let has = |k: &str, h: Point| if p.get(k).is_some() { flag(p, k) } else { (h - point).hypot() > 1e-9 };
            CurvePoint {
                point,
                curve_from: has("hasCurveFrom", from).then_some(from),
                curve_to: has("hasCurveTo", to).then_some(to),
                mode: match int(p, "curveMode") {
                    2 => PointMode::Mirrored,
                    3 => PointMode::Asymmetric,
                    4 => PointMode::Disconnected,
                    _ => PointMode::Straight,
                },
            }
        })
        .collect();
    Subpath { points, closed: flag(v, "isClosed") }
}

/// `"ID1/ID2_stringValue"` style overrides.
fn override_in(v: &Value) -> Option<Override> {
    let (path, property) = text(v, "overrideName").rsplit_once('_')?;
    let path: Vec<LayerId> = path.split('/').map(|s| LayerId(uuid_of(s))).collect();
    let s = v["value"].as_str().unwrap_or("");
    let value = match property {
        "stringValue" => OverrideValue::Text(s.to_owned()),
        "symbolID" => OverrideValue::Symbol((!s.is_empty()).then(|| SymbolId(uuid_of(s)))),
        "layerStyle" | "textStyle" if !s.is_empty() => OverrideValue::Style(StyleId(uuid_of(s))),
        _ => return None,
    };
    Some(Override { path, value })
}

// ---- Export ----------------------------------------------------------------------------------

/// Writes a document as a `.sketch` file.
pub fn export(doc: &Document) -> Result<Vec<u8>, String> {
    let mut cx = Exporter { images: vec![] };
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let err = |e: &dyn std::fmt::Display| e.to_string();

    let mut pages_and_artboards = Map::new();
    let mut page_refs = vec![];
    for page in &doc.pages {
        let pid = id_str(page.id.0);
        let layers: Vec<Value> = page.layers.iter().map(|l| cx.layer(l)).collect();
        let json = json!({
            "_class": "page",
            "do_objectID": pid,
            "booleanOperation": -1,
            "exportOptions": export_options(&[]),
            "frame": rect(&Frame::new(0.0, 0.0, 0.0, 0.0)),
            "hasClickThrough": true,
            "horizontalRulerData": ruler(),
            "verticalRulerData": ruler(),
            "isFixedToViewport": false,
            "isFlippedHorizontal": false,
            "isFlippedVertical": false,
            "isLocked": false,
            "isTemplate": false,
            "isVisible": true,
            "layerListExpandedType": 0,
            "name": page.name,
            "nameIsFixed": false,
            "resizingConstraint": 63,
            "resizingType": 0,
            "rotation": 0,
            "shouldBreakMaskChain": false,
            "style": cx.style(&Default::default(), None),
            "groupLayout": { "_class": "MSImmutableFreeformGroupLayout" },
            "layers": layers,
        });
        let artboards: Map<String, Value> = page
            .layers
            .iter()
            .filter(|l| l.is_artboard())
            .map(|l| (id_str(l.id.0), json!({ "name": l.name })))
            .collect();
        pages_and_artboards.insert(pid.clone(), json!({ "name": page.name, "artboards": artboards }));
        zip.start_file(format!("pages/{pid}.json"), opts).map_err(|e| err(&e))?;
        zip.write_all(&serde_json::to_vec(&json).map_err(|e| err(&e))?).map_err(|e| err(&e))?;
        page_refs.push(json!({ "_class": "MSJSONFileReference", "_ref_class": "MSImmutablePage", "_ref": format!("pages/{pid}") }));
    }

    let shared = |cx: &mut Exporter, s: &SharedStyle| {
        json!({ "_class": "sharedStyle", "do_objectID": id_str(s.id.0), "name": s.name, "value": cx.style(&s.style, s.text.as_ref()) })
    };
    let swatch_out = |s: &Swatch| json!({ "_class": "swatch", "do_objectID": id_str(s.id.0), "name": s.name, "value": color_out(s.color, None) });
    let library_name = |id: LibraryId| doc.library(id).map_or_else(String::new, |l| l.name.clone());
    let layer_styles: Vec<Value> = doc.layer_styles.iter().map(|s| shared(&mut cx, s)).collect();
    let text_styles: Vec<Value> = doc.text_styles.iter().map(|s| shared(&mut cx, s)).collect();
    let foreign_symbols: Vec<Value> = doc
        .foreign_symbols
        .iter()
        .map(|f| {
            let master = cx.layer(&f.item);
            json!({
                "_class": "MSImmutableForeignSymbol",
                "do_objectID": new_id(),
                "libraryID": id_str(f.library.0),
                "sourceLibraryName": library_name(f.library),
                "symbolPrivate": false,
                "originalMaster": master,
                "symbolMaster": master,
            })
        })
        .collect();
    let foreign_styles = |cx: &mut Exporter, text: bool| -> Vec<Value> {
        doc.foreign_styles
            .iter()
            .filter(|f| f.item.is_text() == text)
            .map(|f| {
                json!({
                    "_class": if text { "MSImmutableForeignTextStyle" } else { "MSImmutableForeignLayerStyle" },
                    "do_objectID": new_id(),
                    "libraryID": id_str(f.library.0),
                    "sourceLibraryName": library_name(f.library),
                    "symbolPrivate": false,
                    "remoteStyleID": id_str(f.item.id.0),
                    "localSharedStyle": shared(cx, &f.item),
                })
            })
            .collect()
    };
    let foreign_layer_styles = foreign_styles(&mut cx, false);
    let foreign_text_styles = foreign_styles(&mut cx, true);
    let foreign_swatches: Vec<Value> = doc
        .foreign_swatches
        .iter()
        .map(|f| {
            json!({
                "_class": "MSImmutableForeignSwatch",
                "do_objectID": new_id(),
                "libraryID": id_str(f.library.0),
                "sourceLibraryName": library_name(f.library),
                "symbolPrivate": false,
                "remoteSwatchID": id_str(f.item.id.0),
                "localSwatch": swatch_out(&f.item),
            })
        })
        .collect();

    let document = json!({
        "_class": "document",
        "do_objectID": new_id(),
        "assets": {
            "_class": "assetCollection",
            "do_objectID": new_id(),
            "images": [],
            "colorAssets": [],
            "exportPresets": [],
            "gradientAssets": [],
            "imageCollection": { "_class": "imageCollection", "images": {} },
            "colors": doc.colors.iter().map(|c| color_out(*c, None)).collect::<Vec<_>>(),
            "gradients": [],
        },
        "colorSpace": 1,
        "currentPageIndex": 0,
        "foreignLayerStyles": foreign_layer_styles,
        "foreignSymbols": foreign_symbols,
        "foreignTextStyles": foreign_text_styles,
        "foreignSwatches": foreign_swatches,
        "layerStyles": { "_class": "sharedStyleContainer", "do_objectID": new_id(), "objects": layer_styles },
        "layerTextStyles": { "_class": "sharedTextStyleContainer", "do_objectID": new_id(), "objects": text_styles },
        "sharedSwatches": { "_class": "swatchContainer", "do_objectID": new_id(), "objects": doc.swatches.iter().map(swatch_out).collect::<Vec<_>>() },
        "perDocumentLibraries": [],
        "pages": page_refs,
    });
    let meta = json!({
        "commit": "scrawl",
        "pagesAndArtboards": pages_and_artboards,
        "version": 146,
        "compatibilityVersion": 99,
        "coeditCompatibilityVersion": 145,
        "app": "com.bohemiancoding.sketch3",
        "autosaved": 0,
        "variant": "NONAPPSTORE",
        "created": { "commit": "scrawl", "appVersion": "98", "build": 0, "app": "com.bohemiancoding.sketch3", "compatibilityVersion": 99, "coeditCompatibilityVersion": 145, "version": 146, "variant": "NONAPPSTORE" },
        "saveHistory": [format!("SCRAWL.{}", env!("CARGO_PKG_VERSION"))],
        "appVersion": "98",
        "build": 0,
    });
    for (name, value) in [("document.json", &document), ("meta.json", &meta), ("user.json", &json!({ "document": { "pageListHeight": 200, "pageListCollapsed": 0 } }))] {
        zip.start_file(name, opts).map_err(|e| err(&e))?;
        zip.write_all(&serde_json::to_vec(value).map_err(|e| err(&e))?).map_err(|e| err(&e))?;
    }
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for id in std::mem::take(&mut cx.images) {
        if let Some(bytes) = doc.assets.get(&id) {
            zip.start_file(format!("images/{id}"), stored).map_err(|e| err(&e))?;
            zip.write_all(bytes).map_err(|e| err(&e))?;
        }
    }
    Ok(zip.finish().map_err(|e| err(&e))?.into_inner())
}

fn rect(f: &Frame) -> Value {
    json!({ "_class": "rect", "constrainProportions": false, "x": f.x, "y": f.y, "width": f.width, "height": f.height })
}

fn ruler() -> Value {
    json!({ "_class": "rulerData", "base": 0, "guides": [] })
}

fn color_out(c: Color, swatch: Option<SwatchId>) -> Value {
    let mut v = json!({ "_class": "color", "alpha": c.a, "red": c.r, "green": c.g, "blue": c.b });
    if let Some(s) = swatch {
        v["swatchID"] = json!(id_str(s.0));
    }
    v
}

fn export_options(formats: &[ExportFormat]) -> Value {
    let formats: Vec<Value> = formats
        .iter()
        .map(|f| {
            json!({
                "_class": "exportFormat",
                "absoluteSize": 0,
                "fileFormat": f.format.extension(),
                "name": f.suffix,
                "namingScheme": 0,
                "scale": f.scale,
                "visibleScaleType": 0,
            })
        })
        .collect();
    json!({ "_class": "exportOptions", "exportFormats": formats, "includedLayerIds": [], "layerOptions": 0, "shouldTrim": false })
}

fn gradient_out(g: &Gradient) -> Value {
    json!({
        "_class": "gradient",
        "elipseLength": 0,
        "from": point_str(g.from),
        "to": point_str(g.to),
        "gradientType": match g.kind { GradientKind::Linear => 0, GradientKind::Radial => 1, GradientKind::Angular => 2 },
        "stops": g.stops.iter().map(|s| json!({ "_class": "gradientStop", "position": s.position, "color": color_out(s.color, None) })).collect::<Vec<_>>(),
    })
}

fn context(blend: BlendMode, opacity: f64) -> Value {
    let i = BlendMode::ALL.iter().position(|b| *b == blend).unwrap_or(0);
    json!({ "_class": "graphicsContextSettings", "blendMode": i, "opacity": opacity })
}

/// Sketch's attributes for a text style and color. The schema allows decorations and case only in
/// a layer's text style (`full`), not in attributed-string runs.
fn text_attrs(ts: &TextStyle, color: Color, full: bool) -> Value {
    let mut a = json!({
        "MSAttributedStringFontAttribute": { "_class": "fontDescriptor", "attributes": { "name": font_out(ts), "size": ts.font_size } },
        "MSAttributedStringColorAttribute": color_out(color, None),
        "textStyleVerticalAlignmentKey": match ts.vertical_alignment { VerticalAlign::Top => 0, VerticalAlign::Middle => 1, VerticalAlign::Bottom => 2 },
        "kerning": ts.letter_spacing,
        "paragraphStyle": { "_class": "paragraphStyle", "alignment": match ts.alignment { TextAlign::Left => 0, TextAlign::Right => 1, TextAlign::Center => 2, TextAlign::Justify => 3 } },
        "underlineStyle": u8::from(ts.underline),
        "strikethroughStyle": u8::from(ts.strikethrough),
        "MSAttributedStringTextTransformAttribute": match ts.transform { TextTransform::None => 0, TextTransform::Uppercase => 1, TextTransform::Lowercase => 2 },
    });
    if let Some(lh) = ts.line_height {
        a["paragraphStyle"]["maximumLineHeight"] = json!(lh);
        a["paragraphStyle"]["minimumLineHeight"] = json!(lh);
    }
    if !full && let Some(map) = a.as_object_mut() {
        for k in ["underlineStyle", "strikethroughStyle", "MSAttributedStringTextTransformAttribute"] {
            map.remove(k);
        }
    }
    a
}

fn curve_point(p: &CurvePoint) -> Value {
    json!({
        "_class": "curvePoint",
        "cornerRadius": 0,
        "cornerStyle": 0,
        "curveFrom": point_str(p.curve_from.unwrap_or(p.point)),
        "curveTo": point_str(p.curve_to.unwrap_or(p.point)),
        "hasCurveFrom": p.curve_from.is_some(),
        "hasCurveTo": p.curve_to.is_some(),
        "curveMode": match p.mode { PointMode::Straight => 1, PointMode::Mirrored => 2, PointMode::Asymmetric => 3, PointMode::Disconnected => 4 },
        "point": point_str(p.point),
    })
}

struct Exporter {
    /// Image assets referenced so far, to add to the archive.
    images: Vec<String>,
}

impl Exporter {
    fn image_ref(&mut self, id: &str) -> Value {
        if !self.images.iter().any(|i| i == id) {
            self.images.push(id.to_owned());
        }
        json!({ "_class": "MSJSONFileReference", "_ref_class": "MSImageData", "_ref": format!("images/{id}") })
    }

    fn style(&mut self, s: &crate::Style, text: Option<&TextStyle>) -> Value {
        let shadow = |sh: &Shadow, inner: bool| {
            json!({
                "_class": if inner { "innerShadow" } else { "shadow" },
                "isEnabled": sh.enabled,
                "blurRadius": sh.blur,
                "color": color_out(sh.color, sh.swatch),
                "contextSettings": context(BlendMode::Normal, 1.0),
                "offsetX": sh.offset_x,
                "offsetY": sh.offset_y,
                "spread": sh.spread,
            })
        };
        let fills: Vec<Value> = s
            .fills
            .iter()
            .map(|f| {
                let mut v = json!({
                    "_class": "fill",
                    "isEnabled": f.enabled,
                    "fillType": match f.fill_type { FillType::Color => 0, FillType::Gradient => 1, FillType::Image => 4 },
                    "color": color_out(f.color, f.swatch),
                    "contextSettings": context(f.blend_mode, 1.0),
                    "gradient": gradient_out(&f.gradient),
                    "noiseIndex": 0,
                    "noiseIntensity": 0,
                    "patternFillType": match f.image_mode { ImageMode::Tile => 0, ImageMode::Fill => 1, ImageMode::Stretch => 2, ImageMode::Fit => 3 },
                    "patternTileScale": f.image_scale,
                });
                if let Some(id) = &f.image {
                    v["image"] = self.image_ref(id);
                }
                v
            })
            .collect();
        let o = &s.border_options;
        let marker = |m: Marker| Marker::ALL.iter().position(|x| *x == m).unwrap_or(0);
        let mut v = json!({
            "_class": "style",
            "do_objectID": new_id(),
            "endMarkerType": marker(o.end_marker),
            "startMarkerType": marker(o.start_marker),
            "miterLimit": 10,
            "windingRule": 1,
            "blur": { "_class": "blur", "isEnabled": false, "center": "{0.5, 0.5}", "motionAngle": 0, "radius": 10, "saturation": 1, "type": 0 },
            "borderOptions": {
                "_class": "borderOptions",
                "isEnabled": true,
                "dashPattern": o.dash,
                "lineCapStyle": match o.cap { LineCap::Butt => 0, LineCap::Round => 1, LineCap::Square => 2 },
                "lineJoinStyle": match o.join { LineJoin::Miter => 0, LineJoin::Round => 1, LineJoin::Bevel => 2 },
            },
            "borders": s.borders.iter().map(|b| json!({
                "_class": "border",
                "isEnabled": b.enabled,
                "fillType": if b.fill_type == FillType::Gradient { 1 } else { 0 },
                "color": color_out(b.color, b.swatch),
                "contextSettings": context(BlendMode::Normal, 1.0),
                "gradient": gradient_out(&b.gradient),
                "position": match b.position { BorderPosition::Center => 0, BorderPosition::Inside => 1, BorderPosition::Outside => 2 },
                "thickness": b.thickness,
            })).collect::<Vec<_>>(),
            "colorControls": { "_class": "colorControls", "isEnabled": false, "brightness": 0, "contrast": 1, "hue": 0, "saturation": 1 },
            "contextSettings": context(BlendMode::Normal, 1.0),
            "fills": fills,
            "innerShadows": s.inner_shadows.iter().map(|x| shadow(x, true)).collect::<Vec<_>>(),
            "shadows": s.shadows.iter().map(|x| shadow(x, false)).collect::<Vec<_>>(),
        });
        if let Some(ts) = text {
            let color = s.fills.iter().rev().find(|f| f.enabled && f.fill_type == FillType::Color).map_or(Color::BLACK, |f| f.color);
            v["textStyle"] = json!({
                "_class": "textStyle",
                "encodedAttributes": text_attrs(ts, color, true),
                "verticalAlignment": match ts.vertical_alignment { VerticalAlign::Top => 0, VerticalAlign::Middle => 1, VerticalAlign::Bottom => 2 },
            });
        }
        v
    }

    /// A shape's outline as Sketch curve points, normalized to its frame.
    fn points(l: &Layer) -> Vec<Subpath> {
        match &l.kind {
            LayerKind::Path(p) => p.subpaths.clone(),
            _ => l.editable_path().map(|p| p.fit().1.subpaths).unwrap_or_default(),
        }
    }

    fn layer(&mut self, l: &Layer) -> Value {
        let text_style = match &l.kind {
            LayerKind::Text(t) => Some(&t.style),
            _ => None,
        };
        let mut style = self.style(&l.style, text_style);
        style["contextSettings"] = context(l.blend_mode, l.opacity);
        let winding = if l.fill_rule() == FillRule::EvenOdd { 1 } else { 0 };
        style["windingRule"] = json!(winding);
        let mut v = json!({
            "do_objectID": id_str(l.id.0),
            "booleanOperation": -1,
            "exportOptions": export_options(&l.exports),
            "frame": rect(&l.frame),
            "isFixedToViewport": false,
            "isFlippedHorizontal": l.flip_horizontal,
            "isFlippedVertical": l.flip_vertical,
            "isLocked": l.locked,
            "isTemplate": false,
            "isVisible": l.visible,
            "layerListExpandedType": 0,
            "name": l.name,
            "nameIsFixed": true,
            "resizingConstraint": resizing_out(&l.resizing),
            "resizingType": 0,
            "rotation": if l.rotation == 0.0 { 0.0 } else { -l.rotation },
            "shouldBreakMaskChain": false,
            "hasClippingMask": l.is_mask,
            "clippingMaskMode": 0,
            "style": style,
        });
        if let Some(s) = l.shared_style {
            v["sharedStyleID"] = json!(id_str(s.0));
        }
        let shape = |v: &mut Value, class: &str, sub: &Subpath| {
            v["_class"] = json!(class);
            v["edited"] = json!(class == "shapePath");
            v["isClosed"] = json!(sub.closed);
            v["pointRadiusBehaviour"] = json!(1);
            v["points"] = json!(sub.points.iter().map(curve_point).collect::<Vec<_>>());
        };
        match &l.kind {
            LayerKind::Artboard(a) => {
                v["_class"] = json!(if a.symbol_id.is_some() { "symbolMaster" } else { "artboard" });
                v["layers"] = json!(a.children.iter().map(|c| self.layer(c)).collect::<Vec<_>>());
                v["backgroundColor"] = color_out(a.background.unwrap_or(Color::WHITE), None);
                v["hasBackgroundColor"] = json!(a.background.is_some());
                v["includeBackgroundColorInExport"] = json!(true);
                v["hasClickThrough"] = json!(false);
                v["horizontalRulerData"] = ruler();
                v["verticalRulerData"] = ruler();
                v["isFlowHome"] = json!(false);
                v["resizesContent"] = json!(a.resizes_content);
                v["groupLayout"] = json!({ "_class": "MSImmutableFreeformGroupLayout" });
                if let Some(s) = a.symbol_id {
                    v["symbolID"] = json!(id_str(s.0));
                    v["allowsOverrides"] = json!(true);
                    v["overrideProperties"] = json!([]);
                    v["includeBackgroundColorInInstance"] = json!(false);
                }
            }
            LayerKind::Group(g) => {
                v["_class"] = json!("group");
                v["hasClickThrough"] = json!(false);
                v["groupLayout"] = json!({ "_class": "MSImmutableFreeformGroupLayout" });
                v["layers"] = json!(g.children.iter().map(|c| self.layer(c)).collect::<Vec<_>>());
            }
            LayerKind::Rectangle(r) => {
                let radii = r.corner_radii.unwrap_or([r.corner_radius; 4]);
                let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
                let points: Vec<Value> = corners
                    .iter()
                    .zip(radii)
                    .map(|(&(x, y), radius)| {
                        let mut p = curve_point(&CurvePoint { point: Point::new(x, y), curve_from: None, curve_to: None, mode: PointMode::Straight });
                        p["cornerRadius"] = json!(radius);
                        p
                    })
                    .collect();
                v["_class"] = json!("rectangle");
                v["edited"] = json!(false);
                v["isClosed"] = json!(true);
                v["pointRadiusBehaviour"] = json!(1);
                v["points"] = json!(points);
                v["fixedRadius"] = json!(r.corner_radius);
                v["hasConvertedToNewRoundCorners"] = json!(true);
                v["needsConvertionToNewRoundCorners"] = json!(false);
            }
            LayerKind::Oval | LayerKind::Polygon(_) | LayerKind::Star(_) => {
                let class = match &l.kind {
                    LayerKind::Oval => "oval",
                    LayerKind::Polygon(p) if p.sides == 3 => "triangle",
                    LayerKind::Polygon(_) => "polygon",
                    _ => "star",
                };
                let subs = Self::points(l);
                if let Some(sub) = subs.first() {
                    shape(&mut v, class, sub);
                }
                match &l.kind {
                    LayerKind::Polygon(p) => v["numberOfPoints"] = json!(p.sides),
                    LayerKind::Star(s) => {
                        v["numberOfPoints"] = json!(s.points);
                        v["radius"] = json!(s.inner_ratio);
                    }
                    _ => {}
                }
            }
            LayerKind::Path(p) if p.subpaths.len() == 1 => shape(&mut v, "shapePath", &p.subpaths[0]),
            LayerKind::Path(p) => {
                // Several subpaths: a shape group of one path each, combined like the fill rule.
                let op = if p.fill_rule == FillRule::EvenOdd { 3 } else { 0 };
                let children: Vec<Value> = p
                    .subpaths
                    .iter()
                    .enumerate()
                    .map(|(i, sub)| {
                        let mut c = json!({
                            "do_objectID": new_id(),
                            "booleanOperation": if i == 0 { -1 } else { op },
                            "exportOptions": export_options(&[]),
                            "frame": rect(&Frame::new(0.0, 0.0, l.frame.width, l.frame.height)),
                            "isFixedToViewport": false,
                            "isFlippedHorizontal": false,
                            "isFlippedVertical": false,
                            "isLocked": false,
                            "isTemplate": false,
                            "isVisible": true,
                            "layerListExpandedType": 0,
                            "name": "Path",
                            "nameIsFixed": false,
                            "resizingConstraint": 63,
                            "resizingType": 0,
                            "rotation": 0,
                            "shouldBreakMaskChain": false,
                            "style": self.style(&Default::default(), None),
                        });
                        shape(&mut c, "shapePath", sub);
                        c
                    })
                    .collect();
                v["_class"] = json!("shapeGroup");
                v["windingRule"] = json!(if p.fill_rule == FillRule::EvenOdd { 1 } else { 0 });
                v["hasClickThrough"] = json!(false);
                v["groupLayout"] = json!({ "_class": "MSImmutableFreeformGroupLayout" });
                v["layers"] = json!(children);
            }
            LayerKind::Text(t) => {
                let color = match l.style.fills.iter().rev().find(|f| f.enabled) {
                    Some(f) if f.fill_type == FillType::Color => f.color,
                    Some(_) => Color::BLACK,
                    // Outline-only text.
                    None => Color::TRANSPARENT,
                };
                v["_class"] = json!("text");
                v["attributedString"] = json!({
                    "_class": "attributedString",
                    "string": t.text,
                    "attributes": [{
                        "_class": "stringAttribute",
                        "location": 0,
                        "length": t.text.encode_utf16().count(),
                        "attributes": text_attrs(&t.style, color, false),
                    }],
                });
                v["automaticallyDrawOnUnderlyingPath"] = json!(false);
                v["dontSynchroniseWithSymbol"] = json!(false);
                v["glyphBounds"] = json!(format!("{{{{0, 0}}, {{{}, {}}}}}", l.frame.width, l.frame.height));
                v["lineSpacingBehaviour"] = json!(2);
                v["textBehaviour"] = json!(match t.sizing { TextSizing::Auto => 0, TextSizing::FixedWidth => 1, TextSizing::Fixed => 2 });
            }
            LayerKind::Symbol(s) => {
                v["_class"] = json!("symbolInstance");
                v["symbolID"] = json!(id_str(s.symbol_id.0));
                v["scale"] = json!(1);
                v["horizontalSpacing"] = json!(0);
                v["verticalSpacing"] = json!(0);
                v["overrideValues"] = json!(s.overrides.iter().map(override_out).collect::<Vec<_>>());
            }
            LayerKind::Image(img) => {
                v["_class"] = json!("bitmap");
                v["image"] = self.image_ref(&img.image);
                v["clippingMask"] = json!("{{0, 0}, {1, 1}}");
                v["fillReplacesImage"] = json!(false);
                v["intendedDPI"] = json!(72);
            }
            LayerKind::Slice => {
                v["_class"] = json!("slice");
                v["hasBackgroundColor"] = json!(false);
                v["backgroundColor"] = color_out(Color::WHITE, None);
            }
        }
        v
    }
}

fn override_out(o: &Override) -> Value {
    let path: Vec<String> = o.path.iter().map(|id| id_str(id.0)).collect();
    let (property, value) = match &o.value {
        OverrideValue::Text(s) => ("stringValue", s.clone()),
        OverrideValue::Symbol(s) => ("symbolID", s.map_or_else(String::new, |s| id_str(s.0))),
        OverrideValue::Style(s) => ("layerStyle", id_str(s.0)),
    };
    json!({ "_class": "overrideValue", "overrideName": format!("{}_{property}", path.join("/")), "value": value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    #[test]
    fn helpers() {
        assert_eq!(parse_point("{0.5, 1}"), Point::new(0.5, 1.0));
        assert_eq!(font_in("HelveticaNeue-BoldItalic"), ("Helvetica Neue".to_owned(), 700, true));
        assert_eq!(font_in(".SFNS-Regular"), (String::new(), 400, false));
        let ts = TextStyle { font_family: "Helvetica Neue".into(), font_weight: 600, italic: true, ..Default::default() };
        assert_eq!(font_out(&ts), "HelveticaNeue-SemiboldItalic");
        let r = Resizing { left: true, right: true, top: true, bottom: true, ..Default::default() };
        assert_eq!(resizing_out(&r), 18);
        assert_eq!(resizing_in(18), r);
        assert_eq!(resizing_in(63), Resizing::default());
        assert_eq!(uuid_of("not-a-uuid"), uuid_of("not-a-uuid"));
    }

    /// Everything Scrawl and Sketch share survives a trip through a `.sketch` file.
    #[test]
    fn sample_round_trips_through_sketch() {
        let doc = crate::sample::sample_document();
        let back = import(&export(&doc).unwrap()).unwrap();
        assert_eq!(back.pages.len(), doc.pages.len());
        for (a, b) in doc.pages.iter().zip(&back.pages) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.name, b.name);
            let (mut la, mut lb) = (vec![], vec![]);
            a.walk(|l| la.push(l));
            b.walk(|l| lb.push(l));
            assert_eq!(la.len(), lb.len(), "{}", a.name);
            for (x, y) in la.iter().zip(&lb) {
                assert_eq!(x.id, y.id);
                assert_eq!(x.name, y.name);
                assert_eq!(x.kind.type_name(), y.kind.type_name(), "{}", x.name);
                assert!((x.frame.x - y.frame.x).abs() < 1e-9 && (x.frame.width - y.frame.width).abs() < 1e-9, "{}", x.name);
                assert_eq!(x.rotation, y.rotation);
                assert_eq!(x.resizing, y.resizing, "{}", x.name);
                assert_eq!(x.style.fills, y.style.fills, "{}", x.name);
                assert_eq!(x.style.borders, y.style.borders, "{}", x.name);
                assert_eq!(x.style.shadows, y.style.shadows, "{}", x.name);
                assert_eq!(x.shared_style, y.shared_style);
                if let (LayerKind::Text(p), LayerKind::Text(q)) = (&x.kind, &y.kind) {
                    assert_eq!(p, q);
                }
                if let (LayerKind::Symbol(p), LayerKind::Symbol(q)) = (&x.kind, &y.kind) {
                    assert_eq!((p.symbol_id, &p.overrides), (q.symbol_id, &q.overrides));
                }
                if let (LayerKind::Path(p), LayerKind::Path(q)) = (&x.kind, &y.kind)
                    && p.subpaths.len() == 1
                {
                    assert_eq!(p, q);
                }
            }
        }
        assert_eq!(back.layer_styles, doc.layer_styles);
        assert_eq!(back.text_styles, doc.text_styles);
        assert_eq!(back.swatches, doc.swatches);
    }

    #[test]
    fn images_and_library_components_round_trip() {
        let library = crate::sample::sample_document();
        let mut doc = Document::default();
        let lib = doc.add_library("Kit", "/libs/Kit.scrawl");
        let button = library.all_symbols().into_iter().find(|(_, n)| n == "Icon Button").unwrap().0;
        assert!(doc.import(lib, &library, crate::library::Component::Symbol(button)));
        let png = [0x89, b'P', b'N', b'G', 9, 9, 9].to_vec();
        let id = doc.assets.add_image(png.clone()).unwrap();
        let mut photo = Layer::image(id.clone(), Frame::new(10.0, 10.0, 40.0, 30.0));
        photo.style.fills = vec![Fill::image(id.clone(), ImageMode::Fit)];
        doc.pages[0].layers.push(photo);
        doc.pages[0].layers.push(Layer::new("Btn", Frame::new(0.0, 100.0, 180.0, 44.0), LayerKind::Symbol(SymbolInstance::new(button))));

        let bytes = export(&doc).unwrap();
        if let Some(path) = std::env::var_os("SCRAWL_SKETCH_OUT") {
            std::fs::write(path, &bytes).unwrap();
        }
        let back = import(&bytes).unwrap();
        assert_eq!(&back.assets.get(&id).unwrap()[..], &png[..]);
        assert_eq!(back.pages[0].layers[0].style.fills[0].image.as_deref(), Some(id.as_str()));
        assert_eq!(back.libraries.len(), 1);
        assert_eq!(back.libraries[0].name, "Kit");
        assert_eq!(back.foreign_symbols.len(), doc.foreign_symbols.len());
        assert_eq!(back.foreign_swatches.len(), doc.foreign_swatches.len());
        let LayerKind::Symbol(inst) = &back.pages[0].layers[1].kind else { panic!() };
        assert!(!inst.expanded.is_empty(), "library symbols still resolve");
    }

    /// A small file shaped like Sketch's own output, exercising what our exports never produce.
    #[test]
    fn imports_sketch_style_json() {
        let rect = |id: &str, x: f64, op: i64| {
            json!({ "_class": "rectangle", "do_objectID": id, "name": "R", "booleanOperation": op,
                "frame": { "_class": "rect", "x": x, "y": 0, "width": 40, "height": 40 }, "fixedRadius": 0,
                "points": [], "isClosed": true, "style": { "_class": "style" } })
        };
        let page = json!({ "_class": "page", "do_objectID": "8F4C0A35-6A0C-4D3B-9C4E-1C1C1C1C1C1C", "name": "Page 1", "layers": [
            { "_class": "rectangle", "do_objectID": "A", "name": "Tilted", "rotation": 30, "resizingConstraint": 18,
              "frame": { "_class": "rect", "x": 0, "y": 0, "width": 10, "height": 10 },
              "points": [{ "cornerRadius": 4 }, { "cornerRadius": 4 }, { "cornerRadius": 4 }, { "cornerRadius": 4 }],
              "style": { "_class": "style", "fills": [{ "_class": "fill", "isEnabled": true, "fillType": 0,
                  "color": { "_class": "color", "red": 1, "green": 0, "blue": 0, "alpha": 1, "swatchID": "SW" } }] } },
            { "_class": "shapeGroup", "do_objectID": "B", "name": "Cutout",
              "frame": { "_class": "rect", "x": 100, "y": 0, "width": 60, "height": 40 },
              "style": { "_class": "style", "windingRule": 1 }, "layers": [rect("B1", 0.0, -1), rect("B2", 20.0, 1)] },
            { "_class": "text", "do_objectID": "C", "name": "Label", "textBehaviour": 1,
              "frame": { "_class": "rect", "x": 0, "y": 100, "width": 200, "height": 20 },
              "attributedString": { "_class": "attributedString", "string": "Hello", "attributes": [{ "_class": "stringAttribute",
                  "location": 0, "length": 5, "attributes": {
                      "MSAttributedStringFontAttribute": { "_class": "fontDescriptor", "attributes": { "name": "Avenir-Heavy", "size": 18 } },
                      "MSAttributedStringColorAttribute": { "_class": "color", "red": 0, "green": 0, "blue": 1, "alpha": 1 } } }] },
              "style": { "_class": "style", "textStyle": { "_class": "textStyle", "encodedAttributes": {
                  "MSAttributedStringFontAttribute": { "_class": "fontDescriptor", "attributes": { "name": "Avenir-Book", "size": 12 } },
                  "underlineStyle": 1, "MSAttributedStringTextTransformAttribute": 1 } } } },
            { "_class": "symbolInstance", "do_objectID": "D", "name": "Card", "symbolID": "S1",
              "frame": { "_class": "rect", "x": 0, "y": 200, "width": 100, "height": 50 },
              "overrideValues": [
                  { "_class": "overrideValue", "overrideName": "N1/T1_stringValue", "value": "Deep" },
                  { "_class": "overrideValue", "overrideName": "N1_symbolID", "value": "" },
                  { "_class": "overrideValue", "overrideName": "T2_color", "value": "ignored" }] }
        ]});
        let document = json!({ "_class": "document", "pages": [{ "_ref": "pages/p1" }],
            "sharedSwatches": { "objects": [{ "_class": "swatch", "do_objectID": "SW", "name": "Red",
                "value": { "_class": "color", "red": 1, "green": 0, "blue": 0, "alpha": 1 } }] } });
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, v) in [("document.json", &document), ("pages/p1.json", &page)] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(&serde_json::to_vec(v).unwrap()).unwrap();
        }
        let doc = import(&zip.finish().unwrap().into_inner()).unwrap();
        let layers = &doc.pages[0].layers;

        let tilted = &layers[0];
        assert_eq!(tilted.rotation, 330.0, "Sketch rotates counter-clockwise");
        assert_eq!(tilted.resizing, Resizing { left: true, right: true, top: true, bottom: true, ..Default::default() });
        assert_eq!(tilted.kind, LayerKind::Rectangle(Rectangle { corner_radius: 4.0, corner_radii: None }));
        assert_eq!(tilted.style.fills[0].swatch, doc.swatches.first().map(|s| s.id));

        let cutout = &layers[1];
        let LayerKind::Path(p) = &cutout.kind else { panic!("shape groups become paths") };
        assert_eq!(p.fill_rule, FillRule::EvenOdd);
        // 40×40 minus the overlapping right half of the second square.
        assert!((cutout.local_path().unwrap().area().abs() - 800.0).abs() < 1.0);
        assert_eq!(cutout.frame.width, 20.0);

        let LayerKind::Text(t) = &layers[2].kind else { panic!() };
        assert_eq!((t.style.font_family.as_str(), t.style.font_weight, t.style.font_size), ("Avenir", 800, 18.0), "the run wins");
        assert!(t.style.underline && t.style.transform == TextTransform::Uppercase, "the text style fills in the rest");
        assert_eq!(t.sizing, TextSizing::FixedWidth);
        assert_eq!(layers[2].style.fills[0].color, Color::rgba(0.0, 0.0, 1.0, 1.0));

        let LayerKind::Symbol(inst) = &layers[3].kind else { panic!() };
        assert_eq!(inst.overrides.len(), 2, "unknown properties are skipped");
        assert_eq!(inst.overrides[0].path, vec![LayerId(uuid_of("N1")), LayerId(uuid_of("T1"))]);
        assert_eq!(inst.overrides[1].value, OverrideValue::Symbol(None));
    }

    #[test]
    fn rotated_ring_keeps_its_shape() {
        // A boolean result has two subpaths; it goes out as a shape group and comes back a path.
        let doc = crate::sample::sample_document();
        let mut ring = None;
        doc.pages[0].walk(|l| {
            if l.name == "Ring" {
                ring = Some(l.clone());
            }
        });
        let ring = ring.unwrap();
        let back = import(&export(&doc).unwrap()).unwrap();
        let back_ring = back.pages[0].find(ring.id).unwrap();
        let area = |l: &Layer| (l.local_transform() * l.local_path().unwrap()).area().abs();
        assert!((area(&ring) - area(back_ring)).abs() / area(&ring) < 0.02);
    }
}
