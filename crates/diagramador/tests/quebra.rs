//! A long table crossing pages, and the one question that matters about it.
//!
//! Everything else a break can get wrong — a rule on the wrong page, a stripe
//! that restarts, a column that changes width — is a blemish. Losing a row, or
//! printing one twice, is a lie about the data. So this file reads the rows
//! back out of the finished pages and compares them, in order, against what
//! went in. Nothing else is as important, and nothing else is as easy to
//! believe is fine when it is not.

use diagramador::display::{DisplayItem, DisplayList, DisplayPage};
use diagramador::spec::{Document, FontWeight};
use diagramador::Engine;

fn engine() -> Option<Engine> {
    let mut engine = Engine::new();
    let bytes = std::fs::read("../../fonts/DejaVuSans.ttf")
        .or_else(|_| std::fs::read("fonts/DejaVuSans.ttf"))
        .ok()?;
    engine.add_font("corpo", bytes, Some(FontWeight(400)), Some(false)).ok()?;
    Some(engine)
}

/// A table of `rows` rows, in a frame that auto-flows onto further pages.
fn document(rows: usize, extra: &str) -> Document {
    let cells: Vec<String> = (0..rows)
        .flat_map(|row| {
            [
                format!(r#"{{ "blocks": ["linha {row:03}"] }}"#),
                format!(r#"{{ "blocks": ["valor {:03}"] }}"#, row * 7 % 1000),
            ]
        })
        .collect();

    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "autoFlow": {{ "enabled": true }},
            "pages": [{{ "frames": [
                {{ "id": "corpo", "type": "text", "rect": [56, 56, 483, 730],
                   "autoFlow": true,
                   "blocks": [
                     {{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                        {extra}
                        "cells": [{}] }}
                   ] }}
            ] }}]
        }}"#,
        cells.join(","),
    ))
    .expect("fixture válida")
}

/// Every "linha NNN" painted, in the order the pages were laid out.
fn painted(list: &DisplayList) -> Vec<String> {
    let mut out = Vec::new();
    for page in &list.pages {
        out.extend(text_on(page).into_iter().filter(|t| t.starts_with("linha ")));
    }
    out
}

/// Everything written on one page, in paint order.
fn text_on(page: &DisplayPage) -> Vec<String> {
    fn walk(items: &[DisplayItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                DisplayItem::Glyphs(run) => out.push(run.text.trim().to_string()),
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&page.items, &mut out);
    out
}

#[test]
fn two_hundred_rows_cross_the_pages_without_losing_or_repeating_one() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let list = engine.layout(&document(200, ""));
    let expected: Vec<String> = (0..200).map(|row| format!("linha {row:03}")).collect();

    // Rows of 22 in 730 points of column: 33 a page, so 200 rows need seven.
    assert_eq!(list.pages.len(), 7, "e parte onde se espera");
    assert_eq!(painted(&list), expected, "a concatenação é exactamente a entrada");
}

/// The same, with everything a break has to carry along switched on.
#[test]
fn the_rows_survive_stripes_rules_and_spans_too() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let extra = r##""fill": "#f7fafc",
        "stripe": { "every": 2, "offset": 1, "fill": "#e2e8f0" },
        "lines": [{ "axis": "vertical", "at": 1, "width": 0.5 }],"##;

    let list = engine.layout(&document(200, extra));
    let expected: Vec<String> = (0..200).map(|row| format!("linha {row:03}")).collect();
    assert_eq!(painted(&list), expected);
}

/// A row that cannot fit anywhere still has to come out, or the flow never
/// terminates and the document never finishes.
#[test]
fn a_row_taller_than_a_whole_page_is_emitted_rather_than_carried_forever() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let filler = "palavra ".repeat(600);
    let document: Document = serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "autoFlow": {{ "enabled": true }},
            "pages": [{{ "frames": [
                {{ "id": "corpo", "type": "text", "rect": [56, 56, 483, 300],
                   "autoFlow": true,
                   "blocks": [
                     {{ "type": "table", "columns": ["auto"], "inset": 4, "cells": [
                        {{ "blocks": ["{filler}"] }},
                        {{ "blocks": ["depois"] }}
                     ] }}
                   ] }}
            ] }}]
        }}"#
    ))
    .expect("fixture válida");

    let list = engine.layout(&document);
    let mut found = Vec::new();
    fn walk(items: &[DisplayItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                DisplayItem::Glyphs(run) => out.push(run.text.trim().to_string()),
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }
    for page in &list.pages {
        walk(&page.items, &mut found);
    }
    assert!(found.iter().any(|text| text == "depois"), "a linha seguinte chega ao papel");
    assert!(list.pages.len() < 50, "e o fluxo termina: {} páginas", list.pages.len());
}

// ─────────────────────────────────────────────────────────────────────────────
// Repeated bands
// ─────────────────────────────────────────────────────────────────────────────

/// The long table again, with a header row on top and whatever else is asked.
fn with_header(rows: usize, band: &str) -> Document {
    let mut cells = vec![
        r#"{ "blocks": ["Espécie"] }"#.to_string(),
        r#"{ "blocks": ["Registo"] }"#.to_string(),
    ];
    for row in 0..rows {
        cells.push(format!(r#"{{ "blocks": ["linha {row:03}"] }}"#));
        cells.push(format!(r#"{{ "blocks": ["valor {:03}"] }}"#, row * 7 % 1000));
    }

    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "autoFlow": {{ "enabled": true }},
            "pages": [{{ "frames": [
                {{ "id": "corpo", "type": "text", "rect": [56, 56, 483, 730],
                   "autoFlow": true,
                   "blocks": [
                     {{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                        {band}
                        "cells": [{}] }}
                   ] }}
            ] }}]
        }}"#,
        cells.join(","),
    ))
    .expect("fixture válida")
}

