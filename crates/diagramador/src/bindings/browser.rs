//! Browser bindings (`wasm-bindgen`).
//!
//! ```js
//! import init, { addFont, layout, renderPdf } from "diagramador";
//!
//! await init();
//! addFont("corpo", regularBytes, 400, false);
//! addFont("corpo", boldBytes, 700, false);
//!
//! const displayList = layout(JSON.stringify(document));  // paint this
//! const pdf = renderPdf(JSON.stringify(document));       // and/or export it
//! ```
//!
//! The document goes in as a JSON string rather than a `JsValue`: the schema
//! uses maps (`resources.styles`, `resources.stories`), and JSON is the one
//! representation that means the same thing here, in Python and in Go.
//! The display list comes back as a live object — it is read on every frame,
//! so it skips the string round-trip.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use crate::engine::Engine;
use crate::spec::{Document, FontWeight};

thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::new());
}

fn parse(json: &str) -> Result<Document, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("documento inválido: {e}")))
}

// ─────────────────────────────────────────────────────────────────────────────
// Resources
// ─────────────────────────────────────────────────────────────────────────────

/// Register a font face under a family name.
///
/// `weight` (100–900) and `italic` may be `undefined`, in which case the font's
/// own OS/2 table decides. Returns the face id used by the display list.
#[wasm_bindgen(js_name = addFont)]
pub fn add_font(
    family: &str,
    data: &[u8],
    weight: Option<u16>,
    italic: Option<bool>,
) -> Result<u32, JsError> {
    ENGINE.with(|engine| {
        engine
            .borrow_mut()
            .add_font(family, data.to_vec(), weight.map(FontWeight), italic)
            .map(|id| id.0)
            .map_err(|e| JsError::new(&e.to_string()))
    })
}

/// Register image bytes (PNG or JPEG) under the key documents reference.
#[wasm_bindgen(js_name = addImage)]
pub fn add_image(key: &str, data: &[u8]) {
    ENGINE.with(|engine| engine.borrow_mut().add_image(key, data.to_vec()));
}

/// Drop every registered font and image.
#[wasm_bindgen(js_name = clearResources)]
pub fn clear_resources() {
    ENGINE.with(|engine| engine.borrow_mut().clear());
}

/// Make `family` the fallback for documents that name no font.
#[wasm_bindgen(js_name = setDefaultFamily)]
pub fn set_default_family(family: &str) {
    ENGINE.with(|engine| engine.borrow_mut().fonts.set_default_family(family));
}

// ─────────────────────────────────────────────────────────────────────────────
// Layout and rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Lay a document out and return its display list.
///
/// Never throws for content problems — malformed geometry, missing images and
/// overset frames come back in `diagnostics` so the editor can show the
/// document and flag what is wrong. It throws only if the JSON does not parse.
#[wasm_bindgen]
pub fn layout(document_json: &str) -> Result<JsValue, JsError> {
    let document = parse(document_json)?;
    let list = ENGINE.with(|engine| engine.borrow().layout(&document));
    serde_wasm_bindgen::to_value(&list)
        .map_err(|e| JsError::new(&format!("falha ao serializar o display list: {e}")))
}

/// Lay a document out and return the display list as a JSON string.
///
/// Useful for snapshot tests that need to compare against the other bindings.
#[wasm_bindgen(js_name = layoutJson)]
pub fn layout_json(document_json: &str) -> Result<String, JsError> {
    let document = parse(document_json)?;
    let list = ENGINE.with(|engine| engine.borrow().layout(&document));
    serde_json::to_string(&list).map_err(|e| JsError::new(&e.to_string()))
}

/// Render a document to PDF bytes.
#[wasm_bindgen(js_name = renderPdf)]
pub fn render_pdf(document_json: &str) -> Result<Vec<u8>, JsError> {
    let document = parse(document_json)?;
    ENGINE.with(|engine| {
        engine
            .borrow()
            .render_pdf(&document)
            .map_err(|e| JsError::new(&e.to_string()))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Glyph outlines
// ─────────────────────────────────────────────────────────────────────────────

/// Outline of one glyph as an SVG path, in em units with y growing down.
///
/// Feed it straight to `new Path2D(d)`, then paint with
/// `translate(run.x + glyph.x, run.y); scale(run.size, run.size)`. Caching the
/// `Path2D` per `(font, glyph)` is worth it — the same glyphs recur constantly.
#[wasm_bindgen(js_name = glyphPath)]
pub fn glyph_path(font: u32, glyph: u16) -> Option<String> {
    ENGINE.with(|engine| engine.borrow().glyph_path(font, glyph))
}

/// Outlines for a batch of glyphs of one face, as a `{ [glyphId]: path }`
/// object. One call per face beats one call per glyph when warming the cache.
#[wasm_bindgen(js_name = glyphPaths)]
pub fn glyph_paths(font: u32, glyphs: &[u16]) -> Result<JsValue, JsError> {
    let paths: Vec<(String, String)> = ENGINE.with(|engine| {
        let engine = engine.borrow();
        glyphs
            .iter()
            .filter_map(|glyph| {
                engine
                    .glyph_path(font, *glyph)
                    .map(|path| (glyph.to_string(), path))
            })
            .collect()
    });

    let object = js_sys::Object::new();
    for (glyph, path) in paths {
        js_sys::Reflect::set(&object, &JsValue::from_str(&glyph), &JsValue::from_str(&path))
            .map_err(|_| JsError::new("falha ao montar o objeto de contornos"))?;
    }
    Ok(object.into())
}

/// The schema version this build speaks.
#[wasm_bindgen(js_name = schemaVersion)]
pub fn schema_version() -> u32 {
    crate::spec::SCHEMA_VERSION
}
