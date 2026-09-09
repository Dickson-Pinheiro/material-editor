//! JSON Schema for the public formats.
//!
//! The derived schemas cover every type whose serde is derived. The types with
//! a hand-written `Deserialize` — the ones that accept shorthands such as
//! `"18mm"`, `[x, y, w, h]` or a bare string where a block goes — describe here
//! exactly what their visitor accepts, so a schema consumer (the TypeScript
//! and Pydantic mirrors generated from this) offers the same shorthands the
//! engine reads.
//!
//! `cargo run --example schema` writes `schema/document.schema.json` and
//! `schema/display.schema.json` at the workspace root.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema, schema_for};
use serde::Deserialize;

use crate::color::Color;
use crate::display::DisplayList;
use crate::spec::content::{
    Paragraph, PanelBlock, RuleBlock, SpacerBlock, TableBlock, Tab, SpaceRun, InlineImage,
    InlineRule, TextRun, TrackSize,
};
use crate::spec::style::{FontWeight, LineHeight};
use crate::spec::{Block, Document, Inline};
use crate::units::{Corners, Insets, Len, PageSize, Rect};

/// A length in points, or a string with an explicit unit.
const LEN_PATTERN: &str = r"^\s*-?\d*\.?\d+\s*(pt|mm|cm|in|px)?\s*$";

fn len_schema() -> Schema {
    json_schema!({
        "anyOf": [
            { "type": "number" },
            { "type": "string", "pattern": LEN_PATTERN }
        ],
        "description": "A length: a number of points, or a string with a unit (\"18mm\", \"1cm\", \"12pt\", \"16px\", \"0.5in\")."
    })
}

impl JsonSchema for Len {
    fn schema_name() -> Cow<'static, str> {
        "Len".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        len_schema()
    }
}

impl JsonSchema for Rect {
    fn schema_name() -> Cow<'static, str> {
        "Rect".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        let len = g.subschema_for::<Len>();
        json_schema!({
            "anyOf": [
                { "type": "array", "items": len, "minItems": 4, "maxItems": 4 },
                {
                    "type": "object",
                    "properties": { "x": len, "y": len, "w": len, "h": len, "width": len, "height": len },
                    "additionalProperties": false
                }
            ],
            "description": "[x, y, w, h] in points from the page's top-left corner, or { x, y, w, h }."
        })
    }
}

impl JsonSchema for Insets {
    fn schema_name() -> Cow<'static, str> {
        "Insets".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        let len = g.subschema_for::<Len>();
        json_schema!({
            "anyOf": [
                len,
                { "type": "array", "items": len, "minItems": 1, "maxItems": 4 },
                {
                    "type": "object",
                    "properties": { "top": len, "right": len, "bottom": len, "left": len },
                    "additionalProperties": false
                }
            ],
            "description": "CSS shorthand: one value, [vertical, horizontal], [top, right, bottom, left], or { top, right, bottom, left }."
        })
    }
}

impl JsonSchema for Corners {
    fn schema_name() -> Cow<'static, str> {
        "Corners".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        let len = g.subschema_for::<Len>();
        json_schema!({
            "anyOf": [
                { "type": "null" },
                len,
                { "type": "array", "items": len, "minItems": 1, "maxItems": 4 },
                {
                    "type": "object",
                    "properties": { "topLeft": len, "topRight": len, "bottomRight": len, "bottomLeft": len },
                    "additionalProperties": false
                }
            ],
            "description": "Corner radii clockwise from the top-left: one value, [tl, tr], [tl, tr, br, bl], or a map. Nothing means square corners."
        })
    }
}

impl JsonSchema for PageSize {
    fn schema_name() -> Cow<'static, str> {
        "PageSize".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        let len = g.subschema_for::<Len>();
        json_schema!({
            "anyOf": [
                { "type": "string", "description": "A named size (\"A4\", \"A5\", \"letter\", \"livro-didatico\", …), optionally suffixed \" landscape\"." },
                { "type": "array", "items": len, "minItems": 2, "maxItems": 2 },
                {
                    "type": "object",
                    "properties": { "width": len, "height": len },
                    "required": ["width", "height"],
                    "additionalProperties": false
                }
            ]
        })
    }
}

impl JsonSchema for Color {
    fn schema_name() -> Cow<'static, str> {
        "Color".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "A CSS colour: #rgb, #rgba, #rrggbb, #rrggbbaa, rgb()/rgba(), a named colour, or \"@name\" for an entry of resources.colors."
        })
    }
}

impl JsonSchema for FontWeight {
    fn schema_name() -> Cow<'static, str> {
        "FontWeight".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                { "type": "integer", "minimum": 1, "maximum": 1000 },
                { "type": "string", "enum": ["thin", "extralight", "ultralight", "light", "normal", "regular", "book", "medium", "semibold", "demibold", "bold", "extrabold", "ultrabold", "black", "heavy"] }
            ]
        })
    }
}

