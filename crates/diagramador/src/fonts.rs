//! Font registration, face selection and glyph outlines.
//!
//! Fonts are supplied by the host — nothing is embedded in the binary. Each
//! registered face keeps its raw bytes (needed for PDF subsetting) plus metrics
//! normalised to the em square, so scaling a metric is a single multiplication
//! by the font size.
//!
//! Face selection follows the CSS font-matching rules for weight and slant, so
//! `fontWeight: 600` picks the semibold when the family has one and the bold
//! when it does not.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use thiserror::Error;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::spec::style::{FontStyle, FontWeight};

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum FontError {
    #[error("could not parse font `{family}`: {reason}")]
    Parse { family: String, reason: String },
    #[error("no fonts registered")]
    Empty,
    #[error("unknown font id {0}")]
    UnknownId(u32),
}

// ─────────────────────────────────────────────────────────────────────────────
// Metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Face metrics, all expressed as a fraction of the em square.
///
/// Multiply by the font size in points to get points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceMetrics {
    pub units_per_em: f64,
    /// Distance from the baseline to the top of the em box (positive).
    pub ascender: f64,
    /// Distance from the baseline to the bottom (negative).
    pub descender: f64,
    pub line_gap: f64,
    pub cap_height: f64,
    pub x_height: f64,
    /// Centre of the underline, below the baseline (negative).
    pub underline_position: f64,
    pub underline_thickness: f64,
    pub strikeout_position: f64,
    pub strikeout_thickness: f64,
    pub italic_angle: f64,
}

impl FaceMetrics {
    fn from_face(face: &Face<'_>) -> FaceMetrics {
        let upem = face.units_per_em() as f64;
        let n = |v: f64| v / upem;

        let ascender = n(face.ascender() as f64);
        let descender = n(face.descender() as f64);

        let underline = face.underline_metrics();
        let x_height = face.x_height().map_or(ascender * 0.52, |v| n(v as f64));
        let cap_height = face.capital_height().map_or(ascender * 0.7, |v| n(v as f64));

        FaceMetrics {
            units_per_em: upem,
            ascender,
            descender,
            line_gap: n(face.line_gap() as f64),
            cap_height,
            x_height,
            underline_position: underline.map_or(-0.1, |m| n(m.position as f64)),
            underline_thickness: underline.map_or(0.05, |m| n(m.thickness as f64)),
            // OS/2 strikeout is not exposed by ttf-parser; derive it from x-height.
            strikeout_position: x_height * 0.5,
            strikeout_thickness: underline.map_or(0.05, |m| n(m.thickness as f64)),
            italic_angle: face.italic_angle() as f64,
        }
    }

    /// Default baseline-to-baseline distance suggested by the face itself.
    pub fn natural_line_height(&self) -> f64 {
        self.ascender - self.descender + self.line_gap
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FontFace
// ─────────────────────────────────────────────────────────────────────────────

/// Opaque handle to a registered face. Doubles as the index into the display
/// list's font table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId(pub u32);

impl FontId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One registered font face.
#[derive(Debug, Clone)]
pub struct FontFace {
    pub family: String,
    pub weight: FontWeight,
    pub italic: bool,
    pub metrics: FaceMetrics,
    pub post_script_name: String,
    bytes: Arc<Vec<u8>>,
}

impl FontFace {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Reparse the face. Cheap — `ttf-parser` only reads the table directory.
    pub fn ttf(&self) -> Face<'_> {
        // invariant: the bytes parsed successfully at registration time.
        Face::parse(&self.bytes, 0).expect("face was validated on registration")
    }

    /// A shaping face for `rustybuzz`.
    pub fn shaper(&self) -> rustybuzz::Face<'_> {
        rustybuzz::Face::from_face(self.ttf())
    }

    /// Glyph id for a character, if the face covers it.
    pub fn glyph_for(&self, ch: char) -> Option<u16> {
        self.ttf().glyph_index(ch).map(|g| g.0)
    }

