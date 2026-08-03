//! The parity contract.
//!
//! The browser paints the display list; the PDF emitter writes the same display
//! list. The claim that they agree is only worth anything if it is checked, so
//! this test reads the coordinates back out of the generated PDF and compares
//! them, number by number, against the ones the layout engine produced.
//!
//! Content streams are written uncompressed, which is what makes this possible
//! without a PDF parser.

use diagramador::color::Color;
use diagramador::display::{
    DisplayItem, DisplayList, FillRule, GlyphRun, PathCommand, PathItem, Stroke,
};
use diagramador::spec::{Document, FontWeight};
use diagramador::{Engine, ImageStore};

/// Positions must match to within a hundredth of a point.
const TOLERANCE: f64 = 0.01;

fn engine() -> Option<Engine> {
    let mut engine = Engine::new();
    for (file, weight, italic) in [
        ("DejaVuSans.ttf", 400u16, false),
        ("DejaVuSans-Bold.ttf", 700, false),
    ] {
        let bytes = std::fs::read(format!("../../fonts/{file}"))
            .or_else(|_| std::fs::read(format!("fonts/{file}")))
            .ok()?;
        engine
            .add_font("corpo", bytes, Some(FontWeight(weight)), Some(italic))
            .ok()?;
    }
    Some(engine)
}

fn document() -> Document {
    serde_json::from_str(
        r##"{
            "meta": { "title": "Paridade", "language": "pt-BR" },
            "page": { "size": "A4", "margins": "20mm" },
            "style": { "fontFamily": "corpo", "fontSize": 11 },
            "pages": [{
                "frames": [
                    { "id": "titulo", "type": "text", "rect": ["20mm", "20mm", "170mm", "30mm"],
                      "blocks": [{ "type": "paragraph",
                                   "style": { "fontSize": 24, "fontWeight": "bold" },
                                   "content": ["Fotossíntese"] }] },
                    { "id": "corpo", "type": "text", "rect": ["20mm", "55mm", "80mm", "120mm"],
                      "style": { "textAlign": "justify" },
                      "blocks": [
                        { "type": "paragraph", "content": [
                            "As plantas convertem a luz solar em energia química através da ",
                            { "type": "text", "text": "fotossíntese", "style": { "fontWeight": "bold" } },
                            ", processo essencial para a vida no planeta."
                        ]},
                        { "type": "paragraph", "marker": { "text": "a)" },
                          "content": ["primeira alternativa"] }
                      ]},
                    { "id": "caixa", "type": "shape", "shape": "rect",
                      "rect": ["110mm", "55mm", "80mm", "40mm"],
                      "fill": "#eef4fb", "border": { "width": 1, "color": "#1f4e79" }, "radius": 6 },
                    { "id": "faixa", "type": "shape", "shape": "rect",
                      "rect": [100, 700, 200, 12], "fill": "#1f4e79" },

                    { "id": "quadro", "type": "text",
                      "rect": ["110mm", "100mm", "80mm", "45mm"],
                      "blocks": [{
                        "type": "table",
                        "columns": ["auto", "1fr"],
                        "inset": 4,
                        "header": { "rows": 1 },
                        "stripe": { "every": 2, "offset": 1, "fill": "#f2f4f7" },
                        "lines": [
                          { "axis": "horizontal", "at": 1, "width": 1, "color": "#1f4e79" },
                          { "axis": "vertical", "at": 1, "width": 0.5, "color": "#cbd5e1" }
                        ],
                        "cells": [
                          { "blocks": ["Estado"], "fill": "#1f4e79",
                            "style": { "color": "#ffffff", "fontWeight": "bold" } },
                          { "blocks": ["Mudança"], "fill": "#1f4e79",
                            "style": { "color": "#ffffff", "fontWeight": "bold" } },
                          { "blocks": ["Sólido"] }, { "blocks": ["fusão"] },
                          { "blocks": ["Líquido"] }, { "blocks": ["vaporização"] }
                        ]
                      }]
                    },

                    { "id": "grafico", "type": "chart",
                      "rect": ["20mm", "180mm", "80mm", "50mm"],
                      "mark": "bar",
                      "data": [
                        { "mes": "jan", "v": 12, "regiao": "norte" },
                        { "mes": "fev", "v": 19, "regiao": "norte" },
                        { "mes": "jan", "v": 8, "regiao": "sul" },
                        { "mes": "fev", "v": 14, "regiao": "sul" }
                      ],
                      "encoding": {
                        "x": { "field": "mes", "kind": "categorical" },
                        "y": { "field": "v", "title": "Vendas" },
                        "color": { "field": "regiao" }
                      },
                      "axes": { "y": { "grid": true }, "x": { "title": "" } },
                      "legend": { "position": "bottom" }
                    }
                ]
            }]
        }"##,
    )
    .expect("fixture parses")
}

