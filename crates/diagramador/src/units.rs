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
// Corners
// ─────────────────────────────────────────────────────────────────────────────

/// The four corner radii of a box, in points.
///
/// Corners go clockwise from the top-left — the order CSS `border-radius` and
/// Canvas `roundRect` both use — rather than the top/right/bottom/left order
/// [`Insets`] uses. The two are different things: an inset belongs to an
/// **edge**, a radius to a **corner**, and a shared order would only invite
/// reading one as the other.
///
/// JSON accepts the same shorthands as insets, so `"radius": 8` keeps meaning
/// what it always meant: a single number, `[top_left, top_right]` for the two
/// diagonals, `[top_left, top_right, bottom_right, bottom_left]`, or a map. It
/// writes itself back in the shortest form that still means the same thing, so
/// a document does not gain noise for having passed through the engine.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Corners {
    pub top_left: f64,
    pub top_right: f64,
    pub bottom_right: f64,
    pub bottom_left: f64,
}

impl Corners {
    pub const ZERO: Corners = Corners::all(0.0);

    pub const fn all(v: f64) -> Self {
        Corners { top_left: v, top_right: v, bottom_right: v, bottom_left: v }
    }

    /// Clockwise from the top-left.
    pub const fn new(top_left: f64, top_right: f64, bottom_right: f64, bottom_left: f64) -> Self {
        Corners { top_left, top_right, bottom_right, bottom_left }
    }

    /// Nothing to round — the caller can emit a plain rectangle.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.max() <= 0.0
    }

    #[inline]
    pub fn is_uniform(&self) -> bool {
        self.top_left == self.top_right
            && self.top_right == self.bottom_right
            && self.bottom_right == self.bottom_left
    }

    #[inline]
    pub fn max(&self) -> f64 {
        self.top_left.max(self.top_right).max(self.bottom_right).max(self.bottom_left)
    }

    /// The radii that actually fit inside a `w`×`h` box.
    ///
    /// Two radii sharing an edge can together ask for more than the edge has.
    /// CSS answers by scaling *every* corner by the single worst ratio rather
    /// than clamping each one alone: shrinking only the offending pair would
    /// bend the outline out of proportion, while one factor keeps the shape
    /// recognisably itself. Canvas `roundRect` does the same, which is what
    /// keeps the PDF and the editor's canvas drawing the same box.
    pub fn fitted(&self, w: f64, h: f64) -> Corners {
        let mut out = Corners {
            top_left: self.top_left.max(0.0),
            top_right: self.top_right.max(0.0),
            bottom_right: self.bottom_right.max(0.0),
            bottom_left: self.bottom_left.max(0.0),
        };

        // Each edge is shared by two corners; the tightest edge sets the scale.
        let ratio = |span: f64, a: f64, b: f64| {
            let want = a + b;
            if want <= span { 1.0 } else { span / want }
        };
        let scale = ratio(w, out.top_left, out.top_right)
            .min(ratio(w, out.bottom_left, out.bottom_right))
            .min(ratio(h, out.top_left, out.bottom_left))
            .min(ratio(h, out.top_right, out.bottom_right));

        if scale < 1.0 && scale.is_finite() {
            out.top_left *= scale;
            out.top_right *= scale;
            out.bottom_right *= scale;
            out.bottom_left *= scale;
        }
        out
    }
}

impl Serialize for Corners {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.is_uniform() {
            return s.serialize_f64(self.top_left);
        }
        // A pair when opposite corners mirror, four otherwise.
        if self.top_left == self.bottom_right && self.top_right == self.bottom_left {
            return [self.top_left, self.top_right].serialize(s);
        }
        [self.top_left, self.top_right, self.bottom_right, self.bottom_left].serialize(s)
    }
}

impl<'de> Deserialize<'de> for Corners {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct CornersVisitor;

