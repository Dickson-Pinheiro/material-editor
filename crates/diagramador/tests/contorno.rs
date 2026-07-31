//! Where the text actually lands, shape by shape.
//!
//! Wrap geometry fails quietly. A polygon read one row off, a gap carved on
//! the wrong side, a band queried at the wrong height — none of it panics,
//! none of it breaks a unit test, and all of it looks wrong on the page. The
//! only honest guard is to write down where every line went and notice when
//! that changes.
//!
//! The record is glyph positions rather than pixels: they *are* the geometry,
//! they are exact, and a diff of them reads.
//!
//! Rewrite the record after an intended change:
//!
//! ```sh
//! ATUALIZAR=1 cargo test --test contorno --no-default-features --features images
//! ```

use std::fmt::Write as _;

use diagramador::display::{DisplayItem, DisplayList, GlyphRun};
use diagramador::spec::{Document, FontWeight};
use diagramador::Engine;

const GOLDEN: &str = include_str!("contorno.golden");

/// Lines are recorded to this many decimals. Finer is noise between builds.
const PLACES: usize = 2;

fn engine() -> Option<Engine> {
    let mut engine = Engine::new();
    for (file, weight) in [("DejaVuSans.ttf", 400u16), ("DejaVuSans-Bold.ttf", 700)] {
        let bytes = std::fs::read(format!("../../fonts/{file}"))
            .or_else(|_| std::fs::read(format!("fonts/{file}")))
            .ok()?;
        engine
            .add_font("corpo", bytes, Some(FontWeight(weight)), Some(false))
            .ok()?;
    }
    Some(engine)
}

/// The body text every case wraps around. Long enough to pass a whole shape.
const BODY: &str = "Um parágrafo de corpo suficientemente longo para atravessar a \
forma inteira, linha após linha, e mostrar onde cada trecho começa e termina em \
relação a ela. O texto continua para além do contorno, de modo que as linhas de \
baixo voltem a usar a coluna toda e a diferença fique registada.";

/// A page with one shape standing in the middle of a column of text.
fn page(wrap: &str) -> Document {
    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "pages": [{{ "frames": [
                {{ "type": "image", "src": "ausente.png", "rect": [200, 120, 140, 180],
                   {wrap} }},
                {{ "type": "text", "rect": [56, 100, 483, 500],
                   "blocks": ["{BODY}"] }}
            ] }}]
        }}"#
    ))
    .expect("fixture válida")
}

/// Rotated, so the case exercises the transform rather than the ring alone.
fn turned(wrap: &str, degrees: f64) -> Document {
    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "pages": [{{ "frames": [
                {{ "type": "image", "src": "ausente.png", "rect": [200, 120, 140, 180],
                   "rotation": {degrees}, {wrap} }},
                {{ "type": "text", "rect": [56, 100, 483, 500],
                   "blocks": ["{BODY}"] }}
            ] }}]
        }}"#
    ))
    .expect("fixture válida")
}

