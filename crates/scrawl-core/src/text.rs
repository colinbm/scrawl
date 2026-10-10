//! The text layer model. Layout lives in the `scrawl-text` crate; this is just data.
//!
//! A layer has one [`TextStyle`] for the whole text plus optional [`TextRun`]s: byte ranges that
//! override character-level properties (font, size, weight, color…). Paragraph-level properties
//! (alignment, line height, sizing) stay with the layer.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::Color;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayer {
    pub text: String,
    /// Serialized as `textStyle`: text layers are flattened into the layer object, which already
    /// has a `style`.
    #[serde(default, rename = "textStyle")]
    pub style: TextStyle,
    #[serde(default)]
    pub sizing: TextSizing,
    /// Character styles for parts of the text, sorted and not overlapping.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<TextRun>,
}

/// A range of the text (byte offsets) with styles of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub start: usize,
    pub end: usize,
    #[serde(flatten)]
    pub style: RunStyle,
}

/// Character-level overrides; `None` keeps the layer's value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    /// Paints this range instead of the layer's fills.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

impl RunStyle {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `base` with these overrides applied.
    pub fn apply(&self, base: &TextStyle) -> TextStyle {
        let mut s = base.clone();
        if let Some(v) = &self.font_family {
            s.font_family = v.clone();
        }
        s.font_size = self.font_size.unwrap_or(s.font_size);
        s.font_weight = self.font_weight.unwrap_or(s.font_weight);
        s.italic = self.italic.unwrap_or(s.italic);
        s.letter_spacing = self.letter_spacing.unwrap_or(s.letter_spacing);
        s.underline = self.underline.unwrap_or(s.underline);
        s.strikethrough = self.strikethrough.unwrap_or(s.strikethrough);
        s
    }

    /// The overrides that turn `base` into `styled` (character properties only), keeping this
    /// run's color.
    pub fn diff(base: &TextStyle, styled: &TextStyle, color: Option<Color>) -> Self {
        let some = |same: bool| !same;
        Self {
            font_family: some(base.font_family == styled.font_family).then(|| styled.font_family.clone()),
            font_size: some(base.font_size == styled.font_size).then_some(styled.font_size),
            font_weight: some(base.font_weight == styled.font_weight).then_some(styled.font_weight),
            italic: some(base.italic == styled.italic).then_some(styled.italic),
            letter_spacing: some(base.letter_spacing == styled.letter_spacing).then_some(styled.letter_spacing),
            underline: some(base.underline == styled.underline).then_some(styled.underline),
            strikethrough: some(base.strikethrough == styled.strikethrough).then_some(styled.strikethrough),
            color,
        }
    }
}

