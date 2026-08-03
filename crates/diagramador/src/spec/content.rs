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
use crate::units::{Insets, Len};

// ─────────────────────────────────────────────────────────────────────────────
// Tables
// ─────────────────────────────────────────────────────────────────────────────

/// What a column or row asks for, as the document writes it.
///
/// `"auto"` takes what the content needs; a bare number or a length like
/// `"20mm"` is fixed; `"1fr"` takes a share of what is left; `"25%"` takes a
/// share of the whole. The fraction is what lets an author say "this column
/// gets the rest" without knowing how wide the page is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum TrackSize {
    #[default]
    Auto,
    Fixed(Len),
    /// A share of the container, `0..1`.
    Relative(f64),
    /// A share of the leftover, in `fr` units.
    Fraction(f64),
}

impl Serialize for TrackSize {
    /// Written back in the same form it is read: `"auto"`, a number of
    /// points, `"1fr"`, `"25%"`. Deriving this would emit serde's own tagged
    /// shape, which the deserialiser above does not accept — a document could
    /// then be saved and not reopened.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            TrackSize::Auto => s.serialize_str("auto"),
            TrackSize::Fixed(length) => s.serialize_f64(length.0),
            TrackSize::Relative(share) => s.serialize_str(&format!("{}%", share * 100.0)),
            TrackSize::Fraction(share) => s.serialize_str(&format!("{share}fr")),
        }
    }
}

