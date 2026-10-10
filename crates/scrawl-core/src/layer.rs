use kurbo::{Affine, BezPath, Ellipse, Point, Rect, RoundedRect, RoundedRectRadii, Shape, Vec2};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::constraints::Resizing;
use crate::path::{CurvePoint, PathShape};
use crate::text::TextLayer;
use crate::style::BlendMode;
use crate::{Color, Frame, Style, geometry::union_rects};

/// Tolerance used when flattening analytic shapes to Béziers.
const PATH_TOLERANCE: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub Uuid);

impl LayerId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// A node in a page's layer tree.
///
/// A layer's [`Frame`] lives in its parent's coordinate space. Rotation and flips
/// are applied about the frame's center.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    pub frame: Frame,
    /// Degrees, clockwise on screen.
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub blend_mode: BlendMode,
    /// Clips the layers above it within the same parent ("Use as Mask").
    #[serde(default)]
    pub is_mask: bool,
    /// How a mask clips: by its outline, its opacity, or its brightness.
    #[serde(default, skip_serializing_if = "MaskMode::is_outline")]
    pub mask_mode: MaskMode,
    /// Escapes the masks below it in the same parent ("Ignore Underlying Mask"), along with the
    /// layers above it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub breaks_mask: bool,
    /// How the layer responds when its parent is resized.
    #[serde(default, skip_serializing_if = "Resizing::is_default")]
    pub resizing: Resizing,
    #[serde(default)]
    pub style: Style,
    /// Files written when the layer is exported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<crate::exportable::ExportFormat>,
    /// The shared layer or text style this layer follows. Its `style` is a copy, which may differ
    /// from the shared one until updated or reset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_style: Option<crate::components::StyleId>,
    /// Prototyping link followed when the layer is clicked in the preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<crate::prototype::FlowLink>,
    /// Stays put when the artboard scrolls in the preview (direct children of artboards).
    #[serde(default, skip_serializing_if = "is_false")]
    pub fixed_to_viewport: bool,
    /// How this layer sizes itself inside a stack.
    #[serde(default, skip_serializing_if = "crate::layout::StackItem::is_default")]
    pub stack_item: crate::layout::StackItem,
    /// How this layer combines with the ones below it inside a shape group.
    #[serde(default, skip_serializing_if = "is_union")]
    pub boolean_op: crate::ops::BooleanOp,
    #[serde(flatten)]
    pub kind: LayerKind,
}

fn default_true() -> bool {
    true
}

fn is_union(op: &crate::ops::BooleanOp) -> bool {
    *op == crate::ops::BooleanOp::Union
}

fn is_false(b: &bool) -> bool {
    !b
}

fn default_opacity() -> f64 {
    1.0
}

/// How a mask layer clips what's above it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaskMode {
    /// By the mask's outline.
    #[default]
    Outline,
    /// By the mask's rendered opacity (so a fading fill fades what it masks).
    Alpha,
    /// By the mask's brightness: white shows, black hides.
    Luminance,
}

impl MaskMode {
    pub const ALL: [Self; 3] = [Self::Outline, Self::Alpha, Self::Luminance];