/// A page whose text runs down both sides of a picture.
///
/// The picture's own bytes are never registered: an image that fails to load
/// still occupies its rectangle, which is all a wrap needs. Keeping the
/// fixture free of binary assets keeps this test readable.
fn wrapped_document() -> Document {
    serde_json::from_str(
        r##"{
            "page": { "size": "A4", "margins": "20mm" },
            "style": { "fontFamily": "corpo", "fontSize": 10 },
            "pages": [{
                "frames": [
                    { "id": "foto", "type": "image", "src": "ausente.png",
                      "rect": [200, 80, 120, 60],
                      "wrap": { "mode": { "kind": "box" }, "padding": 6 } },
                    { "id": "corpo", "type": "text", "rect": [56, 80, 440, 400],
                      "style": { "textAlign": "justify" },
                      "blocks": ["Um parágrafo bem comprido que precisa correr dos dois lados da fotografia posta no meio da coluna e depois seguir usando a largura inteira até o fim."] }
                ]
            }]
        }"##,
    )
    .expect("fixture parses")
}

// ─────────────────────────────────────────────────────────────────────────────
// PDF content stream extraction
// ─────────────────────────────────────────────────────────────────────────────

/// Concatenate every uncompressed `stream … endstream` body that looks like a
/// content stream (i.e. contains text or path operators).
fn content_streams(pdf: &[u8]) -> String {
    let mut out = String::new();
    let mut cursor = 0usize;

    while let Some(start) = find(pdf, b"stream", cursor) {
        let body_start = match pdf.get(start + 6) {
            Some(b'\r') => start + 8, // CRLF
            Some(b'\n') => start + 7,
            _ => start + 6,
        };
        let Some(end) = find(pdf, b"endstream", body_start) else {
            break;
        };
        if let Ok(text) = std::str::from_utf8(&pdf[body_start..end])
            && (text.contains("BT") || text.contains(" re"))
        {
            out.push_str(text);
            out.push('\n');
        }
        cursor = end + 9;
    }

    out
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|index| index + from)
}

/// Every `a b c d e f Tm` in the stream, as `(e, f)` — the text origin.
fn text_origins(stream: &str) -> Vec<(f64, f64)> {
    let tokens: Vec<&str> = stream.split_whitespace().collect();
    let mut out = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if *token != "Tm" || index < 6 {
            continue;
        }
        let x = tokens[index - 2].parse::<f64>();
        let y = tokens[index - 1].parse::<f64>();
        if let (Ok(x), Ok(y)) = (x, y) {
            out.push((x, y));
        }
    }

    out
}

fn glyph_runs(list: &DisplayList) -> Vec<GlyphRun> {
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

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// Every run the browser would paint appears in the PDF at the same place.
#[test]
fn every_glyph_run_lands_at_its_display_list_position() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let document = document();
    let list = engine.layout(&document);
    let pdf = engine
        .render_display_list(&list, &document)
        .expect("render succeeds");

    let runs = glyph_runs(&list);
    assert!(runs.len() >= 5, "fixture should produce several runs");

    let origins = text_origins(&content_streams(&pdf));
    assert!(!origins.is_empty(), "no text matrices found in the PDF");

    let page_height = list.pages[0].height;

    for run in &runs {
        // The display list measures y downward from the top of the page; PDF
        // measures it upward from the bottom.
        let expected = (run.x, page_height - run.y);

        let matched = origins.iter().any(|(x, y)| {
            (x - expected.0).abs() < TOLERANCE && (y - expected.1).abs() < TOLERANCE
        });

        assert!(
            matched,
            "run {:?} at ({:.3}, {:.3}) has no matching Tm at ({:.3}, {:.3})",
            run.text, run.x, run.y, expected.0, expected.1
        );
    }
}

