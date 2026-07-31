//! Time the layout engine.
//!
//! ```sh
//! cargo run --release --example bench --no-default-features --features images
//! ```
//!
//! Two questions, and they are different:
//!
//! 1. does a document *without* wrap still lay out as fast as it did? That is
//!    the regression number, and the one that must not move;
//! 2. what does a page *with* wrap cost? That is the price of the feature,
//!    and it is only worth optimising once it has been measured.

use std::time::Instant;

use diagramador::Engine;
use diagramador::spec::{Document, FontWeight};

const FACES: &[(&str, &str, u16, bool)] = &[
    ("corpo", "DejaVuSans.ttf", 400, false),
    ("corpo", "DejaVuSans-Bold.ttf", 700, false),
    ("corpo", "DejaVuSans-Oblique.ttf", 400, true),
    ("corpo", "DejaVuSans-BoldOblique.ttf", 700, true),
    ("plex", "IBMPlexSans-Regular.ttf", 400, false),
    ("plex", "IBMPlexSans-Bold.ttf", 700, false),
    ("plex", "IBMPlexSans-Italic.ttf", 400, true),
];

/// Runs per case. The engine is fast enough that one sample is noise.
const RUNS: usize = 60;

fn main() {
    let mut engine = Engine::new();
    for (family, file, weight, italic) in FACES {
        let Ok(bytes) = std::fs::read(format!("fonts/{file}")) else {
            eprintln!("fontes ausentes: rode a partir da raiz do repositório");
            return;
        };
        let _ = engine.add_font(family, bytes, Some(FontWeight(*weight)), Some(*italic));
    }

    let material = std::fs::read_to_string("examples/material.json").ok();
    if let Some(json) = &material {
        let doc: Document = serde_json::from_str(json).expect("material válido");
        report("material, sem contorno", &engine, &doc);
    } else {
        eprintln!("examples/material.json ausente — caso pulado");
    }

    report("uma página, cinco contornos", &engine, &wrapped(5));
    report("uma página, sem contorno", &engine, &wrapped(0));
}

fn report(label: &str, engine: &Engine, document: &Document) {
    // One untimed run so the first allocation is not part of the sample.
    let list = engine.layout(document);
    let pages = list.pages.len();
    let frames: usize = list.pages.iter().map(|page| page.frames.len()).sum();

    let mut times: Vec<f64> = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        let produced = engine.layout(document);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(produced);
    }

    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    let best = times[0];
    let worst = times[times.len() - 1];

    println!(
        "{label:<30} {pages:>3} pág {frames:>4} frames   mediana {median:>7.2} ms  \
         (melhor {best:.2}, pior {worst:.2})"
    );
}

/// A page of body text with `count` pictures standing in the middle of it.
fn wrapped(count: usize) -> Document {
    let wrap = r#""wrap": { "mode": { "kind": "box" }, "padding": 6 },"#;
    let photos: Vec<String> = (0..count)
        .map(|index| {
            format!(
                r#"{{ "type": "image", "src": "ausente.png", {wrap}
                     "rect": [180, {}, 120, 60] }}"#,
                80 + index * 90,
            )
        })
        .collect();

    let body = "Um parágrafo de corpo, comprido o bastante para ocupar a coluna \
        inteira e ter de contornar tudo o que estiver no caminho, várias vezes. "
        .repeat(24);

    let text = format!(
        r#"{{ "type": "text", "rect": [56, 80, 483, 700],
              "style": {{ "textAlign": "justify" }},
              "blocks": ["{body}"] }}"#
    );
    let mut frames = photos;
    frames.push(text);

    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "pages": [{{ "frames": [{}] }}]
        }}"#,
        frames.join(",\n")
    ))
    .expect("fixture válida")
}
