use kurbo::Point;
use serde::{Deserialize, Serialize};

use crate::Color;
use crate::components::SwatchId;

/// Visual styling shared by every layer type.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// Painted bottom to top.
    pub fills: Vec<Fill>,
    /// Painted bottom to top, above all fills.
    pub borders: Vec<Border>,
    /// Drop shadows, painted beneath the layer.
    pub shadows: Vec<Shadow>,
    /// Painted inside the shape, above fills and below borders.
    pub inner_shadows: Vec<Shadow>,
    /// Border options shared by all borders.
    pub border_options: BorderOptions,
    /// A Gaussian blur of the layer itself, or of what's behind it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<Blur>,
}

/// What a blur applies to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlurKind {
    /// The layer itself.
    #[default]
    Layer,
    /// What's behind the layer, inside its shape (frosted glass).
    Background,
    /// The layer smeared along a direction.
    Motion,
    /// The layer smeared towards a center point.
    Zoom,
}

impl BlurKind {
    pub const ALL: [Self; 4] = [Self::Layer, Self::Motion, Self::Zoom, Self::Background];

    pub fn label(self) -> &'static str {
        match self {
            Self::Layer => "Gaussian",
            Self::Background => "Background",
            Self::Motion => "Motion",
            Self::Zoom => "Zoom",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Blur {
    pub enabled: bool,
    pub kind: BlurKind,
    /// In points: about twice the Gaussian's standard deviation; for zoom blurs,
    /// the strength (100 smears all the way to the center).
    pub radius: f64,
    /// Motion blur direction, in degrees.
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub angle: f64,
    /// Zoom blur center, normalized to the frame.
    #[serde(default = "center", skip_serializing_if = "is_center")]
    pub center: Point,
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

fn center() -> Point {
    Point::new(0.5, 0.5)
}

fn is_center(p: &Point) -> bool {
    *p == center()
}

impl Default for Blur {
    fn default() -> Self {
        Self { enabled: true, kind: BlurKind::Layer, radius: 10.0, angle: 0.0, center: center() }
    }
}

impl Blur {
    /// The blur in effect, if any.
    pub fn active(blur: &Option<Blur>) -> Option<Blur> {
        blur.filter(|b| b.enabled && b.radius > 0.0)
    }
}

impl Style {
    /// The style a freshly drawn shape gets.
    pub fn default_shape() -> Self {
        Self {
            fills: vec![Fill::solid(Color::DEFAULT_FILL)],
            borders: vec![Border::solid(Color::DEFAULT_BORDER, 1.0)],
            ..Default::default()
        }
    }

    /// The default style for lines and open paths, which have no fill.
    pub fn default_line() -> Self {
        Self { borders: vec![Border::solid(Color::DEFAULT_BORDER, 1.0)], ..Default::default() }
    }

    /// Visits every solid color that can follow a color variable: fills, borders and shadows.
    pub fn colors_mut(&mut self, mut f: impl FnMut(&mut Color, &mut Option<SwatchId>)) {
        self.fills.iter_mut().for_each(|x| f(&mut x.color, &mut x.swatch));
        self.borders.iter_mut().for_each(|x| f(&mut x.color, &mut x.swatch));
        // Gradient stops can follow variables too.
        let gradients = self.fills.iter_mut().map(|x| &mut x.gradient).chain(self.borders.iter_mut().map(|x| &mut x.gradient));
        gradients.flat_map(|g| g.stops.iter_mut()).for_each(|s| f(&mut s.color, &mut s.swatch));
        self.shadows.iter_mut().chain(&mut self.inner_shadows).for_each(|x| f(&mut x.color, &mut x.swatch));
    }

    /// Largest distance any enabled border paints outside the geometry.
    pub fn border_outset(&self) -> f64 {
        let markers = if self.border_options.start_marker != Marker::None || self.border_options.end_marker != Marker::None {
            3.0
        } else {
            1.0
        };
        self.borders
            .iter()
            .filter(|b| b.enabled)
            .map(|b| match b.position {
                BorderPosition::Center => b.thickness / 2.0 * markers,
                BorderPosition::Inside => 0.0,
                BorderPosition::Outside => b.thickness,
            })
            .fold(0.0, f64::max)
    }

    /// Largest distance anything (borders, shadows) paints outside the geometry.
    pub fn paint_outset(&self) -> f64 {
        let shadows = self
            .shadows
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.offset_x.abs().max(s.offset_y.abs()) + s.blur * 1.5 + s.spread.max(0.0))
            .fold(0.0, f64::max);
        // A layer blur spreads it out by about three standard deviations.
        let blur = Blur::active(&self.blur).filter(|b| matches!(b.kind, BlurKind::Layer | BlurKind::Motion)).map_or(0.0, |b| b.radius * 1.5);
        self.border_outset().max(shadows) + blur
    }
}

/// What a fill or border paints with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillType {
    #[default]
    Color,
    Gradient,
    /// An image asset (fills only).
    Image,
    /// Grain over what's beneath (fills only).
    Noise,
}

/// Noise fill kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoiseKind {
    /// Grey grain, lightening and darkening.
    #[default]
    Original,
    Black,
    White,
    /// Grain in the fill's color.
    Color,
}