impl<'de> Deserialize<'de> for TrackSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct TrackVisitor;

        impl<'de> Visitor<'de> for TrackVisitor {
            type Value = TrackSize;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("\"auto\", a length, \"1fr\" or \"25%\"")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<TrackSize, E> {
                Ok(TrackSize::Fixed(Len(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<TrackSize, E> {
                Ok(TrackSize::Fixed(Len(v as f64)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<TrackSize, E> {
                Ok(TrackSize::Fixed(Len(v as f64)))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<TrackSize, E> {
                let text = v.trim();
                if text.eq_ignore_ascii_case("auto") {
                    return Ok(TrackSize::Auto);
                }
                if let Some(number) = text.strip_suffix("fr") {
                    return number
                        .trim()
                        .parse::<f64>()
                        .map(TrackSize::Fraction)
                        .map_err(|_| E::custom(format!("fracção inválida: {v}")));
                }
                if let Some(number) = text.strip_suffix('%') {
                    return number
                        .trim()
                        .parse::<f64>()
                        .map(|percent| TrackSize::Relative(percent / 100.0))
                        .map_err(|_| E::custom(format!("percentagem inválida: {v}")));
                }
                crate::units::parse_len(text)
                    .map(TrackSize::Fixed)
                    .map_err(E::custom)
            }
        }

        d.deserialize_any(TrackVisitor)
    }
}

/// A grid of cells.
///
/// A block rather than a frame, because a table has to flow with the text
/// around it: it follows the paragraph before it, and when the page runs out
/// it continues on the next one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TableBlock {
    /// One entry per column. An empty list means the columns are inferred
    /// from the widest row, all of them automatic.
    pub columns: Vec<TrackSize>,
    /// One entry per row; missing entries are automatic.
    pub rows: Vec<TrackSize>,
    pub cells: Vec<Cell>,

    /// Rows repeated when the table continues on another page.
    pub header: Option<RepeatRows>,
    /// Rows repeated at the foot of every page the table continues past.
    pub footer: Option<RepeatRows>,

    /// Padding inside every cell, unless the cell says otherwise.
    pub inset: Insets,
    pub column_gap: Len,
    pub row_gap: Len,

    /// Rules drawn between tracks, independent of any cell.
    ///
    /// A rule under the heading is one declaration here, not a border
    /// repeated in eight cells — the `booktabs` model, and the reason its
    /// tables read.
    pub lines: Vec<GridLine>,
    /// Alternating row fills, so striping is not written into every row.
    pub stripe: Option<Stripe>,

    pub fill: Option<Color>,
    #[serde(rename = "use")]
    pub use_style: Option<String>,
    pub style: Option<Style>,

    /// Where this table sits in the block list the author wrote, when this is
    /// the continuation of one. Same mechanism as `Cell::origin`, one level
    /// up: the leftover is re-flowed into a fresh list whose indices start
    /// again, and the editor has to be told which table it came from.
    #[serde(skip)]
    pub origin: Option<u32>,
}

/// Rows that repeat when the table breaks across pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepeatRows {
    /// How many rows from the top — or, for a footer, from the bottom.
    pub rows: u32,
    /// Off when the rows should appear once and not again.
    pub repeat: bool,
    /// Shown in place of `rows` on the pages where the table is *continuing*
    /// — not on the page it begins, for a header, nor the page it ends on,
    /// for a footer.
    ///
    /// Named for the continuation rather than for the exception, which is what
    /// makes one field serve both ends. `rows` are real rows of the table and
    /// appear where they were written: a header at the top of the first page,
    /// a total at the foot of the last. This is the other thing — the smaller
    /// heading a reader meets halfway down a table, the "(continua)" under a
    /// page that has not finished. Called `first` it would have meant the
    /// opposite thing at each end of the table.
    pub continued: Option<Vec<Cell>>,
}

/// One cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Cell {
    /// Explicit column. Absent means the next free slot, filling row by row.
    pub x: Option<u32>,
    /// Explicit row.
    pub y: Option<u32>,
    pub colspan: u32,
    pub rowspan: u32,

    /// A cell holds blocks, not a string: two paragraphs and a rule inside one
    /// cell is ordinary in teaching material, and this way the whole of
    /// `flow_blocks` is reused rather than a second text path invented.
    pub blocks: Vec<Block>,

    pub vertical_align: CellAlign,
    pub fill: Option<Color>,
    pub inset: Option<Insets>,
    #[serde(rename = "use")]
    pub use_style: Option<String>,
    pub style: Option<Style>,

    /// Where this cell sits in the table the author wrote, when this is a
    /// copy of it.
    ///
    /// Set when a table runs onto another page: the continuation is a new
    /// block whose rows are renumbered and whose header is copied in, so an
    /// index into it addresses a cell the document does not have. Exactly
    /// what `Paragraph::origin` does for a paragraph split across frames, and
    /// for the same reason — what the editor writes back to is the original,
    /// wherever the copy was drawn.
    #[serde(skip)]
    pub origin: Option<u32>,
}

impl Default for RepeatRows {
    fn default() -> Self {
        // Repeating is the point of declaring a header at all.
        RepeatRows { rows: 1, repeat: true, continued: None }
    }
}

impl Default for Cell {
    fn default() -> Self {
        // A cell covers one column and one row. Deriving `Default` would give
        // zero, and a cell that covers nothing is not a cell.
        Cell {
            x: None,
            y: None,
            colspan: 1,
            rowspan: 1,
            blocks: Vec::new(),
            vertical_align: CellAlign::Top,
            fill: None,
            inset: None,
            use_style: None,
            style: None,
            origin: None,
        }
    }
}

impl Default for Stripe {
    fn default() -> Self {
        // Every other row, starting with the second: the first is usually the
        // heading, and striping it would fight the heading's own fill.
        Stripe { every: 2, offset: 1, fill: None }
    }
}

/// A rule drawn along a grid line, independent of the cells.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GridLine {
    /// `horizontal` runs along a row boundary, `vertical` along a column one.
    pub axis: GridAxis,
    /// Which boundary: 0 is before the first track, `n` is after the last.
    pub at: u32,
    /// Track index the rule starts at; absent means the first.
    pub from: Option<u32>,
    /// Track index the rule stops before; absent means past the last.
    pub to: Option<u32>,
    pub width: Len,
    pub color: Option<Color>,
}

/// Where a cell's content sits in the space its row gives it.
///
/// A vocabulary of its own rather than the frame's `VerticalAlign`: a frame
/// can justify its paragraphs and has nothing to align a baseline *with*,
/// while a cell is the other way round. Sharing one enum would mean two
/// values that are meaningless wherever they are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CellAlign {
    #[default]
    Top,
    Middle,
    Bottom,
    /// The cell's first line sits on the same baseline as the other cells in
    /// its row that ask for the same. The CSS rule, and the one that makes a
    /// table of headings and numbers read straight when the type sizes differ.
    Baseline,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridAxis {
    #[default]
    Horizontal,
    Vertical,
}

/// Alternating row fills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stripe {
    /// Fill one row in every `every`.
    pub every: u32,
    /// Which row of each group is filled.
    pub offset: u32,
    pub fill: Option<Color>,
}

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
    /// A grid of cells, sized from its own content.
    Table(TableBlock),
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
            Table(TableBlock),
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
            Repr::Tagged(Tagged::Table(t)) => Block::Table(t),
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
    fn a_track_size_reads_every_form_an_author_would_write() {
        let parse = |json: &str| serde_json::from_str::<TrackSize>(json).unwrap();
        assert_eq!(parse(r#""auto""#), TrackSize::Auto);
        assert_eq!(parse("120"), TrackSize::Fixed(Len(120.0)));
        assert_eq!(parse(r#""20mm""#), TrackSize::Fixed(Len(20.0 * 72.0 / 25.4)));
        assert_eq!(parse(r#""1fr""#), TrackSize::Fraction(1.0));
        assert_eq!(parse(r#""2.5fr""#), TrackSize::Fraction(2.5));
        assert_eq!(parse(r#""25%""#), TrackSize::Relative(0.25));
        assert!(serde_json::from_str::<TrackSize>(r#""banana""#).is_err());
    }

    #[test]
    fn a_table_parses_from_the_shape_an_author_would_write() {
        let json = r##"{
            "type": "table",
            "columns": ["auto", "1fr", 80],
            "inset": [6, 8],
            "header": { "rows": 1 },
            "lines": [{ "axis": "horizontal", "at": 1, "width": 1 }],
            "stripe": { "fill": "#eef4fb" },
            "cells": [
                { "blocks": ["Estado"] },
                { "blocks": ["Mudança"] },
                { "blocks": ["Onde"] },
                { "colspan": 2, "blocks": ["Sólido para líquido"] },
                { "blocks": ["No degelo"] }
            ]
        }"##;
        let block: Block = serde_json::from_str(json).unwrap();
        let Block::Table(table) = block else { panic!("não é tabela") };

        assert_eq!(table.columns.len(), 3);
        assert_eq!(table.columns[1], TrackSize::Fraction(1.0));
        assert_eq!(table.cells.len(), 5);
        assert_eq!(table.cells[3].colspan, 2);
        assert_eq!(table.cells[0].colspan, 1, "sem declarar, um span é um");
        assert_eq!(table.cells[0].rowspan, 1);
        assert!(table.header.as_ref().unwrap().repeat, "um cabeçalho repete por omissão");
        assert_eq!(table.stripe.as_ref().unwrap().every, 2, "zebra de duas em duas");
        assert_eq!(table.lines.len(), 1);
        assert_eq!(table.lines[0].axis, GridAxis::Horizontal);
    }

    #[test]
    fn a_cell_holds_blocks_not_just_a_string() {
        let json = r#"{
            "type": "table",
            "cells": [{ "blocks": ["Um parágrafo", { "type": "spacer", "height": 4 }, "Outro"] }]
        }"#;
        let Block::Table(table) = serde_json::from_str::<Block>(json).unwrap() else {
            panic!("não é tabela")
        };
        assert_eq!(table.cells[0].blocks.len(), 3, "dois parágrafos e um espaçador");
        assert!(matches!(table.cells[0].blocks[1], Block::Spacer(_)));
    }

    #[test]
    fn a_table_survives_the_round_trip() {
        let json = r#"{
            "type": "table",
            "columns": ["auto", "1fr"],
            "cells": [{ "x": 0, "y": 0, "rowspan": 2, "blocks": ["a"] }, { "blocks": ["b"] }]
        }"#;
        let first: Block = serde_json::from_str(json).unwrap();
        let again: Block = serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap();
        assert_eq!(first, again);
    }


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
