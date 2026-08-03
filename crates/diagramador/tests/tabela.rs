//! Where a table actually puts things, case by case.
//!
//! Table geometry fails the same quiet way wrap geometry does: a column
//! resolved a point narrow, a rule on the wrong side of a gap, a spanning cell
//! that grew the row above instead of the row below — none of it panics, and
//! all of it looks wrong on the page. So the same guard: write down where
//! every glyph, fill and rule went, and notice when that changes.
//!
//! Rewrite the record after an intended change:
//!
//! ```sh
//! ATUALIZAR=1 cargo test --test tabela --no-default-features --features images
//! ```

use std::fmt::Write as _;

use diagramador::display::{DisplayItem, DisplayList};
use diagramador::spec::{Document, FontWeight};
use diagramador::Engine;

const GOLDEN: &str = include_str!("tabela.golden");

/// Positions are recorded to this many decimals. Finer is noise between builds.
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

/// A page holding one table, in a frame wide enough to be interesting.
fn page(table: &str) -> Document {
    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "pages": [{{ "frames": [
                {{ "type": "text", "rect": [56, 60, 400, 600], "blocks": [{table}] }}
            ] }}]
        }}"#
    ))
    .expect("fixture válida")
}

/// Cells written as plain strings, which is the shape an author writes.
fn cells(labels: &[&str]) -> String {
    let each: Vec<String> = labels
        .iter()
        .map(|label| format!(r#"{{ "blocks": ["{label}"] }}"#))
        .collect();
    each.join(",")
}

fn cases() -> Vec<(&'static str, Document)> {
    vec![
        (
            "três por três, tudo automático",
            page(&format!(
                r#"{{ "type": "table", "columns": ["auto","auto","auto"],
                      "inset": 4, "cells": [{}] }}"#,
                cells(&["Espécie", "Profundidade", "Peso", "Cachalote", "2000 m",
                        "40 t", "Baleia-azul", "100 m", "150 t"]),
            )),
        ),
        (
            "cabeçalho que atravessa duas colunas",
            page(&format!(
                r#"{{ "type": "table", "columns": ["auto","auto","auto"], "inset": 4,
                      "cells": [
                        {{ "colspan": 2, "blocks": ["Mergulho registado"] }},
                        {{ "blocks": ["Notas"] }},
                        {}
                      ] }}"#,
                cells(&["Cachalote", "2000 m", "verificado", "Foca", "700 m", "estimado"]),
            )),
        ),
        (
            "célula que atravessa duas linhas",
            page(&format!(
                r#"{{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                      "cells": [
                        {{ "rowspan": 2, "blocks": ["Mamíferos que mergulham fundo e por muito tempo"] }},
                        {{ "blocks": ["Cachalote"] }},
                        {{ "blocks": ["Baleia-bicuda"] }},
                        {}
                      ] }}"#,
                cells(&["Aves", "Pinguim-imperador"]),
            )),
        ),
        (
            "réguas à booktabs",
            page(&format!(
                r#"{{ "type": "table", "columns": ["auto","auto"], "inset": 5,
                      "lines": [
                        {{ "axis": "horizontal", "at": 0, "width": 1.2 }},
                        {{ "axis": "horizontal", "at": 1, "width": 0.5 }},
                        {{ "axis": "horizontal", "at": 3, "width": 1.2 }}
                      ],
                      "cells": [{}] }}"#,
                cells(&["Espécie", "Profundidade", "Cachalote", "2000 m",
                        "Baleia-azul", "100 m"]),
            )),
        ),
        (
            "zebrado",
            page(&format!(
                r##"{{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                      "fill": "#f7fafc",
                      "stripe": {{ "every": 2, "offset": 1, "fill": "#e2e8f0" }},
                      "cells": [{}] }}"##,
                cells(&["a", "1", "b", "2", "c", "3", "d", "4"]),
            )),
        ),
        (
            "pistas misturadas: fixa, fracção, percentagem",
            page(&format!(
                r#"{{ "type": "table", "columns": [80, "1fr", "25%"], "inset": 4,
                      "cells": [{}] }}"#,
                cells(&["fixa", "o resto, que é bastante e por isso quebra em várias linhas",
                        "um quarto", "b", "c", "d"]),
            )),
        ),
        (
            "célula com três parágrafos",
            page(
                r#"{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                      "cells": [
                        { "blocks": ["primeiro", "segundo", "terceiro"] },
                        { "blocks": ["ao lado"] }
                      ] }"#,
            ),
        ),
        (
            "intervalos entre pistas",
            page(&format!(
                r#"{{ "type": "table", "columns": ["auto","auto"], "inset": 3,
                      "columnGap": 12, "rowGap": 8,
                      "lines": [{{ "axis": "vertical", "at": 1, "width": 0.5 }}],
                      "cells": [{}] }}"#,
                cells(&["esquerda", "direita", "abaixo", "também"]),
            )),
        ),
        (
            "palavra mais larga que a coluna",
            page(&format!(
                r#"{{ "type": "table", "columns": [40, 40], "inset": 2,
                      "cells": [{}] }}"#,
                cells(&["incompreensibilíssimo", "b"]),
            )),
        ),
        (
            // Multi-word cells inside, so the inner table's width is a thing
            // the outer column has to be told about: one-letter cells cannot
            // shrink, and a case that cannot shrink cannot notice a mistake.
            "tabela dentro de célula",
            page(
                r#"{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                      "cells": [
                        { "blocks": ["fora"] },
                        { "blocks": [
                            { "type": "table", "columns": ["auto","auto"], "inset": 2,
                              "cells": [
                                { "blocks": ["camada superficial"] },
                                { "blocks": ["até 200 metros"] },
                                { "blocks": ["zona abissal"] },
                                { "blocks": ["abaixo de 4000"] }
                              ] }
                        ] }
                      ] }"#,
            ),
        ),
        (
            // A table is a block among blocks: what precedes it pushes it
            // down, and what follows starts below it.
            "texto antes e depois",
            page(&format!(
                r#""Uma frase que ocupa a linha inteira antes da tabela começar.",
                   {{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                      "lines": [{{ "axis": "horizontal", "at": 0, "width": 1 }}],
                      "cells": [{}] }},
                   "E outra frase depois dela, que tem de começar abaixo.""#,
                cells(&["Espécie", "Peso", "Cachalote", "40 t"]),
            )),
        ),
        (
            "os quatro alinhamentos, lado a lado",
            page(
                r#"{ "type": "table", "columns": [70, 70, 70, 70, 90], "inset": 4,
                      "cells": [
                        { "verticalAlign": "top", "blocks": ["topo"] },
                        { "verticalAlign": "middle", "blocks": ["meio"] },
                        { "verticalAlign": "bottom", "blocks": ["base"] },
                        { "verticalAlign": "baseline", "blocks": ["linha"] },
                        { "blocks": ["uma célula alta o suficiente para dar espaço aos outros"] }
                      ] }"#,
            ),
        ),
        (
            // Different type sizes in the same row: the case baseline exists
            // for. Aligned at the top, the small text would float above the
            // large; aligned by baseline, the two sit on the same line.
            "linha de base com corpos diferentes",
            page(
                r#"{ "type": "table", "columns": ["auto","auto","auto"], "inset": 4,
                      "cells": [
                        { "verticalAlign": "baseline",
                          "blocks": [{ "type": "paragraph", "style": { "fontSize": 22 }, "content": ["2000"] }] },
                        { "verticalAlign": "baseline",
                          "blocks": [{ "type": "paragraph", "style": { "fontSize": 9 }, "content": ["metros"] }] },
                        { "verticalAlign": "baseline", "blocks": ["de profundidade"] }
                      ] }"#,
            ),
        ),
        (
            // The empty cell is padded deeply: a baseline invented for it
            // would beat the real one and drag the text down.
            "célula vazia entre células com linha de base",
            page(
                r#"{ "type": "table", "columns": ["auto","auto","auto"], "inset": 4,
                      "cells": [
                        { "verticalAlign": "baseline", "blocks": ["antes"] },
                        { "verticalAlign": "baseline", "inset": 20, "blocks": [] },
                        { "verticalAlign": "baseline", "blocks": ["depois"] }
                      ] }"#,
            ),
        ),
        (
            "posições fixadas e buracos",
            page(
                r#"{ "type": "table", "columns": ["auto","auto","auto"], "inset": 4,
                      "cells": [
                        { "x": 2, "y": 0, "blocks": ["canto"] },
                        { "blocks": ["primeiro"] },
                        { "blocks": ["segundo"] },
                        { "y": 2, "blocks": ["salta uma linha"] }
                      ] }"#,
            ),
        ),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────