impl JsonSchema for LineHeight {
    fn schema_name() -> Cow<'static, str> {
        "LineHeight".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                { "type": "number", "description": "A multiple of the font size." },
                { "type": "string", "pattern": LEN_PATTERN, "description": "An absolute length." }
            ]
        })
    }
}

impl JsonSchema for TrackSize {
    fn schema_name() -> Cow<'static, str> {
        "TrackSize".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                { "type": "string", "const": "auto" },
                { "type": "number" },
                { "type": "string", "pattern": r"^\s*-?\d*\.?\d+\s*(pt|mm|cm|in|px|fr|%)\s*$" }
            ],
            "description": "\"auto\", a fixed length, \"1fr\" (a share of the leftover) or \"25%\" (a share of the whole)."
        })
    }
}

/// The object form of [`Block`]. The string form is a paragraph of that text.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(dead_code)]
enum BlockObject {
    Paragraph(Paragraph),
    Rule(RuleBlock),
    Spacer(SpacerBlock),
    FrameBreak,
    ColumnBreak,
    PageBreak,
    Table(TableBlock),
    Panel(PanelBlock),
}

impl JsonSchema for Block {
    fn schema_name() -> Cow<'static, str> {
        "Block".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        let object = g.subschema_for::<BlockObject>();
        json_schema!({
            "anyOf": [
                { "type": "string", "description": "Shorthand for a paragraph holding this text." },
                object
            ]
        })
    }
}

/// The object form of [`Inline`]. The string form is a text run.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(dead_code)]
enum InlineObject {
    Text(TextRun),
    Break,
    Tab(Tab),
    Space(SpaceRun),
    Image(InlineImage),
    Rule(InlineRule),
}

impl JsonSchema for Inline {
    fn schema_name() -> Cow<'static, str> {
        "Inline".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        let object = g.subschema_for::<InlineObject>();
        json_schema!({
            "anyOf": [
                { "type": "string", "description": "Shorthand for a text run." },
                object
            ]
        })
    }
}

/// The schema of the input document.
pub fn document_schema() -> Schema {
    tidy(schema_for!(Document))
}

/// The schema of the display list the engine emits.
pub fn display_schema() -> Schema {
    tidy(schema_for!(DisplayList))
}

/// Rewrite tagged unions into the shape code generators read well.
///
/// For a `#[serde(tag = "type")]` enum, and for a flattened one, schemars
/// writes each variant as `{ "$ref": Payload, "properties": { "type": … } }`
/// — a `$ref` with siblings. That is valid JSON Schema 2020-12, but both
/// `json-schema-to-typescript` and `datamodel-code-generator` drop the
/// referenced properties on the floor and keep only the tag. So the tag and
/// the enclosing struct's own properties are folded **into** the payload
/// definition, and the union becomes a plain list of references. `TextFrame`
/// ends up carrying `type: "text"` and every `Frame` field; `Frame` is the
/// union of its six variants. Unit variants get a definition of their own,
/// named after the tag value.
fn tidy(schema: Schema) -> Schema {
    use serde_json::{Map, Value, json};

    let mut root = schema.to_value();
    let Some(defs) = root.get_mut("$defs").and_then(Value::as_object_mut) else {
        return Schema::try_from(root).expect("a schema stays a schema");
    };

    let names: Vec<String> = defs.keys().cloned().collect();
    let mut new_defs: Vec<(String, Value)> = Vec::new();

    for name in &names {
        let Some(def) = defs.get(name) else { continue };
        let Some(variants) = def.get("oneOf").and_then(Value::as_array).cloned() else {
            continue;
        };
        // Only tagged unions of objects. A plain enum (`ShapeKind`) is also a
        // `oneOf`, of string constants, and must stay as it is.
        let tagged = variants.iter().all(|v| {
            v.get("properties").is_some()
                && v.get("required").is_some()
                && v.get("type").and_then(Value::as_str) == Some("object")
        });
        if !tagged {
            continue;
        }
        let base_props = def
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let base_required: Vec<Value> = def
            .get("required")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut refs: Vec<Value> = Vec::new();
        for variant in variants {
            let Some(vobj) = variant.as_object() else {
                refs.push(variant);
                continue;
            };
            let vprops = vobj.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
            let vrequired: Vec<Value> =
                vobj.get("required").and_then(Value::as_array).cloned().unwrap_or_default();

            let target_name = match vobj.get("$ref").and_then(Value::as_str) {
                Some(r) => r.trim_start_matches("#/$defs/").to_string(),
                None => {
                    // A unit variant: name it after its tag value.
                    let tag = vprops
                        .values()
                        .find_map(|p| p.get("const").and_then(Value::as_str))
                        .unwrap_or("Variant");
                    let mut candidate = pascal(tag);
                    if defs.contains_key(&candidate) || new_defs.iter().any(|(n, _)| n == &candidate) {
                        candidate = format!("{name}{candidate}");
                    }
                    let mut fresh = Map::new();
                    fresh.insert("type".into(), json!("object"));
                    if let Some(d) = vobj.get("description") {
                        fresh.insert("description".into(), d.clone());
                    }
                    new_defs.push((candidate.clone(), Value::Object(fresh)));
                    candidate
                }
            };

            let target = match defs.get_mut(&target_name) {
                Some(t) => t,
                None => &mut new_defs.iter_mut().find(|(n, _)| n == &target_name).unwrap().1,
            };
            let tobj = target.as_object_mut().expect("payload definitions are objects");
            let mut props = base_props.clone();
            for (k, v) in tobj.get("properties").and_then(Value::as_object).cloned().unwrap_or_default() {
                props.insert(k, v);
            }
            for (k, v) in vprops {
                props.insert(k, v);
            }
            let mut required = base_required.clone();
            for r in tobj.get("required").and_then(Value::as_array).cloned().unwrap_or_default() {
                if !required.contains(&r) {
                    required.push(r);
                }
            }
            for r in vrequired {
                if !required.contains(&r) {
                    required.push(r);
                }
            }
            tobj.insert("type".into(), json!("object"));
            tobj.insert("properties".into(), Value::Object(props));
            tobj.insert("required".into(), Value::Array(required));
            refs.push(json!({ "$ref": format!("#/$defs/{target_name}") }));
        }

        let def = defs.get_mut(name).unwrap().as_object_mut().unwrap();
        def.remove("properties");
        def.remove("required");
        def.remove("type");
        def.insert("oneOf".into(), Value::Array(refs));
    }

    for (name, value) in new_defs {
        defs.insert(name, value);
    }

    Schema::try_from(root).expect("a schema stays a schema")
}

