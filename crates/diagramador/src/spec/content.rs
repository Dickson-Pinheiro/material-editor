//! Text content: blocks (vertical stacking) and inlines (horizontal flow).
//!
//! This is the raw core — there is no notion of "question", "chapter" or any
//! other domain concept. A heading is a paragraph with a style; a list item is
//! a paragraph with a marker; a fill-in blank is an inline rule.
//!
//! Both `Block` and `Inline` accept a bare string as shorthand, so the simplest
//! possible text frame is `{"type":"text","rect":[…],"blocks":["Olá mundo"]}`.

// A `Paragraph` is large — it carries an optional `Style`, which is twenty-odd
// optional fields. Boxing it would shrink `Block` but add an indirection to the
// hottest loop in layout, and block lists are short: a page holds tens of them,
// not millions. The size is a deliberate trade, not an oversight.
#![allow(clippy::large_enum_variant)]

use serde::{Deserialize, Deserializer, Serialize};

use super::style::Style;
use crate::color::Color;
use crate::units::Len;

// ─────────────────────────────────────────────────────────────────────────────
// Block
// ─────────────────────────────────────────────────────────────────────────────

/// A vertically-stacked unit of content inside a text frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Block {
    Paragraph(Paragraph),
    /// A horizontal line spanning the column.
    Rule(RuleBlock),
    /// Blank vertical space.
    Spacer(SpacerBlock),
    /// Force the remaining content into the next threaded frame, skipping any
    /// columns left in this one.
    FrameBreak,
    /// Force the remaining content into the next column.
    ColumnBreak,
    /// Force the remaining content onto a later page. Frames further along the
    /// chain that sit on the same page are skipped over.
    PageBreak,
}

impl Block {
    /// Shorthand constructor for a plain paragraph of text.
    pub fn text(value: impl Into<String>) -> Block {
        Block::Paragraph(Paragraph::from_text(value))
    }

    pub fn as_paragraph(&self) -> Option<&Paragraph> {
        match self {
            Block::Paragraph(p) => Some(p),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Block {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        enum Tagged {
            Paragraph(Paragraph),
            Rule(RuleBlock),
            Spacer(SpacerBlock),
            FrameBreak,
            ColumnBreak,
            PageBreak,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Shorthand(String),
            Tagged(Tagged),
        }

        Ok(match Repr::deserialize(d)? {
            Repr::Shorthand(text) => Block::text(text),
            Repr::Tagged(Tagged::Paragraph(p)) => Block::Paragraph(p),
            Repr::Tagged(Tagged::Rule(r)) => Block::Rule(r),
            Repr::Tagged(Tagged::Spacer(s)) => Block::Spacer(s),
            Repr::Tagged(Tagged::FrameBreak) => Block::FrameBreak,
            Repr::Tagged(Tagged::ColumnBreak) => Block::ColumnBreak,
            Repr::Tagged(Tagged::PageBreak) => Block::PageBreak,
        })
    }
}

/// A run of inline content laid out as a sequence of lines.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Paragraph {
    /// Stable identity, echoed into the display list so the editor can map a
    /// painted glyph back to its source.
    pub id: Option<String>,
    /// Name of a style from `resources.styles`.
    #[serde(rename = "use")]
    pub use_style: Option<String>,
    pub style: Option<Style>,
    /// Bullet, number or label placed before the first line.
    pub marker: Option<Marker>,
    pub content: Vec<Inline>,

    /// Where this paragraph's text lives in the source document.
    ///
    /// Set by the engine when a paragraph is split across threaded frames: the
    /// continuation carries the original block index, the index of the inline
    /// it resumes from, and the byte offset within it. Never serialised — it
    /// is bookkeeping, not part of the schema.
    #[serde(skip)]
    pub origin: Option<Origin>,
}

/// Coordinates of a continued paragraph inside the document that owns it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Origin {
    pub block: u32,
    /// Index of the first inline of this fragment in the original content.
    pub inline: u32,
    /// Byte offset into that first inline where this fragment starts.
    pub offset: u32,
}

impl Paragraph {
    pub fn from_text(value: impl Into<String>) -> Paragraph {
        Paragraph {
            content: vec![Inline::text(value)],
            ..Default::default()
        }
    }

    /// Concatenated plain text of every text run, with transforms not applied.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for inline in &self.content {
            match inline {
                Inline::Text(run) => out.push_str(&run.text),
                Inline::Break => out.push('\n'),
                Inline::Tab(_) => out.push('\t'),
                _ => {}
            }
        }
        out
    }
}

/// A label placed before a paragraph — bullet, number, letter, anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Marker {
    pub text: String,
    pub style: Option<Style>,
    /// Gap between the marker and the text. Ignored when `width` is set.
    pub gap: Option<Len>,
    /// Fixed width of the marker column.
    pub width: Option<Len>,
    /// When true, wrapped lines align with the text, not with the marker.
    pub hanging: bool,
}