impl TextLayer {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: TextStyle::default(), sizing: TextSizing::Auto, runs: vec![] }
    }

    /// The run covering byte `at`, if any.
    pub fn run_at(&self, at: usize) -> Option<&TextRun> {
        self.runs.iter().find(|r| r.start <= at && at < r.end)
    }

    /// The character style (and run color) in effect at byte `at`.
    pub fn style_at(&self, at: usize) -> (TextStyle, Option<Color>) {
        match self.run_at(at) {
            Some(r) => (r.style.apply(&self.style), r.style.color),
            None => (self.style.clone(), None),
        }
    }

    /// Changes the run styles over `range` with `f`, splitting and merging runs as needed.
    pub fn set_run_style(&mut self, range: Range<usize>, f: impl Fn(&mut RunStyle)) {
        let len = self.text.len();
        let range = range.start.min(len)..range.end.min(len);
        if range.is_empty() {
            return;
        }
        // Cut the text at every run edge and the range's ends, then restyle the pieces inside.
        let mut cuts = vec![0, len, range.start, range.end];
        for r in &self.runs {
            cuts.extend([r.start, r.end]);
        }
        cuts.retain(|&c| c <= len);
        cuts.sort_unstable();
        cuts.dedup();
        let mut runs = vec![];
        for w in cuts.windows(2) {
            let (start, end) = (w[0], w[1]);
            let mut style = self.run_at(start).map(|r| r.style.clone()).unwrap_or_default();
            if start >= range.start && end <= range.end {
                f(&mut style);
            }
            runs.push(TextRun { start, end, style });
        }
        self.runs = runs;
        self.normalize();
    }

    /// Replaces the bytes in `range` with `insert`, moving the runs after it. Inserted text takes
    /// the style of the character before it, as typing does.
    pub fn replace_range(&mut self, range: Range<usize>, insert: &str) {
        let range = range.start.min(self.text.len())..range.end.min(self.text.len());
        let (start, removed, added) = (range.start, range.len(), insert.len());
        self.text.replace_range(range.clone(), insert);
        let end = start + removed;
        for r in &mut self.runs {
            // A run starting at or inside the replaced bytes starts after the new text.
            r.start = if r.start < start {
                r.start
            } else if r.start >= end {
                r.start - removed + added
            } else {
                start + added
            };
            // A run reaching the replaced bytes from before grows over the new text, so typing
            // continues the style of what precedes it; one inside them ends up empty.
            r.end = if r.end < start {
                r.end
            } else if r.end > end {
                r.end - removed + added
            } else {
                start + added
            };
        }
        self.normalize();
    }

    /// Drops empty and style-less runs, merges neighbours with the same style, and keeps every
    /// run within the text and on character boundaries.
    pub fn normalize(&mut self) {
        let len = self.text.len();
        let text = &self.text;
        let fix = |mut p: usize| {
            p = p.min(len);
            while !text.is_char_boundary(p) {
                p -= 1;
            }
            p
        };
        let mut out: Vec<TextRun> = vec![];
        let mut runs = std::mem::take(&mut self.runs);
        runs.sort_by_key(|r| r.start);
        for mut r in runs {
            r.start = fix(r.start);
            r.end = fix(r.end);
            if let Some(prev) = out.last() {
                r.start = r.start.max(prev.end);
            }
            if r.start >= r.end || r.style.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(prev) if prev.end == r.start && prev.style == r.style => prev.end = r.end,
                _ => out.push(r),
            }
        }
        self.runs = out;
    }

    /// Whether any run paints its own color.
    pub fn has_colored_runs(&self) -> bool {
        self.runs.iter().any(|r| r.style.color.is_some())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bold(t: &mut TextLayer, r: Range<usize>) {
        t.set_run_style(r, |s| s.font_weight = Some(700));
    }

    fn spans(t: &TextLayer) -> Vec<(usize, usize)> {
        t.runs.iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn styling_ranges_splits_and_merges() {
        let mut t = TextLayer::new("Hello brave world");
        bold(&mut t, 6..11);
        assert_eq!(spans(&t), [(6, 11)]);
        assert_eq!(t.style_at(7).0.font_weight, 700);
        assert_eq!(t.style_at(2).0.font_weight, 400);
        // Overlapping styling merges with the same style, splits with another.
        bold(&mut t, 9..17);
        assert_eq!(spans(&t), [(6, 17)]);
        t.set_run_style(0..8, |s| s.color = Some(Color::BLACK));
        assert_eq!(spans(&t), [(0, 6), (6, 8), (8, 17)]);
        assert_eq!(t.style_at(7).1, Some(Color::BLACK));
        // Clearing a property drops runs that no longer differ.
        t.set_run_style(0..17, |s| {
            s.color = None;
            s.font_weight = None;
        });
        assert!(t.runs.is_empty());
    }

    #[test]
    fn editing_moves_runs() {
        let mut t = TextLayer::new("Hello world");
        bold(&mut t, 6..11);
        // Typing before the run pushes it along.
        t.replace_range(0..0, "Oh, ");
        assert_eq!(spans(&t), [(10, 15)]);
        // Typing at its end continues it.
        t.replace_range(15..15, "!!");
        assert_eq!((t.text.as_str(), spans(&t)), ("Oh, Hello world!!", vec![(10, 17)]));
        // Typing at its start doesn't.
        t.replace_range(10..10, "big ");
        assert_eq!(spans(&t), [(14, 21)]);
        // Deleting across its start trims it; deleting all of it removes it.
        // "Oh, Hello big world!!": cut "g wo", which takes the run's start with it.
        t.replace_range(12..16, "");
        assert_eq!((t.text.as_str(), spans(&t)), ("Oh, Hello birld!!", vec![(12, 17)]));
        t.replace_range(11..17, "");
        assert!(t.runs.is_empty(), "{:?} {}", t.runs, t.text);
    }

    #[test]
    fn runs_stay_on_character_boundaries() {
        let mut t = TextLayer::new("café ok");
        t.set_run_style(3..4, |s| s.italic = Some(true));
        // 'é' is two bytes (3..5): the end snaps back to a boundary.
        assert!(t.runs.iter().all(|r| t.text.is_char_boundary(r.start) && t.text.is_char_boundary(r.end)));
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<TextLayer>(&json).unwrap(), t);
    }
}