#[test]
fn the_header_stands_at_the_top_of_every_page_the_table_reaches() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let list = engine.layout(&with_header(200, r#""header": { "rows": 1 },"#));
    assert!(list.pages.len() >= 5, "várias páginas: {}", list.pages.len());

    for (number, page) in list.pages.iter().enumerate() {
        let text = text_on(page);
        assert_eq!(
            text.first().map(String::as_str),
            Some("Espécie"),
            "a página {} abre pelo cabeçalho: {:?}",
            number + 1,
            &text[..text.len().min(4)],
        );
    }

    let expected: Vec<String> = (0..200).map(|row| format!("linha {row:03}")).collect();
    assert_eq!(painted(&list), expected, "e nenhuma linha se perdeu por causa dele");
}

#[test]
fn a_continuation_header_says_so_on_every_page_but_the_first() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    // One word, and no wider than the column the first page settled on: a
    // label that wraps arrives as two runs, and the first of them is not the
    // label.
    let band = r#""header": { "rows": 1, "continued": [
        { "blocks": ["Espécie·cont"] }, { "blocks": ["Registo"] }
    ] },"#;
    let list = engine.layout(&with_header(200, band));

    let heads: Vec<String> = list.pages.iter().filter_map(|p| text_on(p).first().cloned()).collect();
    assert_eq!(heads[0], "Espécie", "a primeira página tem o cabeçalho escrito");
    assert!(
        heads[1..].iter().all(|head| head == "Espécie·cont"),
        "e as outras o de continuação: {heads:?}",
    );
    assert!(heads.len() >= 5);
}

#[test]
fn a_header_told_not_to_repeat_is_seen_once() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let list = engine.layout(&with_header(200, r#""header": { "rows": 1, "repeat": false },"#));
    let count = list
        .pages
        .iter()
        .filter(|page| text_on(page).iter().any(|text| text == "Espécie"))
        .count();
    assert_eq!(count, 1, "uma vez e não mais");
}

#[test]
fn the_continuation_footer_is_absent_from_the_last_page() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let band = r#""footer": { "rows": 1, "continued": [
        { "blocks": ["(continua)"] }, { "blocks": [""] }
    ] },"#;
    let list = engine.layout(&with_header(200, band));

    let last = list.pages.len() - 1;
    for (number, page) in list.pages.iter().enumerate() {
        let says = text_on(page).iter().any(|text| text == "(continua)");
        assert_eq!(
            says,
            number != last,
            "página {} de {}: {says}",
            number + 1,
            list.pages.len(),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Cells over the break
// ─────────────────────────────────────────────────────────────────────────────

/// Long table where every fourth row is held together by a `rowspan: 3`, so a
/// break has to fall past a span wherever it lands.
fn with_spans(groups: usize) -> Document {
    let mut cells = Vec::new();
    for group in 0..groups {
        cells.push(format!(
            r#"{{ "rowspan": 3, "blocks": ["grupo {group:03}"] }}"#
        ));
        for step in 0..3 {
            cells.push(format!(r#"{{ "blocks": ["linha {:03}"] }}"#, group * 3 + step));
        }
    }

    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": "20mm" }},
            "style": {{ "fontFamily": "corpo", "fontSize": 10 }},
            "autoFlow": {{ "enabled": true }},
            "pages": [{{ "frames": [
                {{ "id": "corpo", "type": "text", "rect": [56, 56, 483, 730],
                   "autoFlow": true,
                   "blocks": [
                     {{ "type": "table", "columns": ["auto","auto"], "inset": 4,
                        "header": {{ "rows": 1 }},
                        "cells": [
                          {{ "blocks": ["Grupo"] }}, {{ "blocks": ["Registo"] }},
                          {}
                        ] }}
                   ] }}
            ] }}]
        }}"#,
        cells.join(","),
    ))
    .expect("fixture válida")
}

#[test]
fn a_span_over_the_break_is_drawn_once_and_never_halved() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let list = engine.layout(&with_spans(60));
    assert!(list.pages.len() >= 3, "várias páginas: {}", list.pages.len());

    let mut groups = Vec::new();
    for page in &list.pages {
        groups.extend(text_on(page).into_iter().filter(|t| t.starts_with("grupo ")));
    }
    let expected: Vec<String> = (0..60).map(|group| format!("grupo {group:03}")).collect();
    assert_eq!(groups, expected, "cada grupo uma vez só, e por ordem");

    let expected: Vec<String> = (0..180).map(|row| format!("linha {row:03}")).collect();
    assert_eq!(painted(&list), expected, "e nenhuma das linhas que ele segura se perde");
}

#[test]
fn a_span_and_the_rows_it_holds_land_on_the_same_page() {
    let Some(engine) = engine() else {
        eprintln!("fontes ausentes — teste ignorado");
        return;
    };

    let list = engine.layout(&with_spans(60));
    for (number, page) in list.pages.iter().enumerate() {
        let text = text_on(page);
        for group in text.iter().filter(|t| t.starts_with("grupo ")) {
            let index: usize = group[6..].parse().expect("número do grupo");
            for step in 0..3 {
                let row = format!("linha {:03}", index * 3 + step);
                assert!(
                    text.contains(&row),
                    "a página {} tem {group} mas não {row}",
                    number + 1,
                );
            }
        }
    }
}