/// One line per item, in paint order — the order *is* part of the contract.
fn record(name: &str, list: &DisplayList) -> String {
    fn walk(items: &[DisplayItem], out: &mut String) {
        for item in items {
            match item {
                DisplayItem::Glyphs(run) => {
                    let trimmed = run.text.trim_end().len();
                    let right = run
                        .glyphs
                        .iter()
                        .filter(|glyph| (glyph.cluster as usize) < trimmed)
                        .map(|glyph| run.x + glyph.x + glyph.advance)
                        .fold(run.x, f64::max);
                    let _ = writeln!(
                        out,
                        "  texto  y={:>7.PLACES$}  {:>7.PLACES$}..{:<7.PLACES$}  {:?}",
                        run.y,
                        run.x,
                        right,
                        run.text.trim_end(),
                    );
                }
                // The box a cell emits to say where it is paints nothing, and
                // a record of what the page looks like should not carry it.
                DisplayItem::Rect(rect)
                    if rect.fill.is_none() && rect.stroke.is_none() => {}
                DisplayItem::Rect(rect) => {
                    let _ = writeln!(
                        out,
                        "  fundo  [{:>7.PLACES$} {:>7.PLACES$} {:>7.PLACES$} {:>7.PLACES$}]  {}",
                        rect.rect.x,
                        rect.rect.y,
                        rect.rect.w,
                        rect.rect.h,
                        rect.fill.map_or("—".to_string(), |c| c.to_hex()),
                    );
                }
                DisplayItem::Line(line) => {
                    let _ = writeln!(
                        out,
                        "  régua  ({:>7.PLACES$},{:>7.PLACES$})..({:>7.PLACES$},{:>7.PLACES$})  {:.2}",
                        line.x1, line.y1, line.x2, line.y2, line.stroke.width,
                    );
                }
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }

    let mut out = format!("# {name}\n");
    for page in &list.pages {
        walk(&page.items, &mut out);
    }
    out
}

#[test]
fn every_table_puts_its_parts_where_it_did_before() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let mut produced = String::new();
    for (name, document) in cases() {
        let list = engine.layout(&document);
        produced.push_str(&record(name, &list));
        produced.push('\n');
    }

    if std::env::var("ATUALIZAR").is_ok() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tabela.golden");
        std::fs::write(path, &produced).expect("gravar o registo");
        eprintln!("registo reescrito: {path}");
        return;
    }

    if produced != GOLDEN {
        let expected: Vec<&str> = GOLDEN.lines().collect();
        let actual: Vec<&str> = produced.lines().collect();
        let mut report = String::from("a tabela mudou de forma:\n");
        for index in 0..expected.len().max(actual.len()) {
            let before = expected.get(index).copied().unwrap_or("<ausente>");
            let after = actual.get(index).copied().unwrap_or("<ausente>");
            if before != after {
                let _ = writeln!(
                    report,
                    "  linha {}\n    antes: {before}\n    agora: {after}",
                    index + 1,
                );
            }
        }
        panic!("{report}");
    }
}

/// The corpus is only worth what it covers: every case has to draw something.
#[test]
fn every_case_actually_produces_a_table() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    for (name, document) in cases() {
        let list = engine.layout(&document);
        let record = record(name, &list);
        let glyphs = record.matches("texto").count();
        assert!(glyphs >= 2, "o caso «{name}» não escreveu nada:\n{record}");
    }
}
