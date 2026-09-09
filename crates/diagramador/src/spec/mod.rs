//! The public JSON schema — two layers over one core.
//!
//! **Raw core.** A [`Document`] is a list of [`Page`]s; a page is a list of
//! [`Frame`]s; a frame is a positioned box holding text [`Block`]s, an image,
//! a shape, or a group. Nothing here knows about school materials, exams or
//! books — it is geometry plus styled runs.
//!
//! **Sugar.** [`Resources`] adds named styles, page masters and threaded
//! stories. Every one of them is resolved away before layout, so the engine
//! only ever sees the raw core. Documents that want none of it can omit
//! `resources` entirely.

pub mod chart;
pub mod content;
pub mod document;
pub mod frame;
pub mod style;

pub use content::{
    PanelBlock,
    Block, Inline, InlineImage, InlineRule, Marker, Origin, Paragraph, RuleBlock, SpaceRun,
    SpacerBlock, Tab, TextRun,
};
pub use document::{
    Component, Document, Master, Meta, Page, PageDefaults, PageGeometry, Resources,
    SCHEMA_VERSION,
};
pub use frame::{
    Border, BorderStyle, CornerStyle, Follow, Frame, FrameContent, GroupFrame, ImageAlign,
    ImageFit, ImageFrame, InstanceFrame, ShapeFrame, ShapeKind, Sides, SlotValue, TextFrame,
};

/// Keys whose string value is a colour, and may therefore say `"@name"`.
const COLOR_KEYS: [&str; 3] = ["fill", "color", "background"];

/// Parse a document, resolving `"@name"` colour references first.
///
/// Both bindings enter here. The reference is resolved on the JSON value,
/// before serde runs, so [`crate::color::Color`] stays a plain RGBA and every
/// consumer of a colour keeps seeing a literal. Only colour-bearing keys
/// (`fill`, `color`, `background`, and the `palette` list of a chart) are
/// touched: a paragraph that begins with `@` is text, not a reference.
pub fn parse_document(json: &str) -> Result<Document, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("documento inválido: {e}"))?;

    let palette: std::collections::BTreeMap<String, String> = value
        .get("resources")
        .and_then(|r| r.get("colors"))
        .and_then(|c| c.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    if !palette.is_empty() || json.contains("\"@") {
        resolve_color_refs(&mut value, &palette, "")?;
    }

    serde_json::from_value(value).map_err(|e| format!("documento inválido: {e}"))
}

fn resolve_color_refs(
    value: &mut serde_json::Value,
    palette: &std::collections::BTreeMap<String, String>,
    path: &str,
) -> Result<(), String> {
    use serde_json::Value;

    let lookup = |raw: &str, at: &str| -> Result<Option<String>, String> {
        match raw.strip_prefix('@') {
            None => Ok(None),
            Some(name) => palette
                .get(name)
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("cor `@{name}` não existe em resources.colors ({at})")),
        }
    };

    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let at = format!("{path}/{key}");
                if key == "colors" && path == "/resources" {
                    continue;
                }
                if COLOR_KEYS.contains(&key.as_str()) {
                    if let Value::String(raw) = child
                        && let Some(hex) = lookup(raw, &at)?
                    {
                        *child = Value::String(hex);
                    }
                    continue;
                }
                if key == "palette"
                    && let Value::Array(items) = child
                {
                    for item in items.iter_mut() {
                        if let Value::String(raw) = item
                            && let Some(hex) = lookup(raw, &at)?
                        {
                            *item = Value::String(hex);
                        }
                    }
                    continue;
                }
                resolve_color_refs(child, palette, &at)?;
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                resolve_color_refs(item, palette, &format!("{path}/{index}"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_colour_resolves_to_the_palette_entry() {
        let doc = parse_document(
            r##"{"resources":{"colors":{"acento":"#1e555c"}},
                 "pages":[{"frames":[{"type":"shape","rect":[0,0,1,1],"fill":"@acento",
                   "border":{"color":"@acento"}}]}]}"##,
        )
        .unwrap();
        let frame = &doc.pages[0].frames[0];
        assert_eq!(frame.fill.unwrap().to_hex(), "#1e555c");
        assert_eq!(frame.border.as_ref().unwrap().color.to_hex(), "#1e555c");
    }

    #[test]
    fn an_unknown_name_fails_and_says_which() {
        let error = parse_document(
            r##"{"resources":{"colors":{}},"pages":[{"frames":[{"type":"shape","rect":[0,0,1,1],"fill":"@nada"}]}]}"##,
        )
        .unwrap_err();
        assert!(error.contains("@nada"), "{error}");
    }

    #[test]
    fn text_that_starts_with_an_at_sign_is_left_alone() {
        let doc = parse_document(
            r##"{"pages":[{"frames":[{"type":"text","rect":[0,0,1,1],"blocks":["@aluno, leia"]}]}]}"##,
        )
        .unwrap();
        let text = doc.pages[0].frames[0].as_text().unwrap();
        assert_eq!(text.blocks[0].as_paragraph().unwrap().plain_text(), "@aluno, leia");
    }
}
pub use style::{
    FontStyle, FontWeight, LineHeight, Overflow, ResolvedStyle, Style, TextAlign, TextTransform,
    VerticalAlign,
};