/// The PDF carries one text matrix per run — no stray, no missing.
#[test]
fn the_pdf_contains_exactly_one_matrix_per_run() {
    let Some(engine) = engine() else { return };

    let document = document();
    let list = engine.layout(&document);
    let pdf = engine.render_display_list(&list, &document).unwrap();

    assert_eq!(
        text_origins(&content_streams(&pdf)).len(),
        glyph_runs(&list).len(),
        "text matrices and glyph runs must correspond one to one"
    );
}

/// Advances survive the trip: the sum of a run's TJ offsets equals its width.
#[test]
fn run_widths_agree_with_the_sum_of_their_advances() {
    let Some(engine) = engine() else { return };

    let list = engine.layout(&document());
    for run in glyph_runs(&list) {
        let total: f64 = run.glyphs.iter().map(|glyph| glyph.advance).sum();
        assert!(
            (total - run.width).abs() < TOLERANCE,
            "run {:?}: advances sum to {total}, width says {}",
            run.text,
            run.width
        );

        // Glyph x offsets must be the running total of the advances, which is
        // what lets the browser hit-test with a single scan.
        let mut pen = 0.0;
        for glyph in &run.glyphs {
            assert!(
                (glyph.x - pen).abs() < TOLERANCE,
                "run {:?}: glyph at {} breaks the running total {pen}",
                run.text,
                glyph.x
            );
            pen += glyph.advance;
        }
    }
}

