//! Text shaping.
//!
//! One thin wrapper over `rustybuzz`, converting its font-unit output into
//! points once, at the boundary. Everything downstream — line breaking, the
//! display list, the PDF emitter — works in points, so there is exactly one
//! place where a scale factor can be wrong.

use crate::fonts::FontFace;

/// A glyph as the shaper produced it, already scaled to points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    pub id: u16,
    /// Byte offset into the shaped string of the character this glyph renders.
    pub cluster: u32,
    pub x_advance: f64,
    pub x_offset: f64,
    /// Positive downward, matching the display list's axis.
    pub y_offset: f64,
}

/// Shape `text` with `face` at `font_size`.
///
/// `letter_spacing` is added after every glyph and `word_spacing` after every
/// glyph that renders an ASCII space — the same rule CSS applies.
pub fn shape_text(
    face: &FontFace,
    text: &str,
    font_size: f64,
    letter_spacing: f64,
    word_spacing: f64,
) -> Vec<ShapedGlyph> {
    if text.is_empty() {
        return Vec::new();
    }

    let hb_face = face.shaper();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(rustybuzz::Direction::LeftToRight);

    let output = rustybuzz::shape(&hb_face, &[], buffer);
    let scale = font_size / face.metrics.units_per_em;
    let bytes = text.as_bytes();

    output
        .glyph_infos()
        .iter()
        .zip(output.glyph_positions())
        .map(|(info, pos)| {
            let cluster = info.cluster;
            let is_space = bytes.get(cluster as usize) == Some(&b' ');
            ShapedGlyph {
                id: info.glyph_id as u16,
                cluster,
                x_advance: pos.x_advance as f64 * scale
                    + letter_spacing
                    + if is_space { word_spacing } else { 0.0 },
                x_offset: pos.x_offset as f64 * scale,
                // HarfBuzz measures y upward; the display list measures it down.
                y_offset: -(pos.y_offset as f64) * scale,
            }
        })
        .collect()
}

/// Total advance of a shaped run.
pub fn run_width(glyphs: &[ShapedGlyph]) -> f64 {
    glyphs.iter().map(|g| g.x_advance).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{FontRegistry, test_fonts};

    fn face() -> Option<(FontRegistry, crate::fonts::FontId)> {
        let bytes = test_fonts::dejavu()?;
        let mut reg = FontRegistry::new();
        let id = reg.add("body", bytes.to_vec(), None, None).ok()?;
        Some((reg, id))
    }

    #[test]
    fn empty_text_shapes_to_nothing() {
        let Some((reg, id)) = face() else { return };
        assert!(shape_text(reg.face(id).unwrap(), "", 12.0, 0.0, 0.0).is_empty());
    }

    #[test]
    fn one_glyph_per_ascii_character() {
        let Some((reg, id)) = face() else { return };
        let glyphs = shape_text(reg.face(id).unwrap(), "Olá", 12.0, 0.0, 0.0);
        assert_eq!(glyphs.len(), 3);
        // Clusters are byte offsets, so the accented character jumps by 2.
        assert_eq!(glyphs[0].cluster, 0);
        assert_eq!(glyphs[1].cluster, 1);
        assert_eq!(glyphs[2].cluster, 2);
        assert!(glyphs.iter().all(|g| g.id != 0), "missing glyph coverage");
    }

    #[test]
    fn advances_scale_linearly_with_the_font_size() {
        let Some((reg, id)) = face() else { return };
        let f = reg.face(id).unwrap();
        let small = run_width(&shape_text(f, "Material", 10.0, 0.0, 0.0));
        let large = run_width(&shape_text(f, "Material", 20.0, 0.0, 0.0));
        assert!((large - small * 2.0).abs() < 1e-9);
    }

    #[test]
    fn letter_spacing_adds_once_per_glyph() {
        let Some((reg, id)) = face() else { return };
        let f = reg.face(id).unwrap();
        let plain = run_width(&shape_text(f, "abcd", 12.0, 0.0, 0.0));
        let spaced = run_width(&shape_text(f, "abcd", 12.0, 2.0, 0.0));
        assert!((spaced - plain - 8.0).abs() < 1e-9);
    }

    #[test]
    fn word_spacing_applies_only_to_spaces() {
        let Some((reg, id)) = face() else { return };
        let f = reg.face(id).unwrap();
        let plain = run_width(&shape_text(f, "a b c", 12.0, 0.0, 0.0));
        let spaced = run_width(&shape_text(f, "a b c", 12.0, 0.0, 3.0));
        assert!((spaced - plain - 6.0).abs() < 1e-9, "expected 2 spaces widened");
    }

    #[test]
    fn wider_text_produces_a_wider_run() {
        let Some((reg, id)) = face() else { return };
        let f = reg.face(id).unwrap();
        assert!(
            run_width(&shape_text(f, "mmmm", 12.0, 0.0, 0.0))
                > run_width(&shape_text(f, "iiii", 12.0, 0.0, 0.0))
        );
    }
}
