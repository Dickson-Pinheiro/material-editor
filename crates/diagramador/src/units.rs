//! Measurement primitives.
//!
//! Everything inside the engine is expressed in PDF points (1 pt = 1/72 in),
//! with the origin at the **top-left** of the page and `y` growing downward.
//! The conversion to PDF's bottom-left origin happens only at emission time.
//!
//! The public JSON accepts friendlier spellings — a bare number is points,
//! a string carries an explicit unit (`"210mm"`, `"1cm"`, `"12pt"`, `"16px"`,
//! `"0.5in"`).

use std::fmt;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};

pub const PT_PER_MM: f64 = 72.0 / 25.4;
pub const PT_PER_CM: f64 = 72.0 / 2.54;
pub const PT_PER_IN: f64 = 72.0;
/// CSS reference pixel — 96 px per inch.
pub const PT_PER_PX: f64 = 72.0 / 96.0;

// ─────────────────────────────────────────────────────────────────────────────
// Len
// ─────────────────────────────────────────────────────────────────────────────

/// A length, always stored in points.
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
pub struct Len(pub f64);

impl Len {
    pub const ZERO: Len = Len(0.0);

    #[inline]
    pub const fn pt(v: f64) -> Self {
        Len(v)
    }
    #[inline]
    pub fn mm(v: f64) -> Self {
        Len(v * PT_PER_MM)
    }
    #[inline]
    pub fn cm(v: f64) -> Self {
        Len(v * PT_PER_CM)
    }
    #[inline]
    pub fn inch(v: f64) -> Self {
        Len(v * PT_PER_IN)
    }
    #[inline]
    pub fn px(v: f64) -> Self {
        Len(v * PT_PER_PX)
    }
    #[inline]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl From<f64> for Len {
    fn from(v: f64) -> Self {
        Len(v)
    }
}

impl From<Len> for f64 {
    fn from(v: Len) -> Self {
        v.0
    }
}

/// Parse `"12pt"`, `"210mm"`, `"1.5cm"`, `"0.5in"`, `"16px"` or a bare number.
pub fn parse_len(raw: &str) -> Result<Len, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty length".into());
    }

    let (number, unit) = split_unit(s);
    let value: f64 = number
        .trim()
        .parse()
        .map_err(|_| format!("invalid length `{raw}`"))?;

    match unit {
        "" | "pt" => Ok(Len(value)),
        "mm" => Ok(Len::mm(value)),
        "cm" => Ok(Len::cm(value)),
        "in" => Ok(Len::inch(value)),
        "px" => Ok(Len::px(value)),
        other => Err(format!("unknown unit `{other}` in `{raw}`")),
    }
}

/// Split a length literal into its numeric prefix and its alphabetic suffix.
fn split_unit(s: &str) -> (&str, &str) {
    let split_at = s
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic() || *c == '%')
        .map_or(s.len(), |(i, _)| i);
    (&s[..split_at], &s[split_at..])
}

impl Serialize for Len {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(self.0)
    }
}

impl<'de> Deserialize<'de> for Len {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct LenVisitor;