fn pascal(tag: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in tag.chars() {
        if ch == '_' || ch == '-' {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validator(schema: &Schema) -> jsonschema::Validator {
        jsonschema::validator_for(schema.as_value()).expect("the generated schema is itself valid")
    }

    #[test]
    fn the_shipped_example_validates_against_the_document_schema() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/material.json"
        ))
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let schema = document_schema();
        let validator = validator(&schema);
        let errors: Vec<String> = validator.iter_errors(&value).map(|e| format!("{e} at {}", e.instance_path)).collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }

    #[test]
    fn shorthands_are_part_of_the_schema() {
        let schema = document_schema();
        let validator = validator(&schema);
        let doc = serde_json::json!({
            "page": { "size": "A4 landscape", "margins": "20mm" },
            "resources": { "colors": { "acento": "#1e555c" } },
            "pages": [{ "frames": [
                { "type": "text", "rect": ["10mm", 10, 100, 40], "fill": "@acento", "radius": [4, 0], "blocks": ["Olá", { "type": "paragraph", "content": ["a", { "type": "break" }] }] },
                { "type": "shape", "shape": "ellipse", "rect": [0, 0, 10, 10] }
            ] }]
        });
        let errors: Vec<String> = validator.iter_errors(&doc).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }

    #[test]
    fn a_wrong_shape_is_refused() {
        let schema = document_schema();
        let validator = validator(&schema);
        let doc = serde_json::json!({ "pages": [{ "frames": [{ "type": "text", "rect": "nope" }] }] });
        assert!(!validator.is_valid(&doc));
    }

    #[test]
    fn tagged_unions_are_lists_of_named_definitions() {
        let schema = document_schema();
        let defs = schema.as_value()["$defs"].as_object().unwrap();
        let frame = &defs["Frame"];
        assert!(frame.get("properties").is_none(), "base props moved into the variants");
        let variants: Vec<&str> = frame["oneOf"].as_array().unwrap().iter().map(|v| v["$ref"].as_str().unwrap()).collect();
        assert!(variants.contains(&"#/$defs/TextFrame"));
        let text = &defs["TextFrame"];
        assert_eq!(text["properties"]["type"]["const"], "text");
        assert!(text["properties"].get("rect").is_some(), "a TextFrame carries the frame geometry");
        assert!(text["properties"].get("blocks").is_some());
        // Unit variants got a definition of their own.
        assert!(defs.contains_key("FrameBreak"));
        assert_eq!(defs["FrameBreak"]["properties"]["type"]["const"], "frameBreak");
        assert_eq!(defs["Box"]["properties"]["kind"]["const"], "box");
    }

    #[test]
    fn the_display_list_round_trips_through_its_schema() {
        let schema = display_schema();
        let validator = validator(&schema);
        let list = DisplayList::new();
        let value = serde_json::to_value(&list).unwrap();
        assert!(validator.is_valid(&value));
    }
}