/// Shapes are placed with the same flip as text.
///
/// Square-cornered rectangles come out as a single `re` operator, so their
/// coordinates can be read straight back and compared.
#[test]
fn rectangles_land_at_their_display_list_position() {
    let Some(engine) = engine() else { return };

    let document = document();
    let list = engine.layout(&document);
    let pdf = engine.render_display_list(&list, &document).unwrap();
    let emitted = rect_operators(&content_streams(&pdf));
    assert!(!emitted.is_empty(), "no `re` operators in the content stream");

    let page_height = list.pages[0].height;

    fn walk(items: &[DisplayItem], out: &mut Vec<diagramador::display::RectItem>) {
        for item in items {
            match item {
                DisplayItem::Rect(rect) => out.push(rect.clone()),
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }
    let mut rects = Vec::new();
    walk(&list.pages[0].items, &mut rects);

    // Only the ones that put ink on the page. A table also emits a box per
    // cell — no fill, no stroke — saying where the cell is so the editor can
    // place a caret in an empty one. Both emitters skip it, and parity is
    // about what is painted.
    let square: Vec<_> = rects
        .iter()
        .filter(|rect| rect.radius == 0.0)
        .filter(|rect| rect.fill.is_some() || rect.stroke.is_some())
        .collect();
    assert!(!square.is_empty(), "fixture should contain a square-cornered rect");

    for rect in square {
        let expected = (
            rect.rect.x,
            page_height - rect.rect.y - rect.rect.h,
            rect.rect.w,
            rect.rect.h,
        );
        let matched = emitted.iter().any(|(x, y, w, h)| {
            (x - expected.0).abs() < TOLERANCE
                && (y - expected.1).abs() < TOLERANCE
                && (w - expected.2).abs() < TOLERANCE
                && (h - expected.3).abs() < TOLERANCE
        });
        assert!(
            matched,
            "rect at {:?} has no matching `re` at {expected:?}",
            rect.rect
        );
    }
}

/// Every `x y w h re` in the stream.
fn rect_operators(stream: &str) -> Vec<(f64, f64, f64, f64)> {
    let tokens: Vec<&str> = stream.split_whitespace().collect();
    let mut out = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if *token != "re" || index < 4 {
            continue;
        }
        let parsed: Option<Vec<f64>> = tokens[index - 4..index]
            .iter()
            .map(|value| value.parse::<f64>().ok())
            .collect();
        if let Some(values) = parsed {
            out.push((values[0], values[1], values[2], values[3]));
        }
    }

    out
}

/// Text pushed aside by a wrap reaches the PDF where the display list put it.
///
/// The wrap only ever moves numbers inside the display list, so in principle
/// the emitter cannot tell. That is exactly why it is worth checking: the
/// claim that the two outputs cannot diverge is only worth what it is tested
/// at, and a line split into two pieces is the shape most likely to expose an
/// emitter that assumed one run per line.
#[test]
fn wrapped_text_lands_at_its_display_list_position() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let document = wrapped_document();
    let list = engine.layout(&document);
    let runs = glyph_runs(&list);

    // Guard against a fixture that quietly stops wrapping: without a line in
    // two pieces this test would pass while proving nothing.
    let split = runs.iter().any(|run| {
        runs.iter()
            .any(|other| (other.y - run.y).abs() < TOLERANCE && other.x != run.x)
    });
    assert!(split, "a fixture precisa produzir ao menos uma linha partida");

    let pdf = engine
        .render_display_list(&list, &document)
        .expect("render succeeds");
    let origins = text_origins(&content_streams(&pdf));

    assert_eq!(
        origins.len(),
        runs.len(),
        "cada trecho de cada lado da foto precisa da sua própria matriz"
    );

    let page_height = list.pages[0].height;
    for run in &runs {
        let expected = (run.x, page_height - run.y);
        let matched = origins.iter().any(|(x, y)| {
            (x - expected.0).abs() < TOLERANCE && (y - expected.1).abs() < TOLERANCE
        });
        assert!(
            matched,
            "trecho {:?} em ({:.3}, {:.3}) não tem Tm correspondente em ({:.3}, {:.3})",
            run.text, run.x, run.y, expected.0, expected.1
        );
    }
}

/// A wrapped document lays out the same way twice, like any other.
#[test]
fn wrapped_layout_is_deterministic() {
    let Some(engine) = engine() else { return };
    let document = wrapped_document();

    let first = serde_json::to_string(&engine.layout(&document)).unwrap();
    let second = serde_json::to_string(&engine.layout(&document)).unwrap();
    assert_eq!(first, second);
}

/// An outline reaches the PDF at the coordinates the display list gave it.
///
/// The path primitive is the only item no layout produces yet, so it is
/// exercised by building a display list by hand and rendering it. That is on
/// purpose: the contract between the display list and the two emitters has to
/// hold before anything depends on it, not after.
#[test]
fn a_path_reaches_the_pdf_where_the_display_list_put_it() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let document = document();
    let mut list = engine.layout(&document);
    let height = list.pages[0].height;

    // A filled triangle and a stroked L, in page coordinates, y down.
    let triangle = PathItem {
        commands: vec![
            PathCommand::MoveTo { x: 100.0, y: 200.0 },
            PathCommand::LineTo { x: 180.0, y: 340.0 },
            PathCommand::LineTo { x: 20.0, y: 340.0 },
            PathCommand::Close,
        ],
        fill: Some(Color::rgb(0.12, 0.31, 0.47)),
        stroke: None,
        fill_rule: FillRule::NonZero,
        source: None,
    };
    let bend = PathItem {
        commands: vec![
            PathCommand::MoveTo { x: 300.0, y: 200.0 },
            PathCommand::LineTo { x: 300.0, y: 300.0 },
            PathCommand::LineTo { x: 400.0, y: 300.0 },
        ],
        fill: None,
        stroke: Some(Stroke { color: Color::rgb(0.88, 0.27, 0.48), width: 2.0, dash: None }),
        fill_rule: FillRule::NonZero,
        source: None,
    };

    list.pages[0].items.push(DisplayItem::Path(triangle.clone()));
    list.pages[0].items.push(DisplayItem::Path(bend.clone()));

    let pdf = engine
        .render_display_list(&list, &document)
        .expect("render succeeds");
    let stream = content_streams(&pdf);

    // Every point of both paths, flipped, must appear as a path operator.
    for path in [&triangle, &bend] {
        for command in &path.commands {
            let (x, y) = match *command {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => (x, y),
                PathCommand::CurveTo { x, y, .. } => (x, y),
                PathCommand::Close => continue,
            };
            let expected = (x, height - y);
            assert!(
                point_appears(&stream, expected),
                "ponto {expected:?} não aparece no fluxo de conteúdo",
            );
        }
    }

    // Filled and stroked are different operators, and using the wrong one
    // would still put the points in the right place.
    // Operators sit on their own line. Filled and stroked are different ones,
    // and the wrong one would still put every point in the right place — so
    // checking the coordinates alone would pass on a path painted invisibly.
    let operators: Vec<&str> = stream.lines().map(str::trim).collect();
    assert!(operators.contains(&"f"), "o triângulo tem de ser preenchido");
    assert!(operators.contains(&"S"), "o L tem de ser traçado");
    assert!(operators.contains(&"h"), "o triângulo tem de fechar");
}

