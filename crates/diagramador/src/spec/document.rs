//! The document root.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::content::Block;
use super::frame::Frame;
use super::style::Style;
use crate::color::Color;
use crate::units::{Insets, PageSize, Rect};

/// Current schema version. Bumped on breaking changes to the JSON shape.
pub const SCHEMA_VERSION: u32 = 1;

// ─────────────────────────────────────────────────────────────────────────────
// Document
// ─────────────────────────────────────────────────────────────────────────────

/// A whole document: the single input to both the PDF emitter and the browser
/// painter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Document {
    pub version: u32,
    pub meta: Meta,
    /// Geometry applied to pages that do not override it.
    pub page: PageDefaults,
    /// Root of the style cascade.
    pub style: Style,
    pub resources: Resources,
    pub pages: Vec<Page>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            version: SCHEMA_VERSION,
            meta: Meta::default(),
            page: PageDefaults::default(),
            style: Style::default(),
            resources: Resources::default(),
            pages: Vec::new(),
        }
    }
}

impl Document {
    /// Resolved geometry of a page, falling back to the document defaults and
    /// then to the page's master.
    ///
    /// `index` is the page's position in the document, needed to mirror margins
    /// when [`PageDefaults::facing`] is on.
    pub fn geometry_of(&self, page: &Page, index: usize) -> PageGeometry {
        let master = page
            .master
            .as_ref()
            .and_then(|name| self.resources.masters.get(name));

        let mut margins = page
            .margins
            .or_else(|| master.and_then(|m| m.margins))
            .unwrap_or(self.page.margins);

        // Facing pages: `left` is the inner (gutter) margin, `right` the outer.
        // Page 1 is a recto, so odd zero-based indices are versos and mirror.
        if self.page.facing && index % 2 == 1 {
            std::mem::swap(&mut margins.left, &mut margins.right);
        }

        PageGeometry {
            size: page
                .size
                .or_else(|| master.and_then(|m| m.size))
                .unwrap_or(self.page.size),
            margins,
            bleed: self.page.bleed,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Meta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Vec<String>,
    /// BCP-47 tag, e.g. `"pt-BR"`. Emitted into the PDF catalog.
    pub language: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Page geometry
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageDefaults {
    pub size: PageSize,
    pub margins: Insets,
    /// Extra area beyond the trim, for print bleed.
    pub bleed: f64,
    /// Treat pages as left/right spreads, mirroring inner/outer margins.
    pub facing: bool,
}

impl Default for PageDefaults {
    fn default() -> Self {
        PageDefaults {
            size: PageSize::default(),
            // ~2cm all round.
            margins: Insets::all(56.7),
            bleed: 0.0,
            facing: false,
        }
    }
}

/// Fully-resolved geometry for one page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    pub size: PageSize,
    pub margins: Insets,
    pub bleed: f64,
}

impl PageGeometry {
    /// The full page box, origin at the top-left corner.
    pub fn page_box(&self) -> Rect {
        Rect::new(0.0, 0.0, self.size.width, self.size.height)
    }

    /// The area inside the margins.
    pub fn margin_box(&self) -> Rect {
        self.page_box().deflate(self.margins)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Resources — the optional sugar layer
// ─────────────────────────────────────────────────────────────────────────────

/// Reusable definitions referenced by name from the raw core.
///
/// Everything here is optional: a document that inlines all its styles and
/// content never needs a `resources` object at all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Resources {
    /// Named paragraph/character styles, referenced with `"use": "h1"` or
    /// `{"extends": "h1"}`. Sorted for reproducible output.
    pub styles: BTreeMap<String, Style>,
    /// Named page templates.
    pub masters: BTreeMap<String, Master>,
    /// Named text flows that can be threaded across frames.
    pub stories: BTreeMap<String, Vec<Block>>,
    /// Named colours, so a palette change is a one-line edit.
    pub colors: BTreeMap<String, Color>,
}

/// A page template: frames stamped onto every page that references it, plus
/// optional geometry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Master {
    pub size: Option<PageSize>,
    pub margins: Option<Insets>,
    pub background: Option<Color>,
    /// Painted beneath the page's own frames.
    pub frames: Vec<Frame>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Page
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Page {
    pub id: Option<String>,
    pub name: Option<String>,
    pub size: Option<PageSize>,
    pub margins: Option<Insets>,
    pub master: Option<String>,
    pub background: Option<Color>,
    /// Style scope for every frame on this page.
    pub style: Option<Style>,
    pub frames: Vec<Frame>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::frame::FrameContent;

    #[test]
    fn empty_object_is_a_valid_document() {
        let d: Document = serde_json::from_str("{}").unwrap();
        assert_eq!(d.version, SCHEMA_VERSION);
        assert!(d.pages.is_empty());
        assert!((d.page.size.width - 595.28).abs() < 0.1);
    }

    #[test]
    fn minimal_document_parses() {
        let json = r#"{
            "pages": [
                { "frames": [ {"type":"text","rect":[56,56,483,200],"blocks":["Olá"]} ] }
            ]
        }"#;
        let d: Document = serde_json::from_str(json).unwrap();
        assert_eq!(d.pages.len(), 1);
        assert_eq!(d.pages[0].frames.len(), 1);
        assert!(matches!(d.pages[0].frames[0].content, FrameContent::Text(_)));
    }

    #[test]
    fn page_geometry_falls_back_document_then_master() {
        let json = r#"{
            "page": { "size": "A5", "margins": 20 },
            "resources": { "masters": { "capa": { "size": "A4" } } },
            "pages": [ {}, { "master": "capa" }, { "size": "letter" } ]
        }"#;
        let d: Document = serde_json::from_str(json).unwrap();

        let from_defaults = d.geometry_of(&d.pages[0], 0);
        assert!((from_defaults.size.width - PageSize::named("a5").unwrap().width).abs() < 0.01);
        assert_eq!(from_defaults.margins, Insets::all(20.0));

        let from_master = d.geometry_of(&d.pages[1], 1);
        assert!((from_master.size.width - PageSize::named("a4").unwrap().width).abs() < 0.01);
        // Margins still come from the document defaults.
        assert_eq!(from_master.margins, Insets::all(20.0));

        let from_page = d.geometry_of(&d.pages[2], 2);
        assert_eq!(from_page.size.width, 612.0);
    }

    #[test]
    fn facing_pages_mirror_the_inner_margin() {
        let json = r#"{
            "page": { "size": "A5", "margins": [40, 20, 40, 60], "facing": true },
            "pages": [ {}, {}, {} ]
        }"#;
        let d: Document = serde_json::from_str(json).unwrap();

        // Page 1 is a recto: the gutter (60) stays on the left.
        let recto = d.geometry_of(&d.pages[0], 0);
        assert_eq!(recto.margins.left, 60.0);
        assert_eq!(recto.margins.right, 20.0);

        // Page 2 is a verso: the gutter moves to the right.
        let verso = d.geometry_of(&d.pages[1], 1);
        assert_eq!(verso.margins.left, 20.0);
        assert_eq!(verso.margins.right, 60.0);

        // Page 3 is a recto again.
        assert_eq!(d.geometry_of(&d.pages[2], 2).margins.left, 60.0);

        // Vertical margins never mirror.
        assert_eq!(verso.margins.top, 40.0);
    }