        impl<'de> Visitor<'de> for CornersVisitor {
            type Value = Corners;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "a number, [tl, tr], [tl, tr, br, bl] \
                     or { topLeft, topRight, bottomRight, bottomLeft }",
                )
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Corners, E> {
                Ok(Corners::all(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Corners, E> {
                Ok(Corners::all(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Corners, E> {
                Ok(Corners::all(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Corners, E> {
                parse_len(v).map(|l| Corners::all(l.get())).map_err(E::custom)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Corners, A::Error> {
                let mut vals = Vec::with_capacity(4);
                while let Some(v) = seq.next_element::<Len>()? {
                    vals.push(v.get());
                }
                Ok(match vals.len() {
                    1 => Corners::all(vals[0]),
                    // Like CSS: the pair is the two diagonals, not two edges.
                    2 => Corners::new(vals[0], vals[1], vals[0], vals[1]),
                    3 => Corners::new(vals[0], vals[1], vals[2], vals[1]),
                    4 => Corners::new(vals[0], vals[1], vals[2], vals[3]),
                    _ => Corners::ZERO,
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Corners, A::Error> {
                let mut out = Corners::ZERO;
                while let Some(key) = map.next_key::<String>()? {
                    let val = map.next_value::<Len>()?.get();
                    match key.as_str() {
                        "topLeft" | "top_left" => out.top_left = val,
                        "topRight" | "top_right" => out.top_right = val,
                        "bottomRight" | "bottom_right" => out.bottom_right = val,
                        "bottomLeft" | "bottom_left" => out.bottom_left = val,
                        // An edge name reaches both corners that sit on it.
                        "top" => {
                            out.top_left = val;
                            out.top_right = val;
                        }
                        "bottom" => {
                            out.bottom_left = val;
                            out.bottom_right = val;
                        }
                        "left" => {
                            out.top_left = val;
                            out.bottom_left = val;
                        }
                        "right" => {
                            out.top_right = val;
                            out.bottom_right = val;
                        }
                        "all" => out = Corners::all(val),
                        _ => {}
                    }
                }
                Ok(out)
            }
        }

        d.deserialize_any(CornersVisitor)
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
    fn a_bare_radius_still_rounds_all_four_corners() {
        // Every document written before corners were separable says this.
        assert_eq!(serde_json::from_str::<Corners>("8").unwrap(), Corners::all(8.0));
        assert_eq!(
            serde_json::from_str::<Corners>(r#""5mm""#).unwrap(),
            Corners::all(5.0 * PT_PER_MM)
        );
    }

    #[test]
    fn corners_accept_css_shorthands() {
        // The pair is the two diagonals, as in CSS — not two edges.
        assert_eq!(
            serde_json::from_str::<Corners>("[4, 9]").unwrap(),
            Corners::new(4.0, 9.0, 4.0, 9.0)
        );
        assert_eq!(
            serde_json::from_str::<Corners>("[1, 2, 3, 4]").unwrap(),
            Corners::new(1.0, 2.0, 3.0, 4.0)
        );
        assert_eq!(
            serde_json::from_str::<Corners>(r#"{"topLeft":6,"bottomRight":2}"#).unwrap(),
            Corners::new(6.0, 0.0, 2.0, 0.0)
        );
        // An edge name reaches both corners that sit on it.
        assert_eq!(
            serde_json::from_str::<Corners>(r#"{"top":6}"#).unwrap(),
            Corners::new(6.0, 6.0, 0.0, 0.0)
        );
    }

    #[test]
    fn corners_write_themselves_back_in_the_shortest_form() {
        let json = |c: Corners| serde_json::to_string(&c).unwrap();
        assert_eq!(json(Corners::all(8.0)), "8.0");
        assert_eq!(json(Corners::new(4.0, 9.0, 4.0, 9.0)), "[4.0,9.0]");
        assert_eq!(json(Corners::new(1.0, 2.0, 3.0, 4.0)), "[1.0,2.0,3.0,4.0]");

        // And a round trip lands back where it started.
        let odd = Corners::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(serde_json::from_str::<Corners>(&json(odd)).unwrap(), odd);
    }

    #[test]
    fn corners_too_big_for_the_box_shrink_together() {
        // 30 + 30 on a 40-wide edge: the whole outline scales by 40/60, so the
        // proportion between the corners survives.
        let fitted = Corners::all(30.0).fitted(40.0, 200.0);
        assert!((fitted.top_left - 20.0).abs() < 1e-9);
        assert!(fitted.is_uniform());

        // One oversized corner drags the others down with it, by the same factor.
        let lopsided = Corners::new(80.0, 20.0, 0.0, 0.0).fitted(100.0, 100.0);
        assert!((lopsided.top_left - 80.0).abs() < 1e-9);
        assert!((lopsided.top_right - 20.0).abs() < 1e-9);

        let tight = Corners::new(90.0, 30.0, 0.0, 0.0).fitted(100.0, 100.0);
        assert!((tight.top_left - 75.0).abs() < 1e-9);
        assert!((tight.top_right - 25.0).abs() < 1e-9);

        // What already fits is left alone.
        let roomy = Corners::new(5.0, 6.0, 7.0, 8.0);
        assert_eq!(roomy.fitted(200.0, 200.0), roomy);
    }

    #[test]
    fn a_zero_radius_is_still_a_square_corner() {
        assert!(Corners::ZERO.is_zero());
        assert!(!Corners::new(0.0, 0.0, 0.0, 3.0).is_zero());
        // A negative radius is not a corner cut the other way — it is nothing.
        assert!(Corners::all(-4.0).fitted(50.0, 50.0).is_zero());
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