/// Whether some `x y` pair in the stream matches, within tolerance.
fn point_appears(stream: &str, expected: (f64, f64)) -> bool {
    let tokens: Vec<&str> = stream.split_whitespace().collect();
    tokens.windows(2).any(|pair| {
        match (pair[0].parse::<f64>(), pair[1].parse::<f64>()) {
            (Ok(x), Ok(y)) => {
                (x - expected.0).abs() < TOLERANCE && (y - expected.1).abs() < TOLERANCE
            }
            _ => false,
        }
    })
}

/// The corpus really does contain a table and a chart.
///
/// Every other test here walks whatever the fixture produced, so a fixture
/// that quietly stopped producing a table would leave them all passing and
/// checking nothing about one. This is the guard on that: the parity contract
/// is only worth what the corpus covers.
#[test]
fn the_corpus_carries_a_table_and_a_chart() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };
    let list = engine.layout(&document());
    let runs = glyph_runs(&list);

    for word in ["Estado", "Mudança", "Sólido", "vaporização"] {
        assert!(runs.iter().any(|run| run.text == word), "a tabela pintou `{word}`");
    }
    for word in ["jan", "fev", "Vendas", "norte"] {
        assert!(runs.iter().any(|run| run.text == word), "o gráfico pintou `{word}`");
    }

    // Text inside a cell says which cell, which is what the editor writes back
    // through — and what no other test in this file would notice the loss of.
    let cell = runs
        .iter()
        .find(|run| run.text == "Sólido")
        .and_then(|run| run.source.clone())
        .expect("proveniência");
    assert!(!cell.cells.is_empty(), "com o rasto de células: {cell:?}");
}

/// The same document renders identically twice — no map iteration order or
/// timestamp leaking into the bytes.
#[test]
fn output_is_reproducible() {
    let Some(engine) = engine() else { return };
    let document = document();

    let first = engine.render_pdf(&document).unwrap();
    let second = engine.render_pdf(&document).unwrap();
    assert_eq!(first, second);
}

/// Laying out twice gives the same display list, which is what makes the editor
/// safe to re-render on every keystroke.
#[test]
fn layout_is_deterministic() {
    let Some(engine) = engine() else { return };
    let document = document();

    let first = serde_json::to_string(&engine.layout(&document)).unwrap();
    let second = serde_json::to_string(&engine.layout(&document)).unwrap();
    assert_eq!(first, second);
}

/// Every `a b c d e f cm` in the stream.
fn transforms(stream: &str) -> Vec<[f64; 6]> {
    let tokens: Vec<&str> = stream.split_whitespace().collect();
    let mut out = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if *token != "cm" || index < 6 {
            continue;
        }
        let mut matrix = [0.0f64; 6];
        let mut ok = true;
        for (slot, raw) in matrix.iter_mut().zip(&tokens[index - 6..index]) {
            match raw.parse::<f64>() {
                Ok(value) => *slot = value,
                Err(_) => ok = false,
            }
        }
        if ok {
            out.push(matrix);
        }
    }

    out
}