        impl<'de> Visitor<'de> for LenVisitor {
            type Value = Len;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number of points or a string like \"10mm\"")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Len, E> {
                Ok(Len(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Len, E> {
                Ok(Len(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Len, E> {
                Ok(Len(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Len, E> {
                parse_len(v).map_err(E::custom)
            }
        }

        d.deserialize_any(LenVisitor)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Rect
// ─────────────────────────────────────────────────────────────────────────────

/// An axis-aligned rectangle in points, origin top-left of the page.
///
/// JSON accepts either the compact form `[x, y, w, h]` (units allowed per
/// component) or the explicit form `{ "x": …, "y": …, "w": …, "h": … }`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    #[inline]
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    #[inline]
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// Shrink the rectangle inward by `insets`.
    pub fn deflate(&self, insets: Insets) -> Rect {
        Rect {
            x: self.x + insets.left,
            y: self.y + insets.top,
            w: (self.w - insets.left - insets.right).max(0.0),
            h: (self.h - insets.top - insets.bottom).max(0.0),
        }
    }

    /// Move the rectangle by `dx` / `dy`.
    pub fn translate(&self, dx: f64, dy: f64) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            w: self.w,
            h: self.h,
        }
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }
}

impl<'de> Deserialize<'de> for Rect {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct RectVisitor;

        impl<'de> Visitor<'de> for RectVisitor {
            type Value = Rect;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("[x, y, w, h] or { x, y, w, h }")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Rect, A::Error> {
                let mut v = [0.0f64; 4];
                for (i, slot) in v.iter_mut().enumerate() {
                    *slot = seq
                        .next_element::<Len>()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?
                        .get();
                }
                Ok(Rect::new(v[0], v[1], v[2], v[3]))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Rect, A::Error> {
                let (mut x, mut y, mut w, mut h) = (0.0, 0.0, 0.0, 0.0);
                while let Some(key) = map.next_key::<String>()? {
                    let val = map.next_value::<Len>()?.get();
                    match key.as_str() {
                        "x" => x = val,
                        "y" => y = val,
                        "w" | "width" => w = val,
                        "h" | "height" => h = val,
                        _ => {}
                    }
                }
                Ok(Rect::new(x, y, w, h))
            }
        }

        d.deserialize_any(RectVisitor)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Insets
// ─────────────────────────────────────────────────────────────────────────────

/// Edge insets in points — used for page margins and frame padding.
///
/// JSON accepts CSS-like shorthands: a single number, `[vertical, horizontal]`,
/// `[top, right, bottom, left]`, or `{ "top": …, "left": … }`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Insets {
    pub const ZERO: Insets = Insets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub const fn all(v: f64) -> Self {
        Insets {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    pub const fn symmetric(vertical: f64, horizontal: f64) -> Self {
        Insets {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    #[inline]
    pub fn horizontal(&self) -> f64 {
        self.left + self.right
    }
    #[inline]
    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
    }
}

impl<'de> Deserialize<'de> for Insets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct InsetsVisitor;

        impl<'de> Visitor<'de> for InsetsVisitor {
            type Value = Insets;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number, [v, h], [t, r, b, l] or { top, right, bottom, left }")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Insets, E> {
                Ok(Insets::all(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Insets, E> {
                Ok(Insets::all(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Insets, E> {
                Ok(Insets::all(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Insets, E> {
                parse_len(v).map(|l| Insets::all(l.get())).map_err(E::custom)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Insets, A::Error> {
                let mut vals = Vec::with_capacity(4);
                while let Some(v) = seq.next_element::<Len>()? {
                    vals.push(v.get());
                }
                Ok(match vals.len() {
                    1 => Insets::all(vals[0]),
                    2 => Insets::symmetric(vals[0], vals[1]),
                    3 => Insets {
                        top: vals[0],
                        right: vals[1],
                        bottom: vals[2],
                        left: vals[1],
                    },
                    4 => Insets {
                        top: vals[0],
                        right: vals[1],
                        bottom: vals[2],
                        left: vals[3],
                    },
                    _ => Insets::ZERO,
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Insets, A::Error> {
                let mut out = Insets::ZERO;
                while let Some(key) = map.next_key::<String>()? {
                    let val = map.next_value::<Len>()?.get();
                    match key.as_str() {
                        "top" => out.top = val,
                        "right" => out.right = val,
                        "bottom" => out.bottom = val,
                        "left" => out.left = val,
                        "x" | "horizontal" => {
                            out.left = val;
                            out.right = val;
                        }
                        "y" | "vertical" => {
                            out.top = val;
                            out.bottom = val;
                        }
                        "all" => out = Insets::all(val),
                        _ => {}
                    }
                }
                Ok(out)
            }
        }

        d.deserialize_any(InsetsVisitor)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PageSize
// ─────────────────────────────────────────────────────────────────────────────

/// A page size in points. JSON accepts a named size (`"A4"`, `"letter"`,
/// optionally suffixed `"A4 landscape"`) or an explicit `[width, height]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

impl PageSize {
    pub const fn new(width: f64, height: f64) -> Self {
        PageSize { width, height }
    }

    pub fn landscape(self) -> Self {
        PageSize::new(self.height, self.width)
    }

    /// Resolve a named page size. Returns `None` for unknown names.
    pub fn named(name: &str) -> Option<Self> {
        let mm = |w: f64, h: f64| PageSize::new(w * PT_PER_MM, h * PT_PER_MM);
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "a0" => mm(841.0, 1189.0),
            "a1" => mm(594.0, 841.0),
            "a2" => mm(420.0, 594.0),
            "a3" => mm(297.0, 420.0),
            "a4" => mm(210.0, 297.0),
            "a5" => mm(148.0, 210.0),
            "a6" => mm(105.0, 148.0),
            "b5" => mm(176.0, 250.0),
            "letter" => PageSize::new(612.0, 792.0),
            "legal" => PageSize::new(612.0, 1008.0),
            "tabloid" => PageSize::new(792.0, 1224.0),
            // Common Brazilian schoolbook trim sizes.
            "livro-didatico" | "textbook" => mm(205.0, 275.0),
            "meia-carta" | "half-letter" => PageSize::new(396.0, 612.0),
            _ => return None,
        })
    }
}

impl Default for PageSize {
    fn default() -> Self {
        // A4 portrait.
        PageSize::named("a4").expect("a4 is a known page size")
    }
}

impl<'de> Deserialize<'de> for PageSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct PageSizeVisitor;

        impl<'de> Visitor<'de> for PageSizeVisitor {
            type Value = PageSize;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a named page size, [width, height] or { width, height }")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<PageSize, E> {
                let lower = v.trim().to_ascii_lowercase();
                let (name, rotate) = match lower.strip_suffix("landscape") {
                    Some(rest) => (rest.trim_end_matches([' ', '-', '_']), true),
                    None => (lower.trim_end_matches("portrait").trim(), false),
                };
                let size = PageSize::named(name)
                    .ok_or_else(|| E::custom(format!("unknown page size `{v}`")))?;
                Ok(if rotate { size.landscape() } else { size })
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<PageSize, A::Error> {
                let w = seq
                    .next_element::<Len>()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let h = seq
                    .next_element::<Len>()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(PageSize::new(w.get(), h.get()))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<PageSize, A::Error> {
                let mut size = PageSize::default();
                let mut rotate = false;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "width" | "w" => size.width = map.next_value::<Len>()?.get(),
                        "height" | "h" => size.height = map.next_value::<Len>()?.get(),
                        "name" => {
                            let name: String = map.next_value()?;
                            size = PageSize::named(&name).ok_or_else(|| {
                                de::Error::custom(format!("unknown page size `{name}`"))
                            })?;
                        }
                        "orientation" => {
                            let o: String = map.next_value()?;
                            rotate = o.eq_ignore_ascii_case("landscape");
                        }
                        _ => {
                            let _ = map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(if rotate { size.landscape() } else { size })
            }
        }

        d.deserialize_any(PageSizeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn len_parses_units() {
        assert_eq!(parse_len("12").unwrap(), Len(12.0));
        assert_eq!(parse_len("12pt").unwrap(), Len(12.0));
        assert_eq!(parse_len("1in").unwrap(), Len(72.0));
        assert!((parse_len("10mm").unwrap().get() - 28.3465).abs() < 0.01);
        assert!((parse_len("1cm").unwrap().get() - 28.3465).abs() < 0.01);
        assert_eq!(parse_len("16px").unwrap(), Len(12.0));
        assert!(parse_len("3parsecs").is_err());
    }

    #[test]
    fn len_deserializes_from_number_or_string() {
        assert_eq!(serde_json::from_str::<Len>("18").unwrap(), Len(18.0));
        assert_eq!(serde_json::from_str::<Len>("18.5").unwrap(), Len(18.5));
        assert_eq!(serde_json::from_str::<Len>(r#""1in""#).unwrap(), Len(72.0));
    }

    #[test]
    fn rect_accepts_array_and_object() {
        let from_array: Rect = serde_json::from_str("[10, 20, 30, 40]").unwrap();
        assert_eq!(from_array, Rect::new(10.0, 20.0, 30.0, 40.0));

        let from_object: Rect =
            serde_json::from_str(r#"{"x":10,"y":20,"width":30,"height":40}"#).unwrap();
        assert_eq!(from_object, from_array);

        let with_units: Rect = serde_json::from_str(r#"["1in", 0, "2in", 0]"#).unwrap();
        assert_eq!(with_units, Rect::new(72.0, 0.0, 144.0, 0.0));
    }

    #[test]
    fn insets_accept_css_shorthands() {
        assert_eq!(serde_json::from_str::<Insets>("5").unwrap(), Insets::all(5.0));
        assert_eq!(
            serde_json::from_str::<Insets>("[5, 10]").unwrap(),
            Insets::symmetric(5.0, 10.0)
        );
        assert_eq!(
            serde_json::from_str::<Insets>("[1, 2, 3, 4]").unwrap(),
            Insets { top: 1.0, right: 2.0, bottom: 3.0, left: 4.0 }
        );
        assert_eq!(
            serde_json::from_str::<Insets>(r#"{"vertical":2,"left":9}"#).unwrap(),
            Insets { top: 2.0, right: 0.0, bottom: 2.0, left: 9.0 }
        );
    }

    #[test]
    fn page_size_named_and_explicit() {
        let a4: PageSize = serde_json::from_str(r#""A4""#).unwrap();
        assert!((a4.width - 595.28).abs() < 0.1);
        assert!((a4.height - 841.89).abs() < 0.1);

        let landscape: PageSize = serde_json::from_str(r#""A4 landscape""#).unwrap();
        assert!((landscape.width - a4.height).abs() < 0.001);

        let explicit: PageSize = serde_json::from_str(r#"["100mm", "200mm"]"#).unwrap();
        assert!((explicit.height - explicit.width * 2.0).abs() < 0.001);
    }

    #[test]
    fn rect_deflate_clamps_to_zero() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let deflated = r.deflate(Insets::all(20.0));
        assert_eq!(deflated.w, 0.0);
        assert_eq!(deflated.h, 0.0);
    }
}
