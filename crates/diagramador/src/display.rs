//! The display list — the engine's single output.
//!
//! Layout ends here and both back-ends start here. `pdf::emit` walks this tree
//! to write a PDF; the browser walks the very same tree to paint a canvas.
//! Neither one re-decides anything: every position, every glyph, every advance
//! is already fixed. That is the whole parity argument.
//!
//! # Coordinates
//!
//! Points, origin at the top-left of the page, `y` growing **down** — the same
//! convention as the canvas. The PDF emitter flips the axis on its way out.
//!
//! # Provenance
//!
//! Painted things carry a [`SourceRef`] back to the JSON that produced them,
//! and each glyph carries the byte offset of the character it came from. That
//! is what lets the editor turn a click at (x, y) into a caret position in the
//! source document.

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::units::Rect;

/// Bumped when the display list shape changes in a way JS must know about.
pub const DISPLAY_VERSION: u32 = 1;

// ─────────────────────────────────────────────────────────────────────────────
// Root
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DisplayList {
    pub version: u32,
    /// Font table. `GlyphRun::font` indexes into this.
    pub fonts: Vec<DisplayFont>,
    pub pages: Vec<DisplayPage>,
    /// Non-fatal problems: overset text, missing images, unknown styles.
    pub diagnostics: Vec<Diagnostic>,
}

impl DisplayList {
    pub fn new() -> Self {
        DisplayList {
            version: DISPLAY_VERSION,
            ..Default::default()
        }
    }

