use serde::{Deserialize, Serialize};

/// A non-premultiplied sRGB color with components in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Self = Self::rgba(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Self = Self::rgba(1.0, 1.0, 1.0, 1.0);
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);
    /// The default fill for new shapes.
    pub const DEFAULT_FILL: Self = Self::rgba(0.847, 0.847, 0.847, 1.0);
    /// The default border for new shapes.
    pub const DEFAULT_BORDER: Self = Self::rgba(0.592, 0.592, 0.592, 1.0);

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0)
    }

    pub fn to_rgba8(self) -> [u8; 4] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// `RRGGBB` or `RRGGBBAA`, without a leading `#`.
    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.to_rgba8();
        if a == 255 {
            format!("{r:02X}{g:02X}{b:02X}")
        } else {
            format!("{r:02X}{g:02X}{b:02X}{a:02X}")
        }
    }

    /// Parses `RGB`, `RRGGBB` or `RRGGBBAA`, with or without a leading `#`.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            3 => {
                let nib = |i: usize| u8::from_str_radix(s.get(i..i + 1)?, 16).ok().map(|v| v * 17);
                Some(Self::from_rgba8(nib(0)?, nib(1)?, nib(2)?, 255))
            }
            6 => Some(Self::from_rgba8(byte(0)?, byte(2)?, byte(4)?, 255)),
            8 => Some(Self::from_rgba8(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = Color::from_rgba8(0x12, 0xAB, 0xEF, 0x80);
        assert_eq!(c.to_hex(), "12ABEF80");
        assert_eq!(Color::from_hex("#12ABEF80"), Some(c));
        assert_eq!(Color::from_hex("fff"), Some(Color::WHITE));
        assert_eq!(Color::from_hex("12ABEF").unwrap().to_hex(), "12ABEF");
        assert_eq!(Color::from_hex("nope"), None);
    }
}
