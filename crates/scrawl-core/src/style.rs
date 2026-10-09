use kurbo::Point;
use serde::{Deserialize, Serialize};

use crate::Color;

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
        self.border_outset().max(shadows)
    }
}

/// What a fill or border paints with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillType {
    #[default]
    Color,
    Gradient,
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
}

impl Fill {
    pub fn solid(color: Color) -> Self {
        Self { enabled: true, fill_type: FillType::Color, color, gradient: Gradient::default(), blend_mode: BlendMode::Normal }
    }

    pub fn paint(&self) -> Paint<'_> {
        match self.fill_type {
            FillType::Color => Paint::Color(self.color),
            FillType::Gradient => Paint::Gradient(&self.gradient),
        }
    }
}

/// A borrowed view of what to paint with.
#[derive(Clone, Copy, Debug)]
pub enum Paint<'a> {
    Color(Color),
    Gradient(&'a Gradient),
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
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            kind: GradientKind::Linear,
            from: Point::new(0.5, 0.0),
            to: Point::new(0.5, 1.0),
            stops: vec![GradientStop { position: 0.0, color: Color::WHITE }, GradientStop { position: 1.0, color: Color::BLACK }],
        }
    }
}

impl Gradient {
    /// A gradient fading `color` to transparent, a natural starting point when switching a
    /// solid fill to a gradient.
    pub fn from_color(color: Color) -> Self {
        Self {
            stops: vec![GradientStop { position: 0.0, color }, GradientStop { position: 1.0, color: color.with_alpha(0.0) }],
            ..Self::default()
        }
    }

    /// The color at `t` in `0..=1`, interpolated in sRGB between stops.
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
                let lerp = |x: f32, y: f32| x + (y - x) * k;
                return Color::rgba(lerp(a.color.r, b.color.r), lerp(a.color.g, b.color.g), lerp(a.color.b, b.color.b), lerp(a.color.a, b.color.a));
            }
        }
        self.stops.last().map_or(Color::TRANSPARENT, |s| s.color)
    }

    /// Inserts a stop at `t` with the interpolated color. Returns its index.
    pub fn add_stop(&mut self, t: f64) -> usize {
        let stop = GradientStop { position: t.clamp(0.0, 1.0), color: self.color_at(t) };
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
        }
    }

    pub fn paint(&self) -> Paint<'_> {
        match self.fill_type {
            FillType::Color => Paint::Color(self.color),
            FillType::Gradient => Paint::Gradient(&self.gradient),
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
}

impl Shadow {
    /// The default drop shadow.
    pub fn drop() -> Self {
        Self { enabled: true, color: Color::BLACK.with_alpha(0.5), offset_x: 0.0, offset_y: 2.0, blur: 4.0, spread: 0.0 }
    }

    /// The default inner shadow.
    pub fn inner() -> Self {
        Self { enabled: true, color: Color::BLACK.with_alpha(0.5), offset_x: 0.0, offset_y: 1.0, blur: 3.0, spread: 0.0 }
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
}
