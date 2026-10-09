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
}

impl Style {
    /// The style a freshly drawn shape gets.
    pub fn default_shape() -> Self {
        Self {
            fills: vec![Fill::solid(Color::DEFAULT_FILL)],
            borders: vec![Border::solid(Color::DEFAULT_BORDER, 1.0)],
        }
    }

    /// The default style for lines and open paths, which have no fill.
    pub fn default_line() -> Self {
        Self { fills: vec![], borders: vec![Border::solid(Color::DEFAULT_BORDER, 1.0)] }
    }

    /// Largest distance any enabled border paints outside the geometry.
    pub fn border_outset(&self) -> f64 {
        self.borders
            .iter()
            .filter(|b| b.enabled)
            .map(|b| match b.position {
                BorderPosition::Center => b.thickness / 2.0,
                BorderPosition::Inside => 0.0,
                BorderPosition::Outside => b.thickness,
            })
            .fold(0.0, f64::max)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub enabled: bool,
    pub color: Color,
}

impl Fill {
    pub fn solid(color: Color) -> Self {
        Self { enabled: true, color }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub enabled: bool,
    pub color: Color,
    pub thickness: f64,
    #[serde(default)]
    pub position: BorderPosition,
}

impl Border {
    pub fn solid(color: Color, thickness: f64) -> Self {
        Self { enabled: true, color, thickness, position: BorderPosition::Center }
    }
}

/// The stroke geometry used for borders of the given width: butt caps and miter joins, matching
/// The usual defaults.
pub fn stroke_style(width: f64) -> kurbo::Stroke {
    kurbo::Stroke::new(width).with_caps(kurbo::Cap::Butt).with_join(kurbo::Join::Miter)
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