    pub fn label(self) -> &'static str {
        match self {
            Self::Outline => "Outline",
            Self::Alpha => "Alpha",
            Self::Luminance => "Luminance",
        }
    }

    fn is_outline(&self) -> bool {
        *self == Self::Outline
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LayerKind {
    Artboard(Artboard),
    Group(Group),
    Rectangle(Rectangle),
    Oval,
    Polygon(Polygon),
    Star(Star),
    Path(PathShape),
    Text(TextLayer),
    /// An instance of a symbol master.
    Symbol(crate::symbols::SymbolInstance),
    /// A bitmap, stretched to the frame.
    Image(ImageLayer),
    /// An invisible region to export.
    Slice,
    /// An invisible area that follows its link when clicked in the preview.
    Hotspot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageLayer {
    /// The asset id of the encoded image.
    pub image: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artboard {
    #[serde(default)]
    pub children: Vec<Layer>,
    /// `None` renders the default white background.
    #[serde(default)]
    pub background: Option<Color>,
    #[serde(default = "default_true")]
    pub clip_content: bool,
    /// Apply children's resizing constraints when the artboard is resized. Off, children keep
    /// their position on the page.
    #[serde(default)]
    pub resizes_content: bool,
    /// Ruler guides, in artboard space.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<crate::grid::Guide>,
    #[serde(default)]
    pub grid: crate::grid::SquareGrid,
    #[serde(default)]
    pub layout: crate::grid::LayoutGrid,
    /// Makes this artboard a symbol master: its children are what instances show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<crate::symbols::SymbolId>,
    /// A prototype's start point.
    #[serde(default, skip_serializing_if = "is_false")]
    pub flow_home: bool,
}

impl Artboard {
    pub fn new() -> Self {
        Self {
            children: vec![],
            background: None,
            clip_content: true,
            resizes_content: false,
            guides: vec![],
            grid: Default::default(),
            layout: Default::default(),
            symbol_id: None,
            flow_home: false,
        }
    }
}

impl Default for Artboard {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    #[serde(default)]
    pub children: Vec<Layer>,
    /// Lays the children out in a row or column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<crate::layout::Stack>,
    /// A shape group: the children's outlines combine by their boolean operations into one
    /// shape, drawn with the group's style.
    #[serde(default, skip_serializing_if = "is_false")]
    pub shape: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rectangle {
    #[serde(default)]
    pub corner_radius: f64,
    /// Per-corner radii (top-left, top-right, bottom-right, bottom-left), overriding
    /// `corner_radius` when set.
    #[serde(default)]
    pub corner_radii: Option<[f64; 4]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub sides: u32,
}

impl Default for Polygon {
    fn default() -> Self {
        Self { sides: 5 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Star {
    pub points: u32,
    /// Inner radius as a fraction of the outer radius.
    pub inner_ratio: f64,
}

impl Default for Star {
    fn default() -> Self {
        Self { points: 5, inner_ratio: 0.5 }
    }
}

/// A single open segment between two corner points.
fn is_line(p: &PathShape) -> bool {
    matches!(&p.subpaths[..], [sp] if !sp.closed && sp.points.len() == 2 && sp.points.iter().all(|q| q.curve_from.is_none() && q.curve_to.is_none()))
}

impl LayerKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Artboard(a) if a.symbol_id.is_some() => "Symbol Master",
            Self::Artboard(_) => "Artboard",
            Self::Group(g) if g.shape => "Shape Group",
            Self::Group(_) => "Group",
            Self::Rectangle(_) => "Rectangle",
            Self::Oval => "Oval",
            Self::Polygon(p) if p.sides == 3 => "Triangle",
            Self::Polygon(_) => "Polygon",
            Self::Star(_) => "Star",
            Self::Text(_) => "Text",
            Self::Path(p) if is_line(p) => "Line",
            Self::Path(_) => "Path",
            Self::Symbol(_) => "Symbol",
            Self::Image(_) => "Image",
            Self::Slice => "Slice",
            Self::Hotspot => "Hotspot",
        }
    }
}

impl Layer {
    pub fn new(name: impl Into<String>, frame: Frame, kind: LayerKind) -> Self {
        Self {
            id: LayerId::new(),
            name: name.into(),
            visible: true,
            locked: false,
            frame,
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            is_mask: false,
            mask_mode: MaskMode::Outline,
            breaks_mask: false,
            resizing: Resizing::default(),
            style: Style::default(),
            exports: vec![],
            shared_style: None,
            flow: None,
            fixed_to_viewport: false,
            boolean_op: crate::ops::BooleanOp::Union,
            stack_item: Default::default(),
            kind,
        }
    }

    pub fn artboard(name: impl Into<String>, frame: Frame) -> Self {
        Self::new(
            name,
            frame,
            LayerKind::Artboard(Artboard::new()),
        )
    }

    pub fn rectangle(frame: Frame) -> Self {
        let mut l = Self::new("Rectangle", frame, LayerKind::Rectangle(Rectangle::default()));
        l.style = Style::default_shape();
        l
    }

    pub fn oval(frame: Frame) -> Self {
        let mut l = Self::new("Oval", frame, LayerKind::Oval);
        l.style = Style::default_shape();
        l
    }

    /// A straight line between two points in parent coordinates.
    pub fn line(a: Point, b: Point) -> Self {
        let mut l = Self::path(&PathShape::single(vec![CurvePoint::corner(a), CurvePoint::corner(b)], false));
        l.name = "Line".into();
        l
    }

    /// A path layer from a shape in parent coordinates. Closed paths get the default shape
    /// style, open ones the line style.
    pub fn path(shape: &PathShape) -> Self {
        let (bounds, norm) = shape.fit();
        let closed = norm.is_closed();
        let mut l = Self::new("Path", Frame::from_rect(bounds), LayerKind::Path(norm));
        l.style = if closed { Style::default_shape() } else { Style::default_line() };
        l
    }

    pub fn polygon(frame: Frame, sides: u32) -> Self {
        let mut l = Self::new(if sides == 3 { "Triangle" } else { "Polygon" }, frame, LayerKind::Polygon(Polygon { sides }));
        l.style = Style::default_shape();
        l
    }

    /// A text layer at `origin` with default styling: black text, no border. Its size is set by
    /// the text engine.
    pub fn text(origin: Point, text: impl Into<String>) -> Self {
        let text = text.into();
        let name = text.lines().next().unwrap_or("").chars().take(40).collect::<String>();
        let mut l = Self::new(
            if name.trim().is_empty() { "Text".to_string() } else { name },
            Frame::new(origin.x, origin.y, 1.0, 1.0),
            LayerKind::Text(TextLayer::new(text)),
        );
        l.style.fills = vec![crate::Fill::solid(Color::BLACK)];
        l
    }

    /// A slice: a region exported at its formats, by default 1× PNG.
    pub fn slice(frame: Frame) -> Self {
        let mut l = Self::new("Slice", frame, LayerKind::Slice);
        l.exports = vec![crate::exportable::ExportFormat::default()];
        l
    }

    /// A hotspot: an invisible link area, linked by the caller.
    pub fn hotspot(frame: Frame) -> Self {
        Self::new("Hotspot", frame, LayerKind::Hotspot)
    }

    /// An image layer showing asset `image` at `frame`.
    pub fn image(image: impl Into<String>, frame: Frame) -> Self {
        Self::new("Image", frame, LayerKind::Image(ImageLayer { image: image.into() }))
    }

    pub fn is_text(&self) -> bool {
        matches!(self.kind, LayerKind::Text(_))
    }

    pub fn star(frame: Frame) -> Self {
        let mut l = Self::new("Star", frame, LayerKind::Star(Star::default()));
        l.style = Style::default_shape();
        l
    }

    pub fn group(children: Vec<Layer>, frame: Frame) -> Self {
        Self::new("Group", frame, LayerKind::Group(Group { children, stack: None, shape: false }))
    }

    pub fn children(&self) -> Option<&Vec<Layer>> {
        match &self.kind {
            LayerKind::Artboard(a) => Some(&a.children),
            LayerKind::Group(g) => Some(&g.children),
            _ => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Layer>> {
        match &mut self.kind {
            LayerKind::Artboard(a) => Some(&mut a.children),
            LayerKind::Group(g) => Some(&mut g.children),
            _ => None,
        }
    }

    pub fn is_artboard(&self) -> bool {
        matches!(self.kind, LayerKind::Artboard(_))
    }

    pub fn is_group(&self) -> bool {
        matches!(self.kind, LayerKind::Group(_))
    }

    /// A live boolean group.
    pub fn is_shape_group(&self) -> bool {
        matches!(&self.kind, LayerKind::Group(g) if g.shape)
    }

    /// A shape group's combined outline in its local space. Children combine bottom to top, each
    /// by its own operation; hidden ones are left out. `None` for other layers or if nothing's
    /// left.
    pub fn combined_path(&self) -> Option<BezPath> {
        let LayerKind::Group(g) = &self.kind else { return None };
        if !g.shape {
            return None;
        }
        let mut acc: Option<BezPath> = None;
        for c in g.children.iter().filter(|c| c.visible) {
            let Some(p) = c.outline() else { continue };
            let p = c.local_transform() * p;
            acc = Some(match acc {
                None => p,
                Some(a) => crate::ops::path_boolean(&a, &p, c.boolean_op.sweep_op()).unwrap_or_else(|_| {
                    let mut both = a;
                    both.extend(p.elements().iter().copied());
                    both
                }),
            });
        }
        acc
    }

    /// The filled outline used when this layer is part of a shape group: its geometry, a nested
    /// shape group's combined path, or a plain group's children together.
    fn outline(&self) -> Option<BezPath> {
        match &self.kind {
            LayerKind::Group(g) if g.shape => self.combined_path(),
            LayerKind::Group(g) => {
                let mut all = BezPath::new();
                for c in g.children.iter().filter(|c| c.visible) {
                    if let Some(p) = c.outline() {
                        all.extend((c.local_transform() * p).elements().iter().copied());
                    }
                }
                Some(all)
            }
            _ if self.is_closed() => self.local_path(),
            _ => None,
        }
    }

    /// Replaces a shape group with a single path of its combined outline, keeping its style.
    /// Other shapes become paths too. Returns false for what can't (containers, text).
    pub fn flatten(&mut self) -> bool {
        if self.is_shape_group() {
            let Some(path) = self.combined_path() else { return false };
            let mut shape = PathShape::from_bez_path(&path);
            shape.fill_rule = crate::FillRule::EvenOdd;
            self.set_local_path(&shape);
            return true;
        }
        self.convert_to_path()
    }

    pub fn is_symbol_instance(&self) -> bool {
        matches!(self.kind, LayerKind::Symbol(_))
    }

    /// The symbol this artboard is the master of.
    pub fn master_of(&self) -> Option<crate::symbols::SymbolId> {
        match &self.kind {
            LayerKind::Artboard(a) => a.symbol_id,
            _ => None,
        }
    }

    /// Maps this layer's local space into its parent's space.
    pub fn local_transform(&self) -> Affine {
        let f = self.frame;
        let c = Vec2::new(f.width / 2.0, f.height / 2.0);
        let sx = if self.flip_horizontal { -1.0 } else { 1.0 };
        let sy = if self.flip_vertical { -1.0 } else { 1.0 };
        Affine::translate(f.origin().to_vec2() + c)
            * Affine::rotate(self.rotation.to_radians())
            * Affine::scale_non_uniform(sx, sy)
            * Affine::translate(-c)
    }

    /// The layer's own geometry in local space. `None` for groups, which have none.
    pub fn local_path(&self) -> Option<BezPath> {
        let r = self.frame.local_rect();
        match &self.kind {
            LayerKind::Artboard(_) => Some(r.to_path(PATH_TOLERANCE)),
            LayerKind::Group(g) if g.shape => self.combined_path(),
            LayerKind::Group(_) => None,
            LayerKind::Rectangle(rect) => {
                let max_r = r.width().min(r.height()) / 2.0;
                let c = |v: f64| v.clamp(0.0, max_r);
                let [tl, tr, br, bl] = rect.corner_radii.unwrap_or([rect.corner_radius; 4]);
                let radii = RoundedRectRadii::new(c(tl), c(tr), c(br), c(bl));
                Some(RoundedRect::from_rect(r, radii).to_path(PATH_TOLERANCE))
            }
            LayerKind::Oval => {
                let mut path = Ellipse::from_rect(r).to_path(PATH_TOLERANCE);
                // kurbo leaves ellipses unclosed, which would show as a seam in strokes.
                path.close_path();
                Some(path)
            }
            LayerKind::Polygon(p) => Some(fitted_polygon(r, &regular_vertices(p.sides.max(3), &[1.0]))),
            LayerKind::Star(st) => {
                Some(fitted_polygon(r, &regular_vertices(st.points.max(3) * 2, &[1.0, st.inner_ratio.clamp(0.01, 1.0)])))
            }
            LayerKind::Path(p) => Some(p.to_bez_path(r.width(), r.height())),
            // A text layer's or symbol instance's geometry, for selection and bounds, is its frame.
            LayerKind::Text(_) | LayerKind::Symbol(_) | LayerKind::Image(_) | LayerKind::Slice | LayerKind::Hotspot => {
                Some(r.to_path(PATH_TOLERANCE))
            }
        }
    }

    pub fn fill_rule(&self) -> crate::FillRule {
        match &self.kind {
            LayerKind::Path(p) => p.fill_rule,
            // Boolean results are exact with even-odd.
            LayerKind::Group(g) if g.shape => crate::FillRule::EvenOdd,
            _ => crate::FillRule::NonZero,
        }
    }

    /// Whether the geometry encloses an area that can be filled.
    pub fn is_closed(&self) -> bool {
        match &self.kind {
            LayerKind::Path(p) => p.is_closed(),
            LayerKind::Group(g) if g.shape => true,
            // Slices are only picked by their outline, so they don't block clicks on what's under.
            LayerKind::Group(_) | LayerKind::Slice => false,
            _ => true,
        }
    }

    /// Bounds of the rendered geometry (excluding borders) in the space `to_space` maps local
    /// coordinates into.
    pub fn bounds_in(&self, to_space: Affine) -> Option<Rect> {
        let t = to_space * self.local_transform();
        match self.children() {
            Some(children) if self.is_group() => union_rects(children.iter().filter_map(|c| c.bounds_in(t))),
            _ => self.local_path().map(|p| (t * p).bounding_box()),
        }
    }

    /// Moves the frame so the layer occupies `r`, given in the layer's *current* local space,
    /// while keeping its rotation and flips. Group children follow their resizing constraints
    /// (scaling by default); artboard children keep their place on the page unless the artboard
    /// adjusts content on resize.
    pub fn reframe(&mut self, r: Rect) {
        let r = r.abs();
        let center_in_parent = self.local_transform() * r.center();
        let size = r.size();
        let frame = Frame::new(
            center_in_parent.x - size.width / 2.0,
            center_in_parent.y - size.height / 2.0,
            size.width,
            size.height,
        );
        self.set_frame(frame, r.origin().to_vec2());
    }

    /// Replaces the frame and resizes the contents. `shift` is the new top-left in the old local
    /// space, for artboards whose children stay put.
    fn set_frame(&mut self, frame: Frame, shift: Vec2) {
        let old = self.frame;
        self.frame = frame;
        let (old, new) = ((old.width, old.height), (frame.width, frame.height));
        match &mut self.kind {
            LayerKind::Group(g) => g.children.iter_mut().for_each(|c| c.fit_to_parent(old, new)),
            LayerKind::Artboard(a) if a.resizes_content => a.children.iter_mut().for_each(|c| c.fit_to_parent(old, new)),
            LayerKind::Artboard(a) => a.children.iter_mut().for_each(|c| c.frame.translate(-shift)),
            _ => {}
        }
    }

    /// Applies resizing constraints after the parent's size changed from `old` to `new`.
    pub(crate) fn fit_to_parent(&mut self, old: (f64, f64), new: (f64, f64)) {
        let rs = self.resizing;
        // A rotated layer is pinned and stretched by its rotated bounds, as it looks.
        if self.rotation.rem_euclid(360.0) != 0.0
            && let Some(b) = self.local_path().map(|p| (self.local_transform() * p).bounding_box())
        {
            let (x, w) = Resizing::axis(b.x0, b.width(), old.0, new.0, rs.left, rs.right, rs.fix_width);
            let (y, h) = Resizing::axis(b.y0, b.height(), old.1, new.1, rs.top, rs.bottom, rs.fix_height);
            let kx = if b.width() > 1e-9 { w / b.width() } else { 1.0 };
            let ky = if b.height() > 1e-9 { h / b.height() } else { 1.0 };
            // Carry the on-screen scales onto the layer's own (rotated) axes.
            let (sin, cos) = self.rotation.to_radians().sin_cos();
            let (sw, sh) = ((kx * cos).hypot(ky * sin), (kx * sin).hypot(ky * cos));
            let (fw, fh) = (self.frame.width * sw, self.frame.height * sh);
            let center = kurbo::Point::new(x + w / 2.0, y + h / 2.0);
            self.set_frame(Frame::new(center.x - fw / 2.0, center.y - fh / 2.0, fw, fh), Vec2::ZERO);
            return;
        }
        let f = self.frame;
        let (x, w) = Resizing::axis(f.x, f.width, old.0, new.0, rs.left, rs.right, rs.fix_width);
        let (y, h) = Resizing::axis(f.y, f.height, old.1, new.1, rs.top, rs.bottom, rs.fix_height);
        if rs.is_default()
            && let LayerKind::Rectangle(rect) = &mut self.kind
        {
            let sx = if old.0 > 0.0 { new.0 / old.0 } else { 1.0 };
            let sy = if old.1 > 0.0 { new.1 / old.1 } else { 1.0 };
            rect.corner_radius *= sx.min(sy);
        }
        self.set_frame(Frame::new(x, y, w, h), Vec2::ZERO);
    }

    /// Shrinks or grows a group's frame to tightly enclose its children, without moving them on
    /// screen. No-op for other layer types, and for stacks, which size themselves.
    pub fn fit_to_children(&mut self) {
        let transform = self.local_transform();
        let LayerKind::Group(g) = &mut self.kind else { return };
        if g.stack.is_some() {
            return;
        }
        let Some(r) = union_rects(g.children.iter().filter_map(|c| c.bounds_in(Affine::IDENTITY))) else {
            return;
        };
        let center_in_parent = transform * r.center();
        self.frame = Frame::new(
            center_in_parent.x - r.width() / 2.0,
            center_in_parent.y - r.height() / 2.0,
            r.width(),
            r.height(),
        );
        for c in &mut g.children {
            c.frame.translate(-r.origin().to_vec2());
        }
    }

    /// The geometry as an editable path in absolute local coordinates. `None` for containers.
    pub fn editable_path(&self) -> Option<PathShape> {
        match &self.kind {
            LayerKind::Path(p) => Some(p.scaled(self.frame.width, self.frame.height)),
            LayerKind::Artboard(_)
            | LayerKind::Group(_)
            | LayerKind::Text(_)
            | LayerKind::Symbol(_)
            | LayerKind::Image(_)
            | LayerKind::Slice
            | LayerKind::Hotspot => None,
            _ => Some(PathShape::from_bez_path(&self.local_path()?)),
        }
    }

    /// Replaces the geometry with `shape`, given in absolute local coordinates. The frame is
    /// refit to the path's tight bounds without moving anything on screen.
    pub fn set_local_path(&mut self, shape: &PathShape) {
        let (bounds, norm) = shape.fit();
        self.kind = LayerKind::Path(PathShape::default());
        self.reframe(bounds);
        self.kind = LayerKind::Path(norm);
    }

    /// Converts a shape (rectangle, oval, polygon…) into an equivalent editable path in place.
    /// Returns false for containers.
    pub fn convert_to_path(&mut self) -> bool {
        if matches!(self.kind, LayerKind::Path(_)) {
            return true;
        }
        let Some(shape) = self.editable_path() else { return false };
        self.set_local_path(&shape);
        true
    }

    /// Deep copy with fresh ids for this layer and all descendants.
    pub fn duplicate(&self) -> Layer {
        let mut l = self.clone();
        l.regenerate_ids();
        l
    }

    fn regenerate_ids(&mut self) {
        self.id = LayerId::new();
        // A copied master defines a new symbol.
        if let LayerKind::Artboard(a) = &mut self.kind
            && a.symbol_id.is_some()
        {
            a.symbol_id = Some(crate::symbols::SymbolId::new());
        }
        if let Some(children) = self.children_mut() {
            children.iter_mut().for_each(Layer::regenerate_ids);
        }
    }

    /// Depth-first, pre-order traversal of this layer and its descendants, mutably.
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Layer)) {
        f(self);
        if let Some(children) = self.children_mut() {
            children.iter_mut().for_each(|c| c.walk_mut(f));
        }
    }

    /// Depth-first, pre-order traversal of this layer and its descendants.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Layer)) {
        f(self);
        if let Some(children) = self.children() {
            children.iter().for_each(|c| c.walk(f));
        }
    }
}

/// Vertices of a regular polygon on the unit circle, starting at the top, cycling through
/// `radii` (for stars).
fn regular_vertices(n: u32, radii: &[f64]) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::TAU / n as f64;
            let r = radii[i as usize % radii.len()];
            Point::new(r * a.cos(), r * a.sin())
        })
        .collect()
}

