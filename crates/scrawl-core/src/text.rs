//! The text layer model. Layout lives in the `scrawl-text` crate; this is just data.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayer {
    pub text: String,
    /// Serialized as `textStyle`: text layers are flattened into the layer object, which already
    /// has a `style`.
    #[serde(default, rename = "textStyle")]
    pub style: TextStyle,
    #[serde(default)]
    pub sizing: TextSizing,
}

impl TextLayer {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: TextStyle::default(), sizing: TextSizing::Auto }
    }

    /// The text as displayed, after the case transform.
    pub fn display_text(&self) -> String {
        match self.style.transform {
            TextTransform::None => self.text.clone(),
            TextTransform::Uppercase => self.text.to_uppercase(),
            TextTransform::Lowercase => self.text.to_lowercase(),
        }
    }
}

/// How a text layer's frame relates to its content (Auto / Fixed width / Fixed).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextSizing {
    /// The frame hugs the text; lines only break at newlines.
    #[default]
    Auto,
    /// Text wraps at the frame width; the height follows the content.
    FixedWidth,
    /// The frame is set by the user; text wraps and may overflow.
    Fixed,
}

impl TextSizing {
    pub const ALL: [Self; 3] = [Self::Auto, Self::FixedWidth, Self::Fixed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::FixedWidth => "Fixed Width",
            Self::Fixed => "Fixed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    /// A font family name. Empty means the platform UI font.
    pub font_family: String,
    pub font_size: f64,
    /// CSS weight, 100–900.
    pub font_weight: u16,
    pub italic: bool,
    /// Absolute line height in points; `None` uses the font's natural spacing.
    pub line_height: Option<f64>,
    pub letter_spacing: f64,
    pub alignment: TextAlign,
    /// Where the text sits in a fixed-size frame.
    pub vertical_alignment: VerticalAlign,
    pub underline: bool,
    pub strikethrough: bool,
    pub transform: TextTransform,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 16.0,
            font_weight: 400,
            italic: false,
            line_height: None,
            letter_spacing: 0.0,
            alignment: TextAlign::Left,
            vertical_alignment: VerticalAlign::Top,
            underline: false,
            strikethrough: false,
            transform: TextTransform::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerticalAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
}

/// Names for CSS font weights, as shown in font pickers.
pub const WEIGHT_NAMES: [(u16, &str); 9] = [
    (100, "Thin"),
    (200, "Extra Light"),
    (300, "Light"),
    (400, "Regular"),
    (500, "Medium"),
    (600, "Semibold"),
    (700, "Bold"),
    (800, "Extra Bold"),
    (900, "Black"),
];

pub fn weight_name(weight: u16) -> &'static str {
    WEIGHT_NAMES.iter().min_by_key(|(w, _)| w.abs_diff(weight)).map_or("Regular", |(_, n)| n)
}