impl NoiseKind {
    pub const ALL: [Self; 4] = [Self::Original, Self::Black, Self::White, Self::Color];

    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Black => "Black",
            Self::White => "White",
            Self::Color => "Color",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Noise {
    pub kind: NoiseKind,
    /// 0..1: how strongly the grain shows.
    pub intensity: f64,
}

impl Default for Noise {
    fn default() -> Self {
        Self { kind: NoiseKind::Original, intensity: 0.2 }
    }
}

/// How an image fill covers the layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageMode {
    /// Scale to cover the whole frame, cropping the overflow.
    #[default]
    Fill,
    /// Scale to fit inside the frame.
    Fit,
    /// Stretch to the frame, ignoring the aspect ratio.
    Stretch,
    /// Repeat at natural size, scaled by `image_scale`.
    Tile,
}

impl ImageMode {
    pub const ALL: [Self; 4] = [Self::Fill, Self::Fit, Self::Stretch, Self::Tile];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fill => "Fill",
            Self::Fit => "Fit",
            Self::Stretch => "Stretch",
            Self::Tile => "Tile",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub enabled: bool,
    #[serde(default)]
    pub fill_type: FillType,
    pub color: Color,
    #[serde(default)]
    pub gradient: Gradient,
    #[serde(default)]
    pub blend_mode: BlendMode,
    /// The color variable `color` follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch: Option<SwatchId>,
    /// The image asset for image fills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub image_mode: ImageMode,
    /// Tile scale for tiled image fills.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub image_scale: f64,
    /// For noise fills.
    #[serde(default, skip_serializing_if = "is_default")]
    pub noise: Noise,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn one() -> f64 {
    1.0
}

fn is_one(v: &f64) -> bool {
    *v == 1.0
}

impl Fill {
    pub fn solid(color: Color) -> Self {
        Self {
            enabled: true,
            fill_type: FillType::Color,
            color,
            gradient: Gradient::default(),
            blend_mode: BlendMode::Normal,
            swatch: None,
            image: None,
            image_mode: ImageMode::Fill,
            image_scale: 1.0,
            noise: Noise::default(),
        }
    }

    /// A fill showing image asset `id`.
    pub fn image(id: impl Into<String>, mode: ImageMode) -> Self {
        Self { fill_type: FillType::Image, image: Some(id.into()), image_mode: mode, ..Self::solid(Color::WHITE) }
    }

    pub fn paint(&self) -> Paint<'_> {
        match (self.fill_type, &self.image) {
            (FillType::Gradient, _) => Paint::Gradient(&self.gradient),
            (FillType::Image, Some(id)) => Paint::Image { id, mode: self.image_mode, scale: self.image_scale },
            // An image fill without an image shows nothing.
            (FillType::Image, None) => Paint::Color(Color::TRANSPARENT),
            (FillType::Color, _) => Paint::Color(self.color),
            (FillType::Noise, _) => Paint::Noise { noise: self.noise, color: self.color },
        }
    }
}

/// A borrowed view of what to paint with.
#[derive(Clone, Copy, Debug)]
pub enum Paint<'a> {
    Color(Color),
    Gradient(&'a Gradient),
    Image { id: &'a str, mode: ImageMode, scale: f64 },
    Noise { noise: Noise, color: Color },
}

/// The color space a gradient blends its stops in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Interpolation {
    /// Straight between sRGB values, as CSS does by default.
    #[default]
    Srgb,
    /// In linear light: brighter, more physical midpoints.
    LinearSrgb,
    /// Perceptually even lightness, without muddy midpoints.
    Oklab,
    /// OKLab with hue going the short way round: vivid midpoints.
    Oklch,
}

