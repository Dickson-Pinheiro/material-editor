//! Style: the partial (cascading) form and the fully-resolved form.
//!
//! Every field of [`Style`] is optional. Absent fields inherit from the
//! enclosing scope — document → page → frame → block → run. The cascade is
//! applied once, in `layout::cascade`, producing a [`ResolvedStyle`] where
//! every value is concrete.

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

use crate::color::Color;
use crate::units::Len;

// ─────────────────────────────────────────────────────────────────────────────
// Style (partial)
// ─────────────────────────────────────────────────────────────────────────────

/// A partial style. `None` means "inherit".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Style {
    /// Name of a style declared in `resources.styles` to inherit from.
    pub extends: Option<String>,

    pub font_family: Option<String>,
    pub font_size: Option<Len>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,

    pub color: Option<Color>,
    /// Painted behind the text run itself (highlighting), not behind the frame.
    pub background: Option<Color>,

    /// A number is a multiple of the font size; a string is an absolute length.
    pub line_height: Option<LineHeight>,
    pub letter_spacing: Option<Len>,
    pub word_spacing: Option<Len>,

    pub text_align: Option<TextAlign>,
    pub text_transform: Option<TextTransform>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,

    pub space_before: Option<Len>,
    pub space_after: Option<Len>,
    pub indent_first: Option<Len>,
    pub indent_left: Option<Len>,
    pub indent_right: Option<Len>,

    /// Raises (positive) or lowers (negative) the run relative to the baseline.
    pub baseline_shift: Option<Len>,
    /// Scales the font size for this run — used for sub/superscripts.
    pub font_scale: Option<f64>,

    /// Keep this paragraph on the same fragment as the next one.
    pub keep_with_next: Option<bool>,
}

impl Style {
    /// Overlay `over` on top of `self`; set fields in `over` win.
    pub fn merge(&self, over: &Style) -> Style {
        macro_rules! pick {
            ($($field:ident),* $(,)?) => {
                Style {
                    $( $field: over.$field.clone().or_else(|| self.$field.clone()), )*
                }
            };
        }
        pick!(
            extends,
            font_family,
            font_size,
            font_weight,
            font_style,
            color,
            background,
            line_height,
            letter_spacing,
            word_spacing,
            text_align,
            text_transform,
            underline,
            strikethrough,
            space_before,
            space_after,
            indent_first,
            indent_left,
            indent_right,
            baseline_shift,
            font_scale,
            keep_with_next,
        )
    }

    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Enumerations
// ─────────────────────────────────────────────────────────────────────────────

/// OpenType weight class, 100–900. Accepts numbers or CSS keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const NORMAL: FontWeight = FontWeight(400);
    pub const BOLD: FontWeight = FontWeight(700);

    /// Whether this weight should select the bold face of a family.
    #[inline]
    pub fn is_bold(self) -> bool {
        self.0 >= 600
    }
}

impl Default for FontWeight {
    fn default() -> Self {
        FontWeight::NORMAL
    }
}

impl Serialize for FontWeight {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u16(self.0)
    }
}

impl<'de> Deserialize<'de> for FontWeight {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct WeightVisitor;