    #[test]
    fn without_facing_every_page_keeps_its_margins() {
        let json = r#"{ "page": { "margins": [40, 20, 40, 60] }, "pages": [ {}, {} ] }"#;
        let d: Document = serde_json::from_str(json).unwrap();
        assert_eq!(
            d.geometry_of(&d.pages[0], 0).margins,
            d.geometry_of(&d.pages[1], 1).margins
        );
    }

    #[test]
    fn margin_box_is_the_page_minus_margins() {
        let geo = PageGeometry {
            size: PageSize::new(600.0, 800.0),
            margins: Insets::all(50.0),
            bleed: 0.0,
        };
        assert_eq!(geo.margin_box(), Rect::new(50.0, 50.0, 500.0, 700.0));
    }

    #[test]
    fn resources_hold_named_styles_and_stories() {
        let json = r##"{
            "resources": {
                "styles": { "h1": { "fontSize": 18, "fontWeight": "bold" } },
                "stories": { "corpo": ["parágrafo um", "parágrafo dois"] },
                "colors": { "marca": "#336699" }
            }
        }"##;
        let d: Document = serde_json::from_str(json).unwrap();
        assert_eq!(d.resources.styles["h1"].font_size.unwrap().get(), 18.0);
        assert_eq!(d.resources.stories["corpo"].len(), 2);
        assert_eq!(d.resources.colors["marca"].to_hex(), "#336699");
    }

    #[test]
    fn document_roundtrips_through_json() {
        let json = r#"{
            "meta": { "title": "Material", "language": "pt-BR" },
            "page": { "size": "A4", "margins": [56, 42] },
            "pages": [ { "frames": [ {"type":"text","rect":[0,0,10,10],"blocks":["x"]} ] } ]
        }"#;
        let original: Document = serde_json::from_str(json).unwrap();
        let back: Document = serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(original, back);
    }
}