impl Interpolation {
    pub const ALL: [Self; 4] = [Self::Srgb, Self::LinearSrgb, Self::Oklab, Self::Oklch];

    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::LinearSrgb => "Linear RGB",
            Self::Oklab => "OKLab",
            Self::Oklch => "OKLCH",
        }
    }

    /// Blends `a` to `b` by `k` in this space (alpha straight, in all of them).
    pub fn mix(self, a: Color, b: Color, k: f32) -> Color {
        let lerp = |x: f32, y: f32| x + (y - x) * k;
        let alpha = lerp(a.a, b.a);
        let [r, g, bl] = match self {
            Self::Srgb => [lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b)],
            Self::LinearSrgb => {
                let (x, y) = (to_linear(a), to_linear(b));
                from_linear([lerp(x[0], y[0]), lerp(x[1], y[1]), lerp(x[2], y[2])])
            }
            Self::Oklab => {
                let (x, y) = (oklab(to_linear(a)), oklab(to_linear(b)));
                from_linear(oklab_inv([lerp(x[0], y[0]), lerp(x[1], y[1]), lerp(x[2], y[2])]))
            }
            Self::Oklch => {
                let (x, y) = (oklab(to_linear(a)), oklab(to_linear(b)));
                let (cx, cy) = (x[1].hypot(x[2]), y[1].hypot(y[2]));
                let (hx, hy) = (x[2].atan2(x[1]), y[2].atan2(y[1]));
                // Greys have no hue: take the other end's, so they don't swing through colors.
                let (hx, hy) = match (cx < 1e-4, cy < 1e-4) {
                    (true, false) => (hy, hy),
                    (false, true) => (hx, hx),
                    _ => (hx, hy),
                };
                let mut dh = hy - hx;
                if dh > std::f32::consts::PI {
                    dh -= std::f32::consts::TAU;
                } else if dh < -std::f32::consts::PI {
                    dh += std::f32::consts::TAU;
                }
                let (c, h) = (lerp(cx, cy), hx + dh * k);
                from_linear(oklab_inv([lerp(x[0], y[0]), c * h.cos(), c * h.sin()]))
            }
        };
        Color::rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), bl.clamp(0.0, 1.0), alpha)
    }
}

fn to_linear(c: Color) -> [f32; 3] {
    let f = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    [f(c.r), f(c.g), f(c.b)]
}

fn from_linear(c: [f32; 3]) -> [f32; 3] {
    let f = |v: f32| {
        let v = v.max(0.0);
        if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
    };
    [f(c[0]), f(c[1]), f(c[2])]
}

/// Linear sRGB to OKLab (Björn Ottosson's matrices, kept at their published precision).
#[allow(clippy::excessive_precision)]
fn oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

#[allow(clippy::excessive_precision)]
fn oklab_inv([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    [
        4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
        -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
        -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_,
    ]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
    Angular,
}

impl GradientKind {
    pub const ALL: [Self; 3] = [Self::Linear, Self::Radial, Self::Angular];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Radial => "Radial",
            Self::Angular => "Angular",
        }
    }
}

/// A gradient whose geometry is normalized to the layer's frame (0..1).
/// For radial gradients `from` is the center and `to` a point on the edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub kind: GradientKind,
    pub from: Point,
    pub to: Point,
    /// Sorted by position.
    pub stops: Vec<GradientStop>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub interpolation: Interpolation,
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            kind: GradientKind::Linear,
            from: Point::new(0.5, 0.0),
            to: Point::new(0.5, 1.0),
            stops: vec![GradientStop { position: 0.0, color: Color::WHITE, swatch: None }, GradientStop { position: 1.0, color: Color::BLACK, swatch: None }],
            interpolation: Interpolation::Srgb,
        }
    }
}

impl Gradient {
    /// A gradient fading `color` to transparent, a natural starting point when switching a
    /// solid fill to a gradient.
    pub fn from_color(color: Color) -> Self {
        Self {
            stops: vec![GradientStop { position: 0.0, color, swatch: None }, GradientStop { position: 1.0, color: color.with_alpha(0.0), swatch: None }],
            ..Self::default()
        }
    }

    /// Stops that reproduce this gradient when blended in sRGB, for formats that can't blend in
    /// other spaces (SVG, design files): the stops themselves for sRGB gradients, otherwise with
    /// samples in between.
    pub fn srgb_stops(&self) -> Vec<GradientStop> {
        if self.interpolation == Interpolation::Srgb || self.stops.len() < 2 {
            return self.stops.clone();
        }
        const STEPS: usize = 16;
        let mut out = vec![self.stops[0].clone()];
        for w in self.stops.windows(2) {
            for i in 1..=STEPS {
                let t = w[0].position + (w[1].position - w[0].position) * i as f64 / STEPS as f64;
                let k = i as f32 / STEPS as f32;
                out.push(GradientStop { position: t, color: self.interpolation.mix(w[0].color, w[1].color, k), swatch: None });
            }
        }
        out
    }