    /// Outline of a glyph as an SVG path, in em units with **y growing down**,
    /// relative to the baseline origin.
    ///
    /// The browser paints it with `translate(x, baseline); scale(size, size)`,
    /// which is exactly the transform the PDF emitter applies. Returns `None`
    /// for glyphs with no outline (a space, say).
    pub fn glyph_path(&self, glyph_id: u16) -> Option<String> {
        let face = self.ttf();
        let scale = 1.0 / self.metrics.units_per_em;
        let mut sink = PathSink {
            d: String::new(),
            scale,
        };
        face.outline_glyph(GlyphId(glyph_id), &mut sink)?;
        if sink.d.is_empty() { None } else { Some(sink.d) }
    }

    /// Nominal advance of a glyph in em units, ignoring shaping.
    pub fn advance_of(&self, glyph_id: u16) -> f64 {
        self.ttf()
            .glyph_hor_advance(GlyphId(glyph_id))
            .map_or(0.0, |a| a as f64 / self.metrics.units_per_em)
    }
}

/// Accumulates glyph outlines into an SVG path string, flipping the y axis.
struct PathSink {
    d: String,
    scale: f64,
}

impl PathSink {
    #[inline]
    fn x(&self, v: f32) -> f64 {
        round6(v as f64 * self.scale)
    }
    #[inline]
    fn y(&self, v: f32) -> f64 {
        round6(-(v as f64) * self.scale)
    }
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

impl OutlineBuilder for PathSink {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "M{} {}", self.x(x), self.y(y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "L{} {}", self.x(x), self.y(y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(
            self.d,
            "Q{} {} {} {}",
            self.x(x1),
            self.y(y1),
            self.x(x),
            self.y(y)
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(
            self.d,
            "C{} {} {} {} {} {}",
            self.x(x1),
            self.y(y1),
            self.x(x2),
            self.y(y2),
            self.x(x),
            self.y(y)
        );
    }
    fn close(&mut self) {
        self.d.push('Z');
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Registry
// ─────────────────────────────────────────────────────────────────────────────

/// Every face the host has registered, grouped by family name.
#[derive(Debug, Default)]
pub struct FontRegistry {
    faces: Vec<FontFace>,
    families: BTreeMap<String, Vec<FontId>>,
    default_family: Option<String>,
}

impl FontRegistry {
    pub fn new() -> Self {
        FontRegistry::default()
    }

    /// Register a face under `family`.
    ///
    /// `weight` and `italic` override what the font declares about itself;
    /// pass `None` to trust the font's own OS/2 table. The first family
    /// registered becomes the default.
    pub fn add(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        weight: Option<FontWeight>,
        italic: Option<bool>,
    ) -> Result<FontId, FontError> {
        let face = Face::parse(&bytes, 0).map_err(|e| FontError::Parse {
            family: family.to_string(),
            reason: format!("{e:?}"),
        })?;

        let metrics = FaceMetrics::from_face(&face);
        let declared_weight = FontWeight(face.weight().to_number());
        let declared_italic = face.is_italic() || face.italic_angle() != 0.0;
        let post_script_name = post_script_name(&face)
            .unwrap_or_else(|| format!("{family}-{}", self.faces.len()))
            .replace(' ', "-");

        let id = FontId(self.faces.len() as u32);
        self.faces.push(FontFace {
            family: family.to_string(),
            weight: weight.unwrap_or(declared_weight),
            italic: italic.unwrap_or(declared_italic),
            metrics,
            post_script_name,
            bytes: Arc::new(bytes),
        });

        self.families.entry(family.to_string()).or_default().push(id);
        if self.default_family.is_none() {
            self.default_family = Some(family.to_string());
        }

        Ok(id)
    }

    pub fn face(&self, id: FontId) -> Option<&FontFace> {
        self.faces.get(id.index())
    }

    pub fn faces(&self) -> &[FontFace] {
        &self.faces
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    pub fn family_names(&self) -> impl Iterator<Item = &str> {
        self.families.keys().map(String::as_str)
    }

    pub fn default_family(&self) -> Option<&str> {
        self.default_family.as_deref()
    }

    pub fn set_default_family(&mut self, name: &str) {
        if self.families.contains_key(name) {
            self.default_family = Some(name.to_string());
        }
    }

    pub fn clear(&mut self) {
        self.faces.clear();
        self.families.clear();
        self.default_family = None;
    }

    /// Pick the face that best matches `family` / `weight` / `style`.
    ///
    /// Falls back to the default family, then to any registered family, so a
    /// document that names a font nobody registered still renders.
    pub fn select(
        &self,
        family: Option<&str>,
        weight: FontWeight,
        style: FontStyle,
    ) -> Option<FontId> {
        let candidates = family
            .and_then(|name| self.families.get(name))
            .or_else(|| {
                self.default_family
                    .as_ref()
                    .and_then(|name| self.families.get(name))
            })
            .or_else(|| self.families.values().next())?;

        let want_italic = style.is_italic();

        // Prefer faces with the requested slant; fall back to the other slant.
        let matching_slant: Vec<FontId> = candidates
            .iter()
            .copied()
            .filter(|id| self.faces[id.index()].italic == want_italic)
            .collect();
        let pool: &[FontId] = if matching_slant.is_empty() {
            candidates
        } else {
            &matching_slant
        };

        pool.iter()
            .copied()
            .min_by_key(|id| weight_distance(self.faces[id.index()].weight, weight))
    }
}

/// CSS font-matching distance between an available weight and a desired one.
///
/// Lower is better. The rules: for a desired weight of 400–500 prefer slightly
/// heavier faces first, below 400 prefer lighter ones, above 500 prefer heavier
/// ones. Encoding that as a sort key keeps the selection a single `min_by_key`.
fn weight_distance(available: FontWeight, desired: FontWeight) -> u32 {
    let a = available.0 as i32;
    let d = desired.0 as i32;
    let delta = (a - d).unsigned_abs();

    if a == d {
        return 0;
    }

    // 1 = the preferred direction, 2 = the fallback direction.
    let direction_penalty = if (400..=500).contains(&d) {
        // 400–500 first looks up to 500, then down, then further up.
        if a > d && a <= 500 { 1 } else if a < d { 2 } else { 3 }
    } else if d < 400 {
        if a < d { 1 } else { 2 }
    } else if a > d {
        1
    } else {
        2
    };

    direction_penalty * 1000 + delta
}

fn post_script_name(face: &Face<'_>) -> Option<String> {
    use ttf_parser::name_id;
    face.names()
        .into_iter()
        .filter(|n| n.name_id == name_id::POST_SCRIPT_NAME)
        .find_map(|n| n.to_string())
}

#[cfg(test)]
pub(crate) mod test_fonts {
    use std::sync::OnceLock;

    /// A real face, borrowed from the sibling prova-pdf checkout when present,
    /// so the tests exercise genuine shaping rather than a stub.
    pub fn dejavu() -> Option<&'static [u8]> {
        static CACHE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                let candidates = [
                    "fonts/DejaVuSans.ttf",
                    "../../fonts/DejaVuSans.ttf",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                    "/usr/share/fonts/TTF/DejaVuSans.ttf",
                ];
                candidates
                    .iter()
                    .find_map(|p| std::fs::read(p).ok())
            })
            .as_deref()
    }

    pub fn dejavu_bold() -> Option<&'static [u8]> {
        static CACHE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                let candidates = [
                    "fonts/DejaVuSans-Bold.ttf",
                    "../../fonts/DejaVuSans-Bold.ttf",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
                ];
                candidates
                    .iter()
                    .find_map(|p| std::fs::read(p).ok())
            })
            .as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with_regular() -> Option<(FontRegistry, FontId)> {
        let bytes = test_fonts::dejavu()?;
        let mut reg = FontRegistry::new();
        let id = reg.add("body", bytes.to_vec(), None, None).unwrap();
        Some((reg, id))
    }