    /// Total number of painted items, groups included, across every page.
    pub fn item_count(&self) -> usize {
        fn count(items: &[DisplayItem]) -> usize {
            items
                .iter()
                .map(|item| match item {
                    DisplayItem::Group(g) => 1 + count(&g.items),
                    _ => 1,
                })
                .sum()
        }
        self.pages.iter().map(|p| count(&p.items)).sum()
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayFont {
    /// Matches [`crate::fonts::FontId`].
    pub id: u32,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub post_script_name: String,
    pub units_per_em: f64,
    /// Em-relative, positive.
    pub ascender: f64,
    /// Em-relative, negative.
    pub descender: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DisplayPage {
    pub index: u32,
    pub id: Option<String>,
    pub width: f64,
    pub height: f64,
    pub background: Option<Color>,
    /// The margin box, so the editor can draw guides.
    pub margin_box: Rect,
    /// Flat index of every frame on the page, for hit-testing and handles.
    pub frames: Vec<DisplayFrame>,
    /// The paint tree, in back-to-front order.
    pub items: Vec<DisplayItem>,
}

/// A frame as the editor sees it: a selectable, draggable box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayFrame {
    pub id: String,
    pub name: Option<String>,
    /// In page coordinates, after any group transforms have been applied.
    pub rect: Rect,
    pub rotation: f64,
    /// `"text"`, `"image"`, `"shape"` or `"group"`.
    pub kind: String,
    pub locked: bool,
    /// Content that did not fit. InDesign's red overset marker.
    pub overset: bool,
    /// Ancestor frame ids, outermost first. Empty for top-level frames.
    pub ancestors: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Items
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DisplayItem {
    /// A nested coordinate space: transform, clip and opacity applied together.
    Group(DisplayGroup),
    Glyphs(GlyphRun),
    Rect(RectItem),
    Ellipse(EllipseItem),
    Line(LineItem),
    Image(ImageItem),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DisplayGroup {
    pub source: Option<SourceRef>,
    /// Affine matrix `[a, b, c, d, e, f]`, applied before painting children.
    pub transform: Option<[f64; 6]>,
    pub clip: Option<ClipShape>,
    pub opacity: f64,
    pub items: Vec<DisplayItem>,
}

impl DisplayGroup {
    pub fn new() -> Self {
        DisplayGroup {
            opacity: 1.0,
            ..Default::default()
        }
    }

    /// True when the group changes nothing and can be flattened away.
    pub fn is_pass_through(&self) -> bool {
        self.transform.is_none() && self.clip.is_none() && self.opacity >= 1.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipShape {
    pub rect: Rect,
    pub radius: f64,
}

/// A sequence of positioned glyphs sharing one font, size and colour.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GlyphRun {
    /// Index into [`DisplayList::fonts`].
    pub font: u32,
    pub size: f64,
    pub fill: Color,
    /// Origin of the run's baseline.
    pub x: f64,
    pub y: f64,
    /// Sum of the glyph advances.
    pub width: f64,
    pub glyphs: Vec<Glyph>,
    /// The characters this run rendered, so the editor and the PDF's
    /// `ToUnicode` map agree on what the glyphs mean.
    pub text: String,
    pub source: Option<SourceRef>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Glyph {
    /// Glyph id in the original (un-subsetted) face.
    pub id: u16,
    /// Offset from the run origin along the baseline, in points.
    pub x: f64,
    /// Offset from the baseline, positive downward.
    pub y: f64,
    /// How far the pen moves after this glyph, in points.
    pub advance: f64,
    /// Byte offset into [`GlyphRun::text`] of the character this glyph renders.
    pub cluster: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RectItem {
    pub rect: Rect,
    pub radius: f64,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub source: Option<SourceRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EllipseItem {
    pub rect: Rect,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub source: Option<SourceRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LineItem {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub stroke: Stroke,
    pub source: Option<SourceRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageItem {
    /// Key registered through `add_image`.
    pub src: String,
    pub rect: Rect,
    /// Portion of the source image to draw, in 0–1 units. `None` = all of it.
    pub crop: Option<Rect>,
    pub source: Option<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
    /// `[on, off]` in points. `None` = solid.
    pub dash: Option<[f64; 2]>,
}

impl Default for Stroke {
    fn default() -> Self {
        Stroke {
            color: Color::BLACK,
            width: 1.0,
            dash: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Provenance
// ─────────────────────────────────────────────────────────────────────────────

/// Where a painted item came from in the source document.
///
/// The indices always address the **origin** of the content, not wherever it
/// happened to land. Text that overflowed frame A into frame B still reports
/// A's block and inline indices, with `offset` advanced accordingly — so the
/// editor writes back to the right place no matter which frame was clicked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceRef {
    pub page: u32,
    /// Id of the frame that painted this, auto-assigned when the JSON omits it.
    pub frame: String,
    /// Name of the story the content lives in, when it came from one. The
    /// editor edits `resources.stories[story]` instead of the frame's blocks.
    pub story: Option<String>,
    /// Index of the block in its owning list.
    pub block: Option<u32>,
    /// Index of the inline within that block's content.
    pub inline: Option<u32>,
    /// Byte offset into that inline's text where this run starts.
    pub offset: Option<u32>,
}

impl SourceRef {
    pub fn frame(page: u32, frame: impl Into<String>) -> Self {
        SourceRef {
            page,
            frame: frame.into(),
            ..Default::default()
        }
    }

    pub fn at(mut self, block: u32, inline: u32, offset: u32) -> Self {
        self.block = Some(block);
        self.inline = Some(inline);
        self.offset = Some(offset);
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Diagnostics
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub page: Option<u32>,
    pub frame: Option<String>,
}

impl Diagnostic {
    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            code: code.to_string(),
            message: message.into(),
            page: None,
            frame: None,
        }
    }

    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            ..Diagnostic::warning(code, message)
        }
    }

    pub fn on(mut self, page: u32, frame: impl Into<String>) -> Self {
        self.page = Some(page);
        self.frame = Some(frame.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_run() -> GlyphRun {
        GlyphRun {
            font: 0,
            size: 12.0,
            fill: Color::BLACK,
            x: 56.0,
            y: 70.0,
            width: 20.0,
            text: "Oi".into(),
            glyphs: vec![
                Glyph { id: 50, x: 0.0, y: 0.0, advance: 8.0, cluster: 0 },
                Glyph { id: 51, x: 8.0, y: 0.0, advance: 12.0, cluster: 1 },
            ],
            source: Some(SourceRef::frame(0, "f1").at(0, 0, 0)),
        }
    }

    #[test]
    fn display_list_roundtrips_through_json() {
        let mut list = DisplayList::new();
        list.pages.push(DisplayPage {
            index: 0,
            width: 595.28,
            height: 841.89,
            items: vec![DisplayItem::Glyphs(sample_run())],
            ..Default::default()
        });

        let json = serde_json::to_string(&list).unwrap();
        let back: DisplayList = serde_json::from_str(&json).unwrap();
        assert_eq!(list, back);
    }

    #[test]
    fn items_serialise_with_a_type_tag() {
        let json = serde_json::to_string(&DisplayItem::Glyphs(sample_run())).unwrap();
        assert!(json.contains(r#""type":"glyphs""#), "{json}");

        let json = serde_json::to_string(&DisplayItem::Rect(RectItem::default())).unwrap();
        assert!(json.contains(r#""type":"rect""#), "{json}");
    }

    #[test]
    fn glyph_x_is_cumulative_so_hit_testing_is_a_scan() {
        let run = sample_run();
        // The caret between the two characters sits at the second glyph's x.
        assert_eq!(run.glyphs[1].x, run.glyphs[0].advance);
        assert_eq!(run.width, run.glyphs.last().unwrap().x + 12.0);
    }

    #[test]
    fn item_count_walks_into_groups() {
        let mut list = DisplayList::new();
        list.pages.push(DisplayPage {
            items: vec![
                DisplayItem::Rect(RectItem::default()),
                DisplayItem::Group(DisplayGroup {
                    items: vec![
                        DisplayItem::Glyphs(sample_run()),
                        DisplayItem::Line(LineItem::default()),
                    ],
                    ..DisplayGroup::new()
                }),
            ],
            ..Default::default()
        });
        // rect + group + 2 children
        assert_eq!(list.item_count(), 4);
    }

    #[test]
    fn pass_through_groups_are_detectable() {
        assert!(DisplayGroup::new().is_pass_through());
        assert!(!DisplayGroup { opacity: 0.5, ..DisplayGroup::new() }.is_pass_through());
        assert!(
            !DisplayGroup {
                clip: Some(ClipShape::default()),
                ..DisplayGroup::new()
            }
            .is_pass_through()
        );
    }

    #[test]
    fn diagnostics_carry_severity_and_location() {
        let d = Diagnostic::warning("overset", "texto não coube").on(2, "f7");
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.page, Some(2));
        assert_eq!(d.frame.as_deref(), Some("f7"));

        let mut list = DisplayList::new();
        assert!(!list.has_errors());
        list.diagnostics.push(Diagnostic::error("noFont", "sem fontes"));
        assert!(list.has_errors());
    }

    #[test]
    fn source_ref_survives_serialisation() {
        let src = SourceRef::frame(3, "frame-a").at(1, 2, 17);
        let back: SourceRef = serde_json::from_str(&serde_json::to_string(&src).unwrap()).unwrap();
        assert_eq!(back, src);
        assert_eq!(back.offset, Some(17));
    }
}
