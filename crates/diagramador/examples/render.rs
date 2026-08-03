//! Render a document JSON to PDF.
//!
//! ```sh
//! cargo run --example render -- examples/material.json out.pdf
//! cargo run --example render -- examples/material.json out.json   # display list
//! ```

use std::fs;
use std::process::ExitCode;

use diagramador::Engine;
use diagramador::spec::{Document, FontWeight};

/// The faces shipped in `fonts/`, registered under one family so the cascade
/// can pick between them by weight and slant.
/// The same two families the browser editor registers, so a document renders
/// identically from the command line and from the page.
const FACES: &[(&str, &str, u16, bool)] = &[
    ("corpo", "DejaVuSans.ttf", 400, false),
    ("corpo", "DejaVuSans-Bold.ttf", 700, false),
    ("corpo", "DejaVuSans-Oblique.ttf", 400, true),
    ("corpo", "DejaVuSans-BoldOblique.ttf", 700, true),
    ("plex", "IBMPlexSans-Regular.ttf", 400, false),
    ("plex", "IBMPlexSans-Bold.ttf", 700, false),
    ("plex", "IBMPlexSans-Italic.ttf", 400, true),
];

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let input = args.next().unwrap_or_else(|| "examples/material.json".into());
    let output = args.next().unwrap_or_else(|| "out.pdf".into());

    let mut engine = Engine::new();
    for (family, file, weight, italic) in FACES {
        let path = format!("fonts/{file}");
        match fs::read(&path) {
            Ok(bytes) => {
                if let Err(error) =
                    engine.add_font(family, bytes, Some(FontWeight(*weight)), Some(*italic))
                {
                    eprintln!("aviso: {path}: {error}");
                }
            }
            Err(error) => eprintln!("aviso: não consegui ler {path}: {error}"),
        }
    }

    if engine.fonts.is_empty() {
        eprintln!("erro: nenhuma fonte carregada — rode a partir da raiz do repositório");
        return ExitCode::FAILURE;
    }

    // Images live in an `images/` folder beside the document, and are keyed by
    // file name — the same key the editor uses when you import one.
    let assets = std::path::Path::new(&input)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("images");

    if let Ok(entries) = fs::read_dir(&assets) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(key) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            match fs::read(&path) {
                Ok(bytes) => engine.add_image(key, bytes),
                Err(error) => eprintln!("aviso: não consegui ler {}: {error}", path.display()),
            }
        }
    }

    let json = match fs::read_to_string(&input) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("erro: {input}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let document: Document = match serde_json::from_str(&json) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("erro: JSON inválido em {input}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let list = engine.layout(&document);
    for diagnostic in &list.diagnostics {
        eprintln!(
            "{:?} [{}] {}{}",
            diagnostic.severity,
            diagnostic.code,
            diagnostic.message,
            diagnostic
                .frame
                .as_ref()
                .map(|f| format!(" (frame {f})"))
                .unwrap_or_default()
        );
    }

    // Asking for JSON gives the display list instead of a PDF: every position
    // the engine decided, before anything is drawn. It is what you want when
    // two renderings differ by a pixel and the question is by how much.
    if output.ends_with(".json") {
        let text = match serde_json::to_string_pretty(&list) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("erro ao serializar a display list: {error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = fs::write(&output, &text) {
            eprintln!("erro ao escrever {output}: {error}");
            return ExitCode::FAILURE;
        }
        println!("{output}: {} páginas, {} itens", list.pages.len(), list.item_count());
        return ExitCode::SUCCESS;
    }

    let bytes = match engine.render_display_list(&list, &document) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("erro ao gerar PDF: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = fs::write(&output, &bytes) {
        eprintln!("erro ao escrever {output}: {error}");
        return ExitCode::FAILURE;
    }

    println!(
        "{output}: {} páginas, {} itens, {} KB",
        list.pages.len(),
        list.item_count(),
        bytes.len() / 1024
    );
    ExitCode::SUCCESS
}