/// A chart's turned axis title lands in the PDF where the display list put it.
///
/// The title of a vertical axis is the first thing the engine draws inside a
/// coordinate space of its own: a group carrying a quarter turn. Everything
/// else is written straight into page space, so this is the one place where
/// the PDF and the browser could diverge while each looks plausible alone —
/// the position is composed by two pieces of code that never meet.
///
/// The composition is done here the way a reader's viewer does it, and not by
/// calling back into the emitter's own conversion: a test that reimplements
/// the code it checks proves nothing.
#[test]
fn a_turned_axis_title_lands_where_the_display_list_put_it() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let document: Document = serde_json::from_str(
        r##"{
            "meta": { "title": "Eixo", "language": "pt-BR" },
            "page": { "size": "A4", "margins": "20mm" },
            "style": { "fontFamily": "corpo", "fontSize": 10 },
            "pages": [{
                "frames": [
                    { "id": "g", "type": "chart", "rect": [60, 60, 300, 200],
                      "data": [{ "mes": "jan", "v": 8 }],
                      "encoding": { "x": { "field": "mes", "kind": "categorical" },
                                    "y": { "field": "v" } },
                      "axes": { "y": { "title": "Vendas" }, "x": { "title": "" } } }
                ]
            }]
        }"##,
    )
    .expect("documento válido");

    let list = engine.layout(&document);
    let height = list.pages[0].height;

    // The only group carrying a matrix on this page is the turned title.
    let turned = list.pages[0]
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Group(group) => group.transform.map(|matrix| (group, matrix)),
            _ => None,
        })
        .expect("o título rodado");
    let run = turned
        .0
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Glyphs(run) => Some(run),
            _ => None,
        })
        .expect("o texto do título");
    assert_eq!(run.text, "Vendas");

    // Where the browser paints the run's origin: the group's matrix applied to
    // the run's own coordinates, in page space with y growing down.
    let [a, b, c, d, e, f] = turned.1;
    let page = (a * run.x + c * run.y + e, b * run.x + d * run.y + f);

    let pdf = engine
        .render_display_list(&list, &document)
        .expect("render succeeds");
    let stream = content_streams(&pdf);

    let matrix = *transforms(&stream)
        .first()
        .expect("uma matriz de grupo no fluxo");

    // Where the PDF puts it: the emitted `cm` applied to the emitted `Tm`.
    let landed = text_origins(&stream).into_iter().any(|(x, y)| {
        let placed = (
            matrix[0] * x + matrix[2] * y + matrix[4],
            matrix[1] * x + matrix[3] * y + matrix[5],
        );
        (placed.0 - page.0).abs() < TOLERANCE
            && (placed.1 - (height - page.1)).abs() < TOLERANCE
    });
    assert!(
        landed,
        "nenhuma origem de texto cai em {:?} depois da matriz {matrix:?}; origens: {:?}",
        (page.0, height - page.1),
        text_origins(&stream),
    );

    // A quarter turn and not a mirror of one. The sense has to survive the
    // flip, and reversing it would land the origin on the very same point.
    assert!(matrix[1].abs() > 0.5 && matrix[2].abs() > 0.5, "matriz {matrix:?}");
    assert!(
        matrix[1] * matrix[2] < 0.0,
        "os termos fora da diagonal têm sinais opostos: {matrix:?}",
    );
}

/// A document with no fonts still lays out, so the editor can show it.
#[test]
fn layout_degrades_without_fonts() {
    let engine = Engine::new();
    let list = engine.layout(&document());

    assert_eq!(list.pages.len(), 1);
    assert!(list.has_errors(), "missing fonts should be reported");
    // But rendering a PDF without fonts is refused rather than silently blank.
    assert!(engine.render_pdf(&document()).is_err());
    let _ = ImageStore::new();
}
