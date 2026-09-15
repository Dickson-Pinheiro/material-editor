//! Write the JSON Schemas of the public formats to `schema/` at the workspace
//! root. The TypeScript and Pydantic mirrors in the consumers are generated
//! from these files, never written by hand.
//!
//! ```sh
//! cargo run --example schema
//! ```

use std::fs;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("schema");
    fs::create_dir_all(&dir).expect("create schema/");

    let write = |name: &str, schema: schemars::Schema| {
        let path = dir.join(name);
        let json = serde_json::to_string_pretty(schema.as_value()).unwrap();
        fs::write(&path, json + "\n").expect("write schema");
        println!("→ {}", path.display());
    };

    write("document.schema.json", diagramador::schema::document_schema());
    write("display.schema.json", diagramador::schema::display_schema());
}