    /// The color at `t` in `0..=1`, blended between stops in the gradient's color space.
    pub fn color_at(&self, t: f64) -> Color {
        let Some(first) = self.stops.first() else { return Color::TRANSPARENT };
        if t <= first.position {
            return first.color;
        }
        for w in self.stops.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t <= b.position {
                let span = (b.position - a.position).max(1e-9);
                let k = ((t - a.position) / span) as f32;
                return self.interpolation.mix(a.color, b.color, k);
            }
        }
        self.stops.last().map_or(Color::TRANSPARENT, |s| s.color)
    }

    /// Inserts a stop at `t` with the interpolated color. Returns its index.
    pub fn add_stop(&mut self, t: f64) -> usize {
        let stop = GradientStop { position: t.clamp(0.0, 1.0), color: self.color_at(t), swatch: None };
        let i = self.stops.iter().position(|s| s.position > stop.position).unwrap_or(self.stops.len());
        self.stops.insert(i, stop);
        i
    }

    /// Re-sorts stops after positions change, returning where stop `i` ended up.
    pub fn sort_stops(&mut self, i: usize) -> usize {
        let tagged = self.stops.get(i).cloned();
        self.stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        tagged.and_then(|t| self.stops.iter().position(|s| *s == t)).unwrap_or(i)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f64,
    pub color: Color,
    /// The color variable `color` follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch: Option<SwatchId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub enabled: bool,
    #[serde(default)]
    pub fill_type: FillType,
    pub color: Color,
    #[serde(default)]
    pub gradient: Gradient,
    pub thickness: f64,
    #[serde(default)]
    pub position: BorderPosition,
    /// The color variable `color` follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch: Option<SwatchId>,
}

impl Border {
    pub fn solid(color: Color, thickness: f64) -> Self {
        Self {
            enabled: true,
            fill_type: FillType::Color,
            color,
            gradient: Gradient::default(),
            thickness,
            position: BorderPosition::Center,
            swatch: None,
        }
    }