    #[test]
    fn rejects_garbage_bytes() {
        let mut reg = FontRegistry::new();
        let err = reg.add("body", vec![0, 1, 2, 3], None, None).unwrap_err();
        assert!(matches!(err, FontError::Parse { .. }));
        assert!(reg.is_empty());
    }

    #[test]
    fn registers_and_reads_metrics() {
        let Some((reg, id)) = registry_with_regular() else {
            return;
        };
        let face = reg.face(id).unwrap();
        assert_eq!(face.family, "body");
        assert!(face.metrics.units_per_em >= 16.0);
        assert!(face.metrics.ascender > 0.0);
        assert!(face.metrics.descender < 0.0);
        assert!(face.metrics.natural_line_height() > 1.0);
        assert!(!face.post_script_name.is_empty());
    }

    #[test]
    fn first_family_becomes_the_default() {
        let Some((reg, _)) = registry_with_regular() else {
            return;
        };
        assert_eq!(reg.default_family(), Some("body"));
    }

    #[test]
    fn selection_falls_back_when_the_family_is_unknown() {
        let Some((reg, id)) = registry_with_regular() else {
            return;
        };
        let picked = reg
            .select(Some("Nonexistent Sans"), FontWeight::NORMAL, FontStyle::Normal)
            .unwrap();
        assert_eq!(picked, id);
    }