impl Default for Marker {
    fn default() -> Self {
        Marker {
            text: String::new(),
            style: None,
            gap: None,
            width: None,
            hanging: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RuleBlock {
    pub thickness: Option<Len>,
    pub color: Option<Color>,
    /// Fraction of the column width, 0–1. Defaults to the full column.
    pub width: Option<f64>,
    pub style: Option<Style>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpacerBlock {
    pub height: Len,
}

// ─────────────────────────────────────────────────────────────────────────────
// Inline
// ─────────────────────────────────────────────────────────────────────────────

/// A horizontally-flowed piece of content inside a paragraph.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Inline {
    Text(TextRun),
    /// Forced line break; the paragraph continues on the next line.
    Break,
    Tab(Tab),
    /// Fixed-width whitespace that never collapses and never breaks.
    Space(SpaceRun),
    Image(InlineImage),
    /// A drawn line sitting on the baseline — the fill-in-the-blank primitive.
    Rule(InlineRule),
}

impl Inline {
    pub fn text(value: impl Into<String>) -> Inline {
        Inline::Text(TextRun {
            text: value.into(),
            ..Default::default()
        })
    }

    pub fn styled(value: impl Into<String>, style: Style) -> Inline {
        Inline::Text(TextRun {
            text: value.into(),
            style: Some(style),
            ..Default::default()
        })
    }
}

impl<'de> Deserialize<'de> for Inline {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        enum Tagged {
            Text(TextRun),
            Break,
            Tab(Tab),
            Space(SpaceRun),
            Image(InlineImage),
            Rule(InlineRule),
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Shorthand(String),
            Tagged(Tagged),
        }

        Ok(match Repr::deserialize(d)? {
            Repr::Shorthand(text) => Inline::text(text),
            Repr::Tagged(Tagged::Text(t)) => Inline::Text(t),
            Repr::Tagged(Tagged::Break) => Inline::Break,
            Repr::Tagged(Tagged::Tab(t)) => Inline::Tab(t),
            Repr::Tagged(Tagged::Space(s)) => Inline::Space(s),
            Repr::Tagged(Tagged::Image(i)) => Inline::Image(i),
            Repr::Tagged(Tagged::Rule(r)) => Inline::Rule(r),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextRun {
    pub text: String,
    /// Name of a style from `resources.styles`.
    #[serde(rename = "use")]
    pub use_style: Option<String>,
    pub style: Option<Style>,
    /// Stable identity for editor round-tripping.
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tab {
    /// Absolute x position within the column to advance to. When absent, the
    /// next multiple of `defaultStop` is used.
    pub to: Option<Len>,
    pub leader: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpaceRun {
    pub width: Len,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InlineImage {
    /// Key registered through `add_image`.
    pub src: String,
    pub width: Option<Len>,
    pub height: Option<Len>,
    /// Offset of the image bottom from the text baseline.
    pub baseline: Option<Len>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InlineRule {
    /// Absent means "fill the rest of the line".
    pub width: Option<Len>,
    pub thickness: Option<Len>,
    pub color: Option<Color>,
    /// Distance below the baseline. Defaults to the font's underline position.
    pub offset: Option<Len>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_string_shorthand_becomes_a_paragraph() {
        let b: Block = serde_json::from_str(r#""Olá mundo""#).unwrap();
        let p = b.as_paragraph().expect("paragraph");
        assert_eq!(p.plain_text(), "Olá mundo");
    }

    #[test]
    fn inline_string_shorthand_becomes_a_text_run() {
        let i: Inline = serde_json::from_str(r#""texto""#).unwrap();
        assert_eq!(i, Inline::text("texto"));
    }

    #[test]
    fn tagged_paragraph_with_mixed_inlines() {
        let json = r#"{
            "type": "paragraph",
            "content": [
                "As plantas ",
                {"type": "text", "text": "convertem", "style": {"fontWeight": "bold"}},
                " luz",
                {"type": "break"},
                {"type": "rule", "width": "3cm"}
            ]
        }"#;
        let b: Block = serde_json::from_str(json).unwrap();
        let p = b.as_paragraph().unwrap();
        assert_eq!(p.content.len(), 5);
        assert!(matches!(p.content[3], Inline::Break));
        assert!(matches!(p.content[4], Inline::Rule(_)));
        assert_eq!(p.plain_text(), "As plantas convertem luz\n");
    }

    #[test]
    fn unit_variants_deserialize_from_tag_alone() {
        let b: Block = serde_json::from_str(r#"{"type":"frameBreak"}"#).unwrap();
        assert_eq!(b, Block::FrameBreak);
        let i: Inline = serde_json::from_str(r#"{"type":"break"}"#).unwrap();
        assert_eq!(i, Inline::Break);
    }

    #[test]
    fn marker_defaults_to_hanging() {
        let m: Marker = serde_json::from_str(r#"{"text":"a)"}"#).unwrap();
        assert!(m.hanging);
        assert_eq!(m.text, "a)");
    }

    #[test]
    fn blocks_roundtrip_through_json() {
        let original = vec![
            Block::text("primeiro"),
            Block::Spacer(SpacerBlock { height: Len(12.0) }),
            Block::ColumnBreak,
        ];
        let json = serde_json::to_string(&original).unwrap();
        let back: Vec<Block> = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn named_style_reference_parses() {
        let b: Block =
            serde_json::from_str(r#"{"type":"paragraph","use":"h1","content":["Título"]}"#).unwrap();
        assert_eq!(b.as_paragraph().unwrap().use_style.as_deref(), Some("h1"));
    }
}
