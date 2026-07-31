//! The façade both bindings sit on.
//!
//! Holds the registered fonts and images and exposes the two operations that
//! matter: lay a document out, and turn it into a PDF. Both start from the same
//! [`DisplayList`], which is the reason the screen and the page agree.

use thiserror::Error;

use crate::display::DisplayList;
use crate::fonts::{FontError, FontId, FontRegistry};
use crate::images::ImageStore;
use crate::layout::LayoutEngine;
use crate::pdf::{self, PdfError};
use crate::spec::{Document, FontStyle, FontWeight};

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Font(#[from] FontError),
    #[error(transparent)]
    Pdf(#[from] PdfError),
    #[error("no fonts registered: call add_font before rendering")]
    NoFonts,
}

#[derive(Debug, Default)]
pub struct Engine {
    pub fonts: FontRegistry,
    pub images: ImageStore,
}

impl Engine {
    pub fn new() -> Self {
        Engine::default()
    }

    /// Register a font face. Pass `None` for `weight`/`italic` to trust what
    /// the font declares about itself.
    pub fn add_font(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        weight: Option<FontWeight>,
        italic: Option<bool>,
    ) -> Result<FontId, EngineError> {
        Ok(self.fonts.add(family, bytes, weight, italic)?)
    }

    pub fn add_image(&mut self, key: &str, bytes: Vec<u8>) {
        self.images.add(key, bytes);
    }

    pub fn clear(&mut self) {
        self.fonts.clear();
        self.images.clear();
    }

    /// Lay out a document. Never fails — problems arrive as diagnostics.
    pub fn layout(&self, document: &Document) -> DisplayList {
        LayoutEngine::new(&self.fonts, &self.images).layout(document)
    }

    /// Lay out a document and render it to PDF bytes.
    pub fn render_pdf(&self, document: &Document) -> Result<Vec<u8>, EngineError> {
        if self.fonts.is_empty() {
            return Err(EngineError::NoFonts);
        }
        let list = self.layout(document);
        self.render_display_list(&list, document)
    }

    /// Render an already-computed display list.
    ///
    /// The editor uses this to export exactly what it last painted, with no
    /// second layout pass that could drift from what the user saw.
    pub fn render_display_list(
        &self,
        list: &DisplayList,
        document: &Document,
    ) -> Result<Vec<u8>, EngineError> {
        Ok(pdf::render(list, &self.fonts, &self.images, &document.meta)?)
    }

    /// Outline of one glyph as an SVG path, in em units with y growing down.
    ///
    /// The browser caches these as `Path2D` objects and paints them with the
    /// transform the display list dictates, so the canvas draws the same
    /// contours the PDF embeds.
    pub fn glyph_path(&self, font: u32, glyph: u16) -> Option<String> {
        self.fonts.face(FontId(font))?.glyph_path(glyph)
    }

    /// Pick the face a given family/weight/style resolves to.
    pub fn select_font(
        &self,
        family: Option<&str>,
        weight: FontWeight,
        style: FontStyle,
    ) -> Option<u32> {
        self.fonts.select(family, weight, style).map(|id| id.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::test_fonts;

    fn engine() -> Option<Engine> {
        let mut engine = Engine::new();
        engine
            .add_font("body", test_fonts::dejavu()?.to_vec(), None, None)
            .ok()?;
        Some(engine)
    }

    fn sample() -> Document {
        serde_json::from_str(
            r#"{
                "meta": { "title": "Material", "language": "pt-BR" },
                "pages": [{ "frames": [
                    {"type":"text","rect":[56,56,483,300],"blocks":["Fotossíntese","As plantas convertem luz."]}
                ]}]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn rendering_without_fonts_is_an_error() {
        let engine = Engine::new();
        assert!(matches!(
            engine.render_pdf(&sample()),
            Err(EngineError::NoFonts)
        ));
    }

    #[test]
    fn produces_a_valid_pdf_header_and_trailer() {
        let Some(engine) = engine() else { return };
        let bytes = engine.render_pdf(&sample()).unwrap();

        assert!(bytes.starts_with(b"%PDF-1."), "missing PDF header");
        assert!(
            bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF"),
            "missing EOF marker"
        );
        assert!(bytes.len() > 2000, "suspiciously small PDF: {}", bytes.len());
    }

    #[test]
    fn an_empty_document_still_yields_one_page() {
        let Some(engine) = engine() else { return };
        let empty: Document = serde_json::from_str("{}").unwrap();
        let bytes = engine.render_pdf(&empty).unwrap();
        assert!(bytes.starts_with(b"%PDF-1."));
        assert!(contains(&bytes, b"/Count 1"));
    }

    #[test]
    fn the_pdf_embeds_a_subset_font() {
        let Some(engine) = engine() else { return };
        let bytes = engine.render_pdf(&sample()).unwrap();
        assert!(contains(&bytes, b"/Type0"), "no Type0 font");
        assert!(contains(&bytes, b"Identity-H"), "no Identity-H encoding");
        assert!(contains(&bytes, b"/FontFile2"), "font not embedded");
        assert!(contains(&bytes, b"/ToUnicode"), "no ToUnicode map");
    }

    #[test]
    fn page_count_follows_the_document() {
        let Some(engine) = engine() else { return };
        let doc: Document = serde_json::from_str(r#"{"pages":[{},{},{}]}"#).unwrap();
        let bytes = engine.render_pdf(&doc).unwrap();
        assert!(contains(&bytes, b"/Count 3"));
    }

    #[test]
    fn metadata_reaches_the_document_info() {
        let Some(engine) = engine() else { return };
        let bytes = engine.render_pdf(&sample()).unwrap();
        assert!(contains(&bytes, b"Material"), "title missing");
        assert!(contains(&bytes, b"pt-BR"), "language missing");
    }

    #[test]
    fn the_same_document_renders_byte_for_byte_the_same() {
        let Some(engine) = engine() else { return };
        let first = engine.render_pdf(&sample()).unwrap();
        let second = engine.render_pdf(&sample()).unwrap();
        assert_eq!(first, second, "output is not reproducible");
    }

    #[test]
    fn glyph_paths_come_back_for_real_glyphs() {
        let Some(engine) = engine() else { return };
        let list = engine.layout(&sample());
        let font = list.fonts[0].id;

        // Find any glyph the layout actually used. Text frames are clipped, so
        // their runs sit inside a group.
        fn first_glyph(items: &[crate::display::DisplayItem]) -> Option<u16> {
            use crate::display::DisplayItem;
            items.iter().find_map(|item| match item {
                DisplayItem::Glyphs(run) => run.glyphs.first().map(|g| g.id),
                DisplayItem::Group(group) => first_glyph(&group.items),
                _ => None,
            })
        }
        let glyph = first_glyph(&list.pages[0].items).expect("a glyph was laid out");

        let path = engine.glyph_path(font, glyph).expect("glyph has an outline");
        assert!(path.starts_with('M'));
    }

    #[test]
    fn rendering_a_display_list_matches_rendering_the_document() {
        let Some(engine) = engine() else { return };
        let document = sample();
        let list = engine.layout(&document);

        let from_list = engine.render_display_list(&list, &document).unwrap();
        let from_doc = engine.render_pdf(&document).unwrap();
        assert_eq!(from_list, from_doc);
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }
}