    #[test]
    fn selection_prefers_the_matching_weight() {
        let (Some(regular), Some(bold)) = (test_fonts::dejavu(), test_fonts::dejavu_bold()) else {
            return;
        };
        let mut reg = FontRegistry::new();
        let r = reg
            .add("body", regular.to_vec(), Some(FontWeight::NORMAL), Some(false))
            .unwrap();
        let b = reg
            .add("body", bold.to_vec(), Some(FontWeight::BOLD), Some(false))
            .unwrap();

        assert_eq!(
            reg.select(Some("body"), FontWeight::NORMAL, FontStyle::Normal),
            Some(r)
        );
        assert_eq!(
            reg.select(Some("body"), FontWeight::BOLD, FontStyle::Normal),
            Some(b)
        );
        // 600 has no exact face; the bold is closer than the regular.
        assert_eq!(
            reg.select(Some("body"), FontWeight(600), FontStyle::Normal),
            Some(b)
        );
        // Italic is unavailable, so it falls back to the upright of that weight.
        assert_eq!(
            reg.select(Some("body"), FontWeight::BOLD, FontStyle::Italic),
            Some(b)
        );
    }

    #[test]
    fn glyph_path_is_in_em_units_with_y_down() {
        let Some((reg, id)) = registry_with_regular() else {
            return;
        };
        let face = reg.face(id).unwrap();
        let gid = face.glyph_for('H').expect("H is covered");

        let path = face.glyph_path(gid).expect("H has an outline");
        assert!(path.starts_with('M'));

        // Every coordinate must be within a couple of em squares, which only
        // holds if the scale was applied.
        let coords: Vec<f64> = path
            .split(|c: char| c.is_ascii_alphabetic())
            .flat_map(|seg| seg.split_whitespace())
            .filter_map(|t| t.parse::<f64>().ok())
            .collect();
        assert!(!coords.is_empty());
        assert!(coords.iter().all(|v| v.abs() < 2.0), "outline not normalised");

        // Cap height sits above the baseline, so with y down it must be negative.
        assert!(coords.iter().any(|v| *v < -0.3), "no upward extent found");
    }

    #[test]
    fn space_has_no_outline() {
        let Some((reg, id)) = registry_with_regular() else {
            return;
        };
        let face = reg.face(id).unwrap();
        let gid = face.glyph_for(' ').unwrap();
        assert_eq!(face.glyph_path(gid), None);
        assert!(face.advance_of(gid) > 0.0);
    }

    #[test]
    fn weight_distance_prefers_heavier_for_semibold() {
        // Desired 600: 700 (heavier) must beat 400 (lighter).
        assert!(
            weight_distance(FontWeight(700), FontWeight(600))
                < weight_distance(FontWeight(400), FontWeight(600))
        );
        // Desired 300: 200 (lighter) must beat 400 (heavier).
        assert!(
            weight_distance(FontWeight(200), FontWeight(300))
                < weight_distance(FontWeight(400), FontWeight(300))
        );
        // Exact match always wins.
        assert_eq!(weight_distance(FontWeight(400), FontWeight(400)), 0);
    }

    #[test]
    fn clear_empties_the_registry() {
        let Some((mut reg, _)) = registry_with_regular() else {
            return;
        };
        reg.clear();
        assert!(reg.is_empty());
        assert_eq!(reg.default_family(), None);
        assert_eq!(
            reg.select(None, FontWeight::NORMAL, FontStyle::Normal),
            None
        );
    }
}
