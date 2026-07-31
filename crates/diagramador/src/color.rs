//! CSS-ish color parsing.
//!
//! Colors are stored as normalised RGBA in `[0, 1]` so the PDF emitter and the
//! browser painter consume exactly the same numbers.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// A normalised RGBA color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const TRANSPARENT: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    #[inline]
    pub fn is_transparent(&self) -> bool {
        self.a <= f32::EPSILON
    }

    /// `#rrggbb`, or `#rrggbbaa` when the color is not fully opaque.
    pub fn to_hex(self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                c(self.r),
                c(self.g),
                c(self.b),
                c(self.a)
            )
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::BLACK
    }
}

/// Parse a CSS color literal.
///
/// Accepts `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)`,
/// `transparent`, and a small set of named colors.
pub fn parse_color(raw: &str) -> Result<Color, String> {
    let s = raw.trim();

    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex).ok_or_else(|| format!("invalid hex color `{raw}`"));
    }

    let lower = s.to_ascii_lowercase();

    if let Some(args) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return parse_rgb_function(args).ok_or_else(|| format!("invalid color `{raw}`"));
    }

    named_color(&lower).ok_or_else(|| format!("unknown color `{raw}`"))
}

fn parse_hex(hex: &str) -> Option<Color> {
    let nibble = |c: u8| (c as char).to_digit(16).map(|v| v as f32);
    let bytes = hex.as_bytes();

    match bytes.len() {
        // #rgb / #rgba — each nibble is doubled, so `f` means `ff`.
        3 | 4 => {
            let v: Option<Vec<f32>> = bytes.iter().map(|&c| nibble(c).map(|n| n / 15.0)).collect();
            let v = v?;
            Some(Color::rgba(v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0)))
        }
        6 | 8 => {
            let mut v = Vec::with_capacity(4);
            for pair in bytes.chunks(2) {
                let hi = nibble(pair[0])?;
                let lo = nibble(pair[1])?;
                v.push((hi * 16.0 + lo) / 255.0);
            }
            Some(Color::rgba(v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0)))
        }
        _ => None,
    }
}

fn parse_rgb_function(args: &str) -> Option<Color> {
    let parts: Vec<&str> = args
        .split([',', '/', ' '])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }

    // Channels are 0-255 (or a percentage); alpha is 0-1 (or a percentage).
    let channel = |p: &str| -> Option<f32> {
        match p.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().ok().map(|v| v / 100.0),
            None => p.parse::<f32>().ok().map(|v| v / 255.0),
        }
        .map(|v| v.clamp(0.0, 1.0))
    };
    let alpha = |p: &str| -> Option<f32> {
        match p.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().ok().map(|v| v / 100.0),
            None => p.parse::<f32>().ok(),
        }
        .map(|v| v.clamp(0.0, 1.0))
    };

    Some(Color::rgba(
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        parts.get(3).and_then(|p| alpha(p)).unwrap_or(1.0),
    ))
}

fn named_color(name: &str) -> Option<Color> {
    let hex = match name {
        "transparent" => return Some(Color::TRANSPARENT),
        "black" => "000000",
        "white" => "ffffff",
        "red" => "ff0000",
        "green" => "008000",
        "lime" => "00ff00",
        "blue" => "0000ff",
        "yellow" => "ffff00",
        "cyan" | "aqua" => "00ffff",
        "magenta" | "fuchsia" => "ff00ff",
        "gray" | "grey" => "808080",
        "lightgray" | "lightgrey" => "d3d3d3",
        "darkgray" | "darkgrey" => "a9a9a9",
        "silver" => "c0c0c0",
        "orange" => "ffa500",
        "purple" => "800080",
        "navy" => "000080",
        "teal" => "008080",
        "olive" => "808000",
        "maroon" => "800000",
        "brown" => "a52a2a",
        "pink" => "ffc0cb",
        "beige" => "f5f5dc",
        _ => return None,
    };
    parse_hex(hex)
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ColorVisitor;

        impl<'de> Visitor<'de> for ColorVisitor {
            type Value = Color;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a CSS color string")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Color, E> {
                parse_color(v).map_err(E::custom)
            }
        }

        d.deserialize_str(ColorVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.005
    }

    #[test]
    fn parses_long_hex() {
        let c = parse_color("#336699").unwrap();
        assert!(approx(c.r, 0.2) && approx(c.g, 0.4) && approx(c.b, 0.6));
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn parses_short_hex_by_doubling_nibbles() {
        assert_eq!(parse_color("#f00").unwrap(), Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(parse_color("#fff").unwrap(), Color::WHITE);
    }

    #[test]
    fn parses_hex_with_alpha() {
        let c = parse_color("#00000080").unwrap();
        assert!(approx(c.a, 0.502));
    }

    #[test]
    fn parses_rgb_functions() {
        assert_eq!(parse_color("rgb(255, 0, 0)").unwrap(), Color::rgb(1.0, 0.0, 0.0));
        let c = parse_color("rgba(0, 0, 0, 0.5)").unwrap();
        assert!(approx(c.a, 0.5));
    }

    #[test]
    fn parses_named_colors() {
        assert_eq!(parse_color("black").unwrap(), Color::BLACK);
        assert_eq!(parse_color("  White ").unwrap(), Color::WHITE);
        assert!(parse_color("transparent").unwrap().is_transparent());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("chartreuse-ish").is_err());
    }

    #[test]
    fn hex_roundtrips() {
        let c = parse_color("#336699").unwrap();
        assert_eq!(c.to_hex(), "#336699");
        assert_eq!(parse_color(&c.to_hex()).unwrap(), c);
    }
}