    pub fn paint(&self) -> Paint<'_> {
        match self.fill_type {
            FillType::Gradient => Paint::Gradient(&self.gradient),
            // Borders can't paint images or noise.
            FillType::Color | FillType::Image | FillType::Noise => Paint::Color(self.color),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BorderPosition {
    #[default]
    Center,
    Inside,
    Outside,
}

impl BorderPosition {
    pub const ALL: [Self; 3] = [Self::Center, Self::Inside, Self::Outside];

    pub fn label(self) -> &'static str {
        match self {
            Self::Center => "Center",
            Self::Inside => "Inside",
            Self::Outside => "Outside",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BorderOptions {
    /// Alternating dash and gap lengths; empty for a solid line.
    pub dash: Vec<f64>,
    pub cap: LineCap,
    pub join: LineJoin,
    /// Arrowheads for the start and end of open paths.
    pub start_marker: Marker,
    pub end_marker: Marker,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Marker {
    #[default]
    None,
    OpenArrow,
    FilledArrow,
    Line,
    OpenCircle,
    FilledCircle,
    OpenSquare,
    FilledSquare,
}

impl Marker {
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::OpenArrow,
        Self::FilledArrow,
        Self::Line,
        Self::OpenCircle,
        Self::FilledCircle,
        Self::OpenSquare,
        Self::FilledSquare,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::OpenArrow => "Open Arrow",
            Self::FilledArrow => "Filled Arrow",
            Self::Line => "Line",
            Self::OpenCircle => "Open Circle",
            Self::FilledCircle => "Filled Circle",
            Self::OpenSquare => "Open Square",
            Self::FilledSquare => "Filled Square",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub enabled: bool,
    pub color: Color,
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur: f64,
    #[serde(default)]
    pub spread: f64,
    /// The color variable `color` follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch: Option<SwatchId>,
}

impl Shadow {
    /// The default drop shadow.
    pub fn drop() -> Self {
        Self { enabled: true, color: Color::BLACK.with_alpha(0.5), offset_x: 0.0, offset_y: 2.0, blur: 4.0, spread: 0.0, swatch: None }
    }

    /// The default inner shadow.
    pub fn inner() -> Self {
        Self { enabled: true, color: Color::BLACK.with_alpha(0.5), offset_x: 0.0, offset_y: 1.0, blur: 3.0, spread: 0.0, swatch: None }
    }
}

/// Layer and fill blend modes (the usual list, which matches CSS/PDF).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlendMode {
    #[default]
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    Lighten,
    Screen,
    ColorDodge,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [Self; 16] = [
        Self::Normal,
        Self::Darken,
        Self::Multiply,
        Self::ColorBurn,
        Self::Lighten,
        Self::Screen,
        Self::ColorDodge,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::Difference,
        Self::Exclusion,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Darken => "Darken",
            Self::Multiply => "Multiply",
            Self::ColorBurn => "Color Burn",
            Self::Lighten => "Lighten",
            Self::Screen => "Screen",
            Self::ColorDodge => "Color Dodge",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::HardLight => "Hard Light",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }
}

/// The stroke geometry for borders of the given width with the style's options.
pub fn stroke_style(width: f64, options: &BorderOptions) -> kurbo::Stroke {
    let cap = match options.cap {
        LineCap::Butt => kurbo::Cap::Butt,
        LineCap::Round => kurbo::Cap::Round,
        LineCap::Square => kurbo::Cap::Square,
    };
    let join = match options.join {
        LineJoin::Miter => kurbo::Join::Miter,
        LineJoin::Round => kurbo::Join::Round,
        LineJoin::Bevel => kurbo::Join::Bevel,
    };
    let mut stroke = kurbo::Stroke::new(width).with_caps(cap).with_join(join);
    let dashes: Vec<f64> = options.dash.iter().copied().filter(|d| *d > 0.0).collect();
    if !dashes.is_empty() {
        // A single value means equal dash and gap.
        let pattern = if dashes.len() == 1 { vec![dashes[0], dashes[0]] } else { dashes };
        stroke = stroke.with_dashes(0.0, pattern);
    }
    stroke
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_color_at_and_add_stop() {
        let mut g = Gradient::default();
        let mid = g.color_at(0.5);
        assert!((mid.r - 0.5).abs() < 1e-6 && (mid.a - 1.0).abs() < 1e-6);
        let i = g.add_stop(0.25);
        assert_eq!(i, 1);
        assert!((g.stops[1].color.r - 0.75).abs() < 1e-6);
        g.stops[1].position = 0.9;
        assert_eq!(g.sort_stops(1), 1);
        g.stops[1].position = 1.1;
        assert_eq!(g.sort_stops(1), 2);
    }

    #[test]
    fn old_files_without_new_fields_load() {
        let json = r#"{"fills":[{"enabled":true,"color":{"r":1,"g":0,"b":0,"a":1}}],
            "borders":[{"enabled":true,"color":{"r":0,"g":0,"b":0,"a":1},"thickness":2}]}"#;
        let s: Style = serde_json::from_str(json).unwrap();
        assert_eq!(s.fills[0].fill_type, FillType::Color);
        assert_eq!(s.borders[0].position, BorderPosition::Center);
        assert!(s.shadows.is_empty());
    }

    #[test]
    fn gradients_blend_in_their_color_space() {
        let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
        let blue = Color::rgba(0.0, 0.0, 1.0, 1.0);
        let mut g = Gradient {
            stops: vec![GradientStop { position: 0.0, color: red, swatch: None }, GradientStop { position: 1.0, color: blue, swatch: None }],
            ..Gradient::default()
        };
        let srgb = g.color_at(0.5);
        assert!((srgb.r - 0.5).abs() < 1e-6 && (srgb.b - 0.5).abs() < 1e-6);
        assert_eq!(g.srgb_stops().len(), 2, "nothing to add for sRGB");
        // Linear light makes a brighter middle; OKLCH keeps it saturated through purple.
        g.interpolation = Interpolation::LinearSrgb;
        assert!(g.color_at(0.5).r > 0.7);
        g.interpolation = Interpolation::Oklch;
        let mid = g.color_at(0.5);
        assert!(mid.r > 0.5 && mid.b > 0.5 && mid.g < 0.2, "{mid:?}");
        // Ends are exact in every space.
        for space in Interpolation::ALL {
            g.interpolation = space;
            let (a, b) = (g.color_at(0.0), g.color_at(1.0));
            assert!((a.r - 1.0).abs() < 1e-3 && (b.b - 1.0).abs() < 1e-3, "{space:?}");
        }
        assert!(g.srgb_stops().len() > 10, "sampled for formats that blend in sRGB");
        // Round trip through the file format.
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Gradient>(&json).unwrap(), g);
        assert!(json.contains("oklch"));
    }

    #[test]
    fn oklab_round_trips() {
        for c in [[0.2f32, 0.4, 0.9], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [0.9, 0.1, 0.3]] {
            let back = oklab_inv(oklab(c));
            assert!(c.iter().zip(back).all(|(a, b)| (a - b).abs() < 1e-4), "{c:?} → {back:?}");
        }
    }
}