/// Closes `pts` into a path stretched to fill `r`, like shape layers.
fn fitted_polygon(r: Rect, pts: &[Point]) -> BezPath {
    let b = pts.iter().fold(Rect::from_points(pts[0], pts[0]), |b, &p| b.union_pt(p));
    let map = |p: Point| {
        Point::new(r.x0 + (p.x - b.x0) / b.width() * r.width(), r.y0 + (p.y - b.y0) / b.height() * r.height())
    };
    let mut path = BezPath::new();
    path.move_to(map(pts[0]));
    pts[1..].iter().for_each(|&p| path.line_to(map(p)));
    path.close_path();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Point, b: Point) -> bool {
        (a - b).hypot() < 1e-9
    }

    #[test]
    fn reframe_keeps_opposite_edge_fixed_when_rotated() {
        let mut l = Layer::rectangle(Frame::new(10.0, 20.0, 100.0, 50.0));
        l.rotation = 30.0;
        let anchor_before = l.local_transform() * Point::new(0.0, 0.0);
        // Drag the right edge out by 40 in local space.
        l.reframe(Rect::new(0.0, 0.0, 140.0, 50.0));
        let anchor_after = l.local_transform() * Point::new(0.0, 0.0);
        assert!(approx(anchor_before, anchor_after));
        assert!((l.frame.width - 140.0).abs() < 1e-9);
    }

    #[test]
    fn fit_to_children_preserves_world_positions() {
        let child = Layer::rectangle(Frame::new(30.0, 40.0, 10.0, 10.0));
        let mut g = Layer::group(vec![child], Frame::new(100.0, 100.0, 200.0, 200.0));
        g.rotation = 45.0;
        let before = g.local_transform() * g.children().unwrap()[0].local_transform() * Point::ORIGIN;
        g.fit_to_children();
        let after = g.local_transform() * g.children().unwrap()[0].local_transform() * Point::ORIGIN;
        assert!(approx(before, after));
        assert!((g.frame.width - 10.0).abs() < 1e-9);
    }

    #[test]
    fn line_normalizes_points() {
        let l = Layer::line(Point::new(10.0, 10.0), Point::new(110.0, 10.0));
        assert_eq!(l.frame, Frame::new(10.0, 10.0, 100.0, 0.0));
        let LayerKind::Path(p) = &l.kind else { panic!() };
        assert_eq!(p.subpaths[0].points[1].point, Point::new(1.0, 0.0));
        assert_eq!(l.kind.type_name(), "Line");
    }

    #[test]
    fn convert_to_path_keeps_geometry() {
        let mut l = Layer::oval(Frame::new(10.0, 20.0, 100.0, 50.0));
        l.rotation = 30.0;
        let before = l.bounds_in(Affine::IDENTITY).unwrap();
        assert!(l.convert_to_path());
        assert_eq!(l.kind.type_name(), "Path");
        let after = l.bounds_in(Affine::IDENTITY).unwrap();
        assert!((before.center() - after.center()).hypot() < 1e-6);
        assert!((before.area() - after.area()).abs() < 1e-3);
    }

    #[test]
    fn polygons_fill_their_frame() {
        let l = Layer::polygon(Frame::new(0.0, 0.0, 100.0, 80.0), 3);
        assert_eq!(l.bounds_in(Affine::IDENTITY).unwrap(), Rect::new(0.0, 0.0, 100.0, 80.0));
        let s = Layer::star(Frame::new(0.0, 0.0, 100.0, 100.0));
        let b = s.bounds_in(Affine::IDENTITY).unwrap();
        assert!((b.width() - 100.0).abs() < 1e-9 && (b.height() - 100.0).abs() < 1e-9);
    }
}