fn contour(points: &str, padding: f64) -> String {
    format!(r#""wrap": {{ "mode": {{ "kind": "contour", "points": {points} }}, "padding": {padding} }}"#)
}

/// A circle, as a ring of 24 points.
fn circle() -> String {
    let points: Vec<String> = (0..24)
        .map(|step| {
            let angle = std::f64::consts::TAU * f64::from(step) / 24.0;
            format!("[{:.4},{:.4}]", 0.5 + 0.5 * angle.cos(), 0.5 + 0.5 * angle.sin())
        })
        .collect();
    contour(&format!("[{}]", points.join(",")), 6.0)
}

fn cases() -> Vec<(&'static str, Document)> {
    let box_wrap = r#""wrap": { "mode": { "kind": "box" }, "padding": 6 }"#;

    vec![
        ("caixa (controlo)", page(box_wrap)),
        ("círculo", page(&circle())),
        (
            "triângulo, ápice em cima",
            page(&contour("[[0.5,0],[1,1],[0,1]]", 6.0)),
        ),
        (
            "triângulo, ápice em baixo",
            page(&contour("[[0,0],[1,0],[0.5,1]]", 6.0)),
        ),
        (
            "côncavo em C, boca à direita",
            page(&contour(
                "[[0,0],[1,0],[1,0.2],[0.3,0.2],[0.3,0.8],[1,0.8],[1,1],[0,1]]",
                6.0,
            )),
        ),
        (
            "dois braços, entalhe no meio",
            page(&contour(
                "[[0,0],[0.3,0],[0.3,0.7],[0.7,0.7],[0.7,0],[1,0],[1,1],[0,1]]",
                4.0,
            )),
        ),
        (
            "serrilha fina",
            page(&contour(
                "[[0,0],[1,0],[1,1],[0.8,0.6],[0.6,1],[0.4,0.6],[0.2,1],[0,0.6]]",
                4.0,
            )),
        ),
        ("caixa rodada 15°", turned(box_wrap, 15.0)),
        ("triângulo rodado 15°", turned(&contour("[[0.5,0],[1,1],[0,1]]", 6.0), 15.0)),
        ("sem folga", page(r#""wrap": { "mode": { "kind": "box" }, "padding": 0 }"#)),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────

fn runs(list: &DisplayList) -> Vec<GlyphRun> {
    fn walk(items: &[DisplayItem], out: &mut Vec<GlyphRun>) {
        for item in items {
            match item {
                DisplayItem::Glyphs(run) => out.push(run.clone()),
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for page in &list.pages {
        walk(&page.items, &mut out);
    }
    out
}

/// Right edge of a run without its trailing space, which hangs by design.
fn visible_right(run: &GlyphRun) -> f64 {
    let trimmed = run.text.trim_end().len();
    run.glyphs
        .iter()
        .filter(|glyph| (glyph.cluster as usize) < trimmed)
        .map(|glyph| run.x + glyph.x + glyph.advance)
        .fold(run.x, f64::max)
}

/// One line per baseline: its `y`, then every stretch of text on it.
fn record(name: &str, list: &DisplayList) -> String {
    let mut lines = runs(list);
    lines.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));

    let mut out = format!("# {name}\n");
    let mut current = f64::NEG_INFINITY;
    let mut row = String::new();

    for run in &lines {
        if (run.y - current).abs() > 0.01 {
            if !row.is_empty() {
                let _ = writeln!(out, "{row}");
            }
            row = format!("  y={:>8.PLACES$} ", run.y);
            current = run.y;
        } else {
            row.push_str(" | ");
        }
        let _ = write!(row, "{:.PLACES$}..{:.PLACES$}", run.x, visible_right(run));
    }
    if !row.is_empty() {
        let _ = writeln!(out, "{row}");
    }
    out
}

#[test]
fn every_shape_puts_the_text_where_it_did_before() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let mut produced = String::new();
    for (name, document) in cases() {
        produced.push_str(&record(name, &engine.layout(&document)));
        produced.push('\n');
    }

    if std::env::var("ATUALIZAR").is_ok() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/contorno.golden");
        std::fs::write(path, &produced).expect("gravar o registo");
        eprintln!("registo reescrito: {path}");
        return;
    }

    if produced != GOLDEN {
        let expected: Vec<&str> = GOLDEN.lines().collect();
        let actual: Vec<&str> = produced.lines().collect();
        let mut report = String::from("o texto mudou de lugar:\n");
        for index in 0..expected.len().max(actual.len()) {
            let before = expected.get(index).copied().unwrap_or("<ausente>");
            let after = actual.get(index).copied().unwrap_or("<ausente>");
            if before != after {
                let _ = writeln!(report, "  linha {}\n    antes: {before}\n    agora: {after}", index + 1);
            }
        }
        panic!("{report}");
    }
}

/// The corpus is only worth what it covers: every case has to wrap something.
#[test]
fn every_case_actually_bends_the_text() {
    let Some(engine) = engine() else { return };

    let plain = engine.layout(&serde_json::from_str::<Document>(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "pages": [{{ "frames": [
                {{ "type": "text", "rect": [56, 100, 483, 500], "blocks": ["{BODY}"] }}
            ] }}]
        }}"#
    )).unwrap());
    let straight = record("liso", &plain);

    for (name, document) in cases() {
        let wrapped = record(name, &engine.layout(&document));
        assert_ne!(
            wrapped.trim_start_matches(|c| c != '\n'),
            straight.trim_start_matches(|c| c != '\n'),
            "o caso «{name}» não moveu uma linha sequer — não está a testar contorno nenhum",
        );
    }
}