        impl<'de> Visitor<'de> for WeightVisitor {
            type Value = FontWeight;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("100–900 or a CSS weight keyword")
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<FontWeight, E> {
                Ok(FontWeight((v as u16).clamp(1, 1000)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<FontWeight, E> {
                Ok(FontWeight((v.max(1) as u16).clamp(1, 1000)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<FontWeight, E> {
                Ok(FontWeight((v.round().max(1.0) as u16).clamp(1, 1000)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<FontWeight, E> {
                Ok(match v.trim().to_ascii_lowercase().as_str() {
                    "thin" => FontWeight(100),
                    "extralight" | "ultralight" => FontWeight(200),
                    "light" => FontWeight(300),
                    "normal" | "regular" | "book" => FontWeight(400),
                    "medium" => FontWeight(500),
                    "semibold" | "demibold" => FontWeight(600),
                    "bold" => FontWeight(700),
                    "extrabold" | "ultrabold" => FontWeight(800),
                    "black" | "heavy" => FontWeight(900),
                    other => {
                        return other
                            .parse::<u16>()
                            .map(FontWeight)
                            .map_err(|_| E::custom(format!("unknown font weight `{v}`")));
                    }
                })
            }
        }

        d.deserialize_any(WeightVisitor)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    /// Treated as italic when the family has no separate oblique face.
    Oblique,
}

impl FontStyle {
    #[inline]
    pub fn is_italic(self) -> bool {
        matches!(self, FontStyle::Italic | FontStyle::Oblique)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

impl TextTransform {
    pub fn apply(self, text: &str) -> String {
        match self {
            TextTransform::None => text.to_string(),
            TextTransform::Uppercase => text.to_uppercase(),
            TextTransform::Lowercase => text.to_lowercase(),
            TextTransform::Capitalize => capitalize_words(text),
        }
    }
}

fn capitalize_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_word_start = true;
    for ch in text.chars() {
        if at_word_start {
            out.extend(ch.to_uppercase());
        } else {
            out.push(ch);
        }
        at_word_start = ch.is_whitespace();
    }
    out
}

/// Vertical placement of content inside a frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerticalAlign {
    #[default]
    Top,
    Middle,
    Bottom,
    /// Stretch inter-paragraph spacing so the content fills the frame.
    Justify,
}

/// What to do with content that does not fit in its frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Overflow {
    /// Drop it, and report the frame as overset (InDesign's red `+`).
    #[default]
    Clip,
    /// Render past the frame bounds.
    Visible,
    /// Grow the frame downward to fit.
    Grow,
}

// ─────────────────────────────────────────────────────────────────────────────
// LineHeight
// ─────────────────────────────────────────────────────────────────────────────

/// Distance between consecutive baselines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    /// Multiple of the font size.
    Multiple(f64),
    /// Absolute length in points.
    Absolute(f64),
}

impl LineHeight {
    #[inline]
    pub fn resolve(self, font_size: f64) -> f64 {
        match self {
            LineHeight::Multiple(m) => font_size * m,
            LineHeight::Absolute(v) => v,
        }
    }
}

impl Default for LineHeight {
    fn default() -> Self {
        LineHeight::Multiple(1.4)
    }
}

impl Serialize for LineHeight {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            LineHeight::Multiple(m) => s.serialize_f64(*m),
            LineHeight::Absolute(v) => s.serialize_str(&format!("{v}pt")),
        }
    }
}

impl<'de> Deserialize<'de> for LineHeight {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct LineHeightVisitor;

        impl<'de> Visitor<'de> for LineHeightVisitor {
            type Value = LineHeight;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a multiple of the font size, or an absolute length string")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<LineHeight, E> {
                Ok(LineHeight::Multiple(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<LineHeight, E> {
                Ok(LineHeight::Multiple(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<LineHeight, E> {
                Ok(LineHeight::Multiple(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<LineHeight, E> {
                crate::units::parse_len(v)
                    .map(|l| LineHeight::Absolute(l.get()))
                    .map_err(E::custom)
            }
        }

        d.deserialize_any(LineHeightVisitor)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ResolvedStyle
// ─────────────────────────────────────────────────────────────────────────────

/// A style with every value made concrete. Produced by the cascade phase.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStyle {
    pub font_family: Option<String>,
    pub font_size: f64,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,

    pub color: Color,
    pub background: Option<Color>,

    pub line_height: LineHeight,
    pub letter_spacing: f64,
    pub word_spacing: f64,

    pub text_align: TextAlign,
    pub text_transform: TextTransform,
    pub underline: bool,
    pub strikethrough: bool,

    pub space_before: f64,
    pub space_after: f64,
    pub indent_first: f64,
    pub indent_left: f64,
    pub indent_right: f64,

    pub baseline_shift: f64,
    pub keep_with_next: bool,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        ResolvedStyle {
            font_family: None,
            font_size: 11.0,
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            color: Color::BLACK,
            background: None,
            line_height: LineHeight::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_align: TextAlign::Left,
            text_transform: TextTransform::None,
            underline: false,
            strikethrough: false,
            space_before: 0.0,
            space_after: 0.0,
            indent_first: 0.0,
            indent_left: 0.0,
            indent_right: 0.0,
            baseline_shift: 0.0,
            keep_with_next: false,
        }
    }
}

impl ResolvedStyle {
    /// Baseline-to-baseline distance for this style.
    #[inline]
    pub fn leading(&self) -> f64 {
        self.line_height.resolve(self.font_size)
    }

    /// Apply a partial style on top, producing a new resolved style.
    ///
    /// `font_scale` multiplies the inherited size — this is how sub/superscript
    /// and relative sizing work without needing a separate mechanism.
    pub fn apply(&self, patch: &Style) -> ResolvedStyle {
        let mut out = self.clone();

        if let Some(v) = &patch.font_family {
            out.font_family = Some(v.clone());
        }
        if let Some(v) = patch.font_size {
            out.font_size = v.get();
        }
        if let Some(scale) = patch.font_scale {
            out.font_size *= scale;
        }
        if let Some(v) = patch.font_weight {
            out.font_weight = v;
        }
        if let Some(v) = patch.font_style {
            out.font_style = v;
        }
        if let Some(v) = patch.color {
            out.color = v;
        }
        if let Some(v) = patch.background {
            out.background = if v.is_transparent() { None } else { Some(v) };
        }
        if let Some(v) = patch.line_height {
            out.line_height = v;
        }
        if let Some(v) = patch.letter_spacing {
            out.letter_spacing = v.get();
        }
        if let Some(v) = patch.word_spacing {
            out.word_spacing = v.get();
        }
        if let Some(v) = patch.text_align {
            out.text_align = v;
        }
        if let Some(v) = patch.text_transform {
            out.text_transform = v;
        }
        if let Some(v) = patch.underline {
            out.underline = v;
        }
        if let Some(v) = patch.strikethrough {
            out.strikethrough = v;
        }
        if let Some(v) = patch.space_before {
            out.space_before = v.get();
        }
        if let Some(v) = patch.space_after {
            out.space_after = v.get();
        }
        if let Some(v) = patch.indent_first {
            out.indent_first = v.get();
        }
        if let Some(v) = patch.indent_left {
            out.indent_left = v.get();
        }
        if let Some(v) = patch.indent_right {
            out.indent_right = v.get();
        }
        if let Some(v) = patch.baseline_shift {
            out.baseline_shift = v.get();
        }
        if let Some(v) = patch.keep_with_next {
            out.keep_with_next = v;
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_weight_accepts_numbers_and_keywords() {
        assert_eq!(
            serde_json::from_str::<FontWeight>("700").unwrap(),
            FontWeight::BOLD
        );
        assert_eq!(
            serde_json::from_str::<FontWeight>(r#""bold""#).unwrap(),
            FontWeight::BOLD
        );
        assert_eq!(
            serde_json::from_str::<FontWeight>(r#""semibold""#).unwrap(),
            FontWeight(600)
        );
        assert!(serde_json::from_str::<FontWeight>(r#""bold""#).unwrap().is_bold());
        assert!(!serde_json::from_str::<FontWeight>("400").unwrap().is_bold());
    }

    #[test]
    fn line_height_number_is_multiple_string_is_absolute() {
        let m: LineHeight = serde_json::from_str("1.5").unwrap();
        assert_eq!(m, LineHeight::Multiple(1.5));
        assert_eq!(m.resolve(10.0), 15.0);

        let a: LineHeight = serde_json::from_str(r#""18pt""#).unwrap();
        assert_eq!(a, LineHeight::Absolute(18.0));
        assert_eq!(a.resolve(10.0), 18.0);
    }

    #[test]
    fn style_merge_prefers_the_overlay() {
        let base = Style {
            font_size: Some(Len(10.0)),
            underline: Some(true),
            ..Default::default()
        };
        let over = Style {
            font_size: Some(Len(14.0)),
            ..Default::default()
        };
        let merged = base.merge(&over);
        assert_eq!(merged.font_size, Some(Len(14.0)));
        assert_eq!(merged.underline, Some(true));
    }

    #[test]
    fn resolved_apply_sets_only_present_fields() {
        let base = ResolvedStyle::default();
        let patch = Style {
            font_size: Some(Len(20.0)),
            ..Default::default()
        };
        let out = base.apply(&patch);
        assert_eq!(out.font_size, 20.0);
        assert_eq!(out.color, base.color);
        assert_eq!(out.text_align, base.text_align);
    }

    #[test]
    fn font_scale_multiplies_inherited_size() {
        let base = ResolvedStyle {
            font_size: 12.0,
            ..Default::default()
        };
        let out = base.apply(&Style {
            font_scale: Some(0.65),
            ..Default::default()
        });
        assert!((out.font_size - 7.8).abs() < 1e-9);
    }

    #[test]
    fn explicit_size_applies_before_scale() {
        let out = ResolvedStyle::default().apply(&Style {
            font_size: Some(Len(10.0)),
            font_scale: Some(2.0),
            ..Default::default()
        });
        assert_eq!(out.font_size, 20.0);
    }

    #[test]
    fn transparent_background_clears_inherited_one() {
        let base = ResolvedStyle {
            background: Some(Color::WHITE),
            ..Default::default()
        };
        let out = base.apply(&Style {
            background: Some(Color::TRANSPARENT),
            ..Default::default()
        });
        assert_eq!(out.background, None);
    }

    #[test]
    fn text_transform_capitalize() {
        assert_eq!(
            TextTransform::Capitalize.apply("olá mundo cruel"),
            "Olá Mundo Cruel"
        );
    }
}
