//! What does not fit, measured where it happens — and nothing moved for it.
//!
//! A word wider than its line is forced in, a fixed row keeps its height
//! whatever the cell holds, and a thread can point at a frame that never
//! takes the text. The engine lays all of that out exactly as it always did;
//! what changes is that it now says so, with a size and a place.
//!
//! Two halves, then. The record pins every glyph where it landed before the
//! reporting existed, so reporting can never become repositioning. The tests
//! below it read the report.
//!
//! Rewrite the record after an intended layout change:
//!
//! ```sh
//! ATUALIZAR=1 cargo test --test medida --no-default-features --features images
//! ```

use std::fmt::Write as _;

use diagramador::Engine;
use diagramador::display::{Diagnostic, DisplayFrame, DisplayItem, DisplayList};
use diagramador::spec::{Document, FontWeight};

const GOLDEN: &str = include_str!("medida.golden");

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

fn document(frames: &str) -> Document {
    serde_json::from_str(&format!(
        r#"{{
            "page": {{ "size": "A4", "margins": 40 }},
            "style": {{ "fontFamily": "corpo", "fontSize": 14 }},
            "pages": [{{ "frames": [{frames}] }}]
        }}"#
    ))
    .expect("fixture válida")
}

/// A 150 pt frame and a word that no 150 pt line holds.
fn overfull_word() -> Document {
    document(
        r#"{ "id": "estreito", "type": "text", "rect": [40, 40, 150, 120],
             "blocks": ["um laboratórioextraordinariamentecomprido aqui",
                        "e um parágrafo comum que quebra"] }"#,
    )
}

/// The same word, centred: it spills out of both sides.
fn centred_word() -> Document {
    document(
        r#"{ "id": "centro", "type": "text", "rect": [40, 40, 150, 60],
             "style": { "textAlign": "center" },
             "blocks": ["supercalifragilisticexpialidocious"] }"#,
    )
}

/// A first row fixed at 18 pt holding several lines, and a word wider than
/// the 60 pt column under it.
fn fixed_row() -> Document {
    document(
        r#"{ "id": "tab", "type": "text", "rect": [40, 40, 300, 300], "blocks": [
             { "type": "table", "columns": [60, "1fr"], "rows": [18, "auto"], "inset": 2,
               "cells": [
                 { "blocks": ["uma célula com texto demais para dezoito pontos"] },
                 { "blocks": ["cabe"] },
                 { "blocks": ["palavraimensamentegrandedemais"] },
                 { "blocks": ["b"] }
               ] }
           ] }"#,
    )
}

fn cases() -> Vec<(&'static str, Document)> {
    vec![
        ("palavra mais larga que o frame", overfull_word()),
        (
            "palavra centralizada mais larga que o frame",
            centred_word(),
        ),
        ("linha de tabela com altura fixa", fixed_row()),
    ]
}

/// Every run of glyphs, and every rectangle, in paint order.
fn record(name: &str, list: &DisplayList) -> String {
    fn walk(items: &[DisplayItem], out: &mut String) {
        for item in items {
            match item {
                DisplayItem::Glyphs(run) => {
                    let _ = writeln!(
                        out,
                        "  texto  ({:>7.PLACES$}, {:>7.PLACES$})  w={:.PLACES$}  {:?}",
                        run.x, run.y, run.width, run.text
                    );
                }
                DisplayItem::Rect(rect) => {
                    let r = rect.rect;
                    let _ = writeln!(
                        out,
                        "  caixa  ({:>7.PLACES$}, {:>7.PLACES$})  {:.PLACES$}x{:.PLACES$}",
                        r.x, r.y, r.w, r.h
                    );
                }
                DisplayItem::Group(group) => {
                    let _ = writeln!(out, "  grupo  clip={}", group.clip.is_some());
                    walk(&group.items, out);
                }
                _ => {}
            }
        }
    }
    let mut out = format!("# {name}\n");
    for page in &list.pages {
        for frame in &page.frames {
            let r = frame.rect;
            let _ = writeln!(
                out,
                "  frame  {}  ({:.PLACES$}, {:.PLACES$}) {:.PLACES$}x{:.PLACES$}  overset={}",
                frame.id, r.x, r.y, r.w, r.h, frame.overset
            );
        }
        walk(&page.items, &mut out);
    }
    out
}

#[test]
fn reporting_what_does_not_fit_moves_nothing() {
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
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/medida.golden");
        std::fs::write(path, &produced).expect("gravar o registo");
        eprintln!("registo reescrito: {path}");
        return;
    }

    if produced != GOLDEN {
        let expected: Vec<&str> = GOLDEN.lines().collect();
        let actual: Vec<&str> = produced.lines().collect();
        let mut report = String::from("o layout mudou:\n");
        for index in 0..expected.len().max(actual.len()) {
            let before = expected.get(index).copied().unwrap_or("<ausente>");
            let after = actual.get(index).copied().unwrap_or("<ausente>");
            if before != after {
                let _ = writeln!(
                    report,
                    "  linha {}\n    antes: {before}\n    agora: {after}",
                    index + 1
                );
            }
        }
        panic!("{report}");
    }
}

// ── The report ──────────────────────────────────────────────────────────────

fn coded<'a>(list: &'a DisplayList, code: &str) -> Vec<&'a Diagnostic> {
    list.diagnostics.iter().filter(|d| d.code == code).collect()
}

fn frame<'a>(list: &'a DisplayList, id: &str) -> &'a DisplayFrame {
    list.pages
        .iter()
        .flat_map(|page| page.frames.iter())
        .find(|frame| frame.id == id)
        .unwrap_or_else(|| panic!("frame {id} ausente"))
}

#[test]
fn a_word_wider_than_its_line_is_reported_with_its_size_and_place() {
    let Some(engine) = engine() else { return };
    let list = engine.layout(&overfull_word());

    let said = coded(&list, "overfullLine");
    assert_eq!(said.len(), 1, "um aviso por parágrafo: {said:?}");
    let d = said[0];
    assert_eq!(d.page, Some(0));
    assert_eq!(d.frame.as_deref(), Some("estreito"));
    assert!(
        d.message
            .contains("\"laboratórioextraordinariamentecomprido\""),
        "{}",
        d.message
    );
    assert!(d.message.contains(" pt)"), "{}", d.message);

    let amount = d.amount.expect("quanto passa");
    assert!(amount > 0.0, "{amount}");

    // The word starts three bytes into the first inline of the first block.
    let source = d.source.as_ref().expect("de onde veio");
    assert_eq!(
        (source.block, source.inline, source.offset),
        (Some(0), Some(0), Some(3))
    );
    assert_eq!(source.frame, "estreito");

    // The line starts at the frame's edge and is as much wider than it as said.
    let rect = d.rect.expect("onde está");
    assert!((rect.x - 40.0).abs() < 0.01, "{rect:?}");
    assert!(
        (rect.w - (150.0 + amount)).abs() < 0.01,
        "{rect:?} vs {amount}"
    );
}

#[test]
fn a_centred_word_reports_the_line_where_it_was_drawn() {
    let Some(engine) = engine() else { return };
    let list = engine.layout(&centred_word());

    let d = coded(&list, "overfullLine")[0];
    let amount = d.amount.unwrap();
    let rect = d.rect.unwrap();
    // Half the excess out of each side.
    assert!(
        (rect.x - (40.0 - amount / 2.0)).abs() < 0.01,
        "{rect:?} vs {amount}"
    );
}

#[test]
fn a_paragraph_that_fits_says_nothing() {
    let Some(engine) = engine() else { return };
    let list = engine.layout(&document(
        r#"{ "id": "ok", "type": "text", "rect": [40, 40, 300, 34],
             "style": { "spaceAfter": 30 },
             "blocks": ["duas palavras"] }"#,
    ));

    assert!(
        coded(&list, "overfullLine").is_empty(),
        "{:?}",
        list.diagnostics
    );
    let fit = frame(&list, "ok").fit.expect("frame de texto mede");
    assert_eq!(fit.overflow_x, 0.0);
    // The spacing after the paragraph runs past the box; the text does not.
    assert!(fit.content_h > fit.box_h, "{fit:?}");
    assert_eq!(fit.overflow_y, 0.0, "{fit:?}");
    assert!(!fit.clipped, "{fit:?}");
}

#[test]
fn a_fixed_row_that_cannot_hold_its_cell_is_reported_against_the_cell() {
    let Some(engine) = engine() else { return };
    let list = engine.layout(&fixed_row());

    let said = coded(&list, "cellOverflow");
    let down = said
        .iter()
        .find(|d| d.message.contains("altura da linha"))
        .unwrap_or_else(|| panic!("sem aviso vertical: {said:?}"));
    assert_eq!(down.page, Some(0));
    assert_eq!(down.frame.as_deref(), Some("tab"));
    assert!(down.amount.unwrap() > 18.0, "{down:?}");

    let source = down.source.as_ref().unwrap();
    assert_eq!(source.cells.len(), 1, "{source:?}");
    assert_eq!((source.cells[0].block, source.cells[0].cell), (0, 0));

    let rect = down.rect.unwrap();
    assert!(
        (rect.h - 18.0).abs() < 0.01,
        "a célula tem a altura declarada: {rect:?}"
    );

    let across = said
        .iter()
        .find(|d| d.message.contains("mais larga que a coluna"))
        .unwrap_or_else(|| panic!("sem aviso horizontal: {said:?}"));
    assert!(across.amount.unwrap() > 0.0);
    let source = across.source.as_ref().unwrap();
    assert_eq!(source.cells[0].cell, 2, "{source:?}");
    assert_eq!(source.block, Some(0), "aponta a palavra, dentro da célula");

    // The word is the cell's problem, not the frame's.
    assert!(
        coded(&list, "overfullLine").is_empty(),
        "{:?}",
        list.diagnostics
    );
}

#[test]
fn a_table_sized_by_its_content_has_no_overflow() {
    let Some(engine) = engine() else { return };
    let list = engine.layout(&document(
        r#"{ "id": "tab", "type": "text", "rect": [40, 40, 400, 300], "blocks": [
             { "type": "table", "columns": ["auto", "1fr"], "inset": 4,
               "cells": [
                 { "blocks": ["Espécie"] }, { "blocks": ["um texto mais longo que quebra em várias linhas dentro da coluna"] },
                 { "blocks": ["Cachalote"], "verticalAlign": "middle" }, { "blocks": ["2000 m"] }
               ] }
           ] }"#,
    ));
    assert!(
        coded(&list, "cellOverflow").is_empty(),
        "{:?}",
        list.diagnostics
    );
}

#[test]
fn text_frames_carry_how_their_content_measured() {
    let Some(engine) = engine() else { return };
    let long = "texto que não cabe de jeito nenhum neste frame tão pequeno e baixo";
    let list = engine.layout(&document(&format!(
        r#"{{ "id": "cheio", "type": "text", "rect": [40, 40, 150, 30], "blocks": ["{long}"] }},
           {{ "id": "visivel", "type": "text", "overflow": "visible", "rect": [240, 40, 150, 30], "blocks": ["{long}"] }},
           {{ "id": "cresce", "type": "text", "overflow": "grow", "padding": 4, "rect": [40, 200, 150, 30], "blocks": ["{long}"] }},
           {{ "id": "folga", "type": "text", "rect": [240, 200, 150, 100], "blocks": ["pouco"] }},
           {{ "id": "forma", "type": "shape", "shape": "rect", "rect": [40, 400, 10, 10] }},
           {{ "type": "text", "rect": [240, 400, 150, 60], "blocks": ["um laboratórioextraordinariamentecomprido"], "id": "largo" }}"#
    )));

    // Clipped, with the rest never placed: known to be taller by what is left.
    let cheio = frame(&list, "cheio").fit.unwrap();
    assert!(frame(&list, "cheio").overset);
    assert!(cheio.content_h <= cheio.box_h + 0.01, "{cheio:?}");
    assert!(cheio.overflow_y > 0.0, "{cheio:?}");
    assert!(
        !cheio.clipped,
        "o que sobrou não foi desenhado, então não foi cortado"
    );

    // Drawn past the bottom, and not clipped.
    let visivel = frame(&list, "visivel").fit.unwrap();
    assert_eq!(visivel.box_h, 30.0);
    assert!(
        (visivel.overflow_y - (visivel.content_h - 30.0)).abs() < 0.01,
        "{visivel:?}"
    );
    assert!(!visivel.clipped);

    // Grown to hold it: nothing overflows.
    let cresce = frame(&list, "cresce").fit.unwrap();
    assert!((cresce.content_h - cresce.box_h).abs() < 0.01, "{cresce:?}");
    assert_eq!(frame(&list, "cresce").rect.h, cresce.box_h + 8.0);
    assert_eq!(
        (cresce.overflow_x, cresce.overflow_y, cresce.clipped),
        (0.0, 0.0, false)
    );

    // Room to spare.
    let folga = frame(&list, "folga").fit.unwrap();
    assert!(
        folga.content_h > 0.0 && folga.content_h < folga.box_h,
        "{folga:?}"
    );
    assert_eq!(
        (folga.overflow_x, folga.overflow_y, folga.clipped),
        (0.0, 0.0, false)
    );

    // A word past the right edge of a clipping frame is cut off.
    let largo = frame(&list, "largo").fit.unwrap();
    assert!(largo.overflow_x > 0.0 && largo.clipped, "{largo:?}");
    let said = coded(&list, "overfullLine");
    let reported = said
        .iter()
        .find(|d| d.frame.as_deref() == Some("largo"))
        .unwrap();
    assert!((reported.amount.unwrap() - largo.overflow_x).abs() < 1e-9);

    // Only text frames measure.
    assert!(frame(&list, "forma").fit.is_none());
}

#[test]
fn content_that_never_arrives_names_the_frame_it_was_waiting_for() {
    let Some(engine) = engine() else { return };
    let doc: Document = serde_json::from_str(
        r#"{
            "style": { "fontFamily": "corpo", "fontSize": 14 },
            "pages": [
              { "frames": [
                { "id": "antes", "type": "text", "rect": [40, 40, 150, 40], "blocks": ["tarde demais"] }
              ] },
              { "frames": [
                { "id": "fonte", "type": "text", "rect": [40, 40, 150, 20], "threadNext": "antes",
                  "blocks": ["muito texto que segue para um frame de uma página que já passou"] },
                { "id": "outra", "type": "text", "rect": [240, 40, 150, 20], "threadNext": "sumido",
                  "blocks": ["muito texto que segue para um frame que não existe em lugar nenhum"] }
              ] }
            ]
        }"#,
    )
    .unwrap();
    let list = engine.layout(&doc);

    let unplaced: Vec<&Diagnostic> = coded(&list, "overset")
        .into_iter()
        .filter(|d| d.message.contains("não foi colocado"))
        .collect();
    assert_eq!(unplaced.len(), 2, "{:?}", list.diagnostics);

    let antes = unplaced
        .iter()
        .find(|d| d.frame.as_deref() == Some("antes"))
        .unwrap();
    assert_eq!(
        antes.page,
        Some(0),
        "o frame existe, e está na primeira página"
    );

    let sumido = unplaced
        .iter()
        .find(|d| d.frame.as_deref() == Some("sumido"))
        .unwrap();
    assert_eq!(sumido.page, None, "um frame que não existe não tem página");
}

#[test]
fn a_problem_on_a_master_is_reported_on_the_first_page_that_uses_it() {
    let Some(engine) = engine() else { return };
    let doc: Document = serde_json::from_str(
        r#"{
            "style": { "fontFamily": "corpo", "fontSize": 14 },
            "resources": { "masters": {
              "usada": { "frames": [ { "id": "a", "type": "instance", "component": "nada", "rect": [0, 0, 10, 10] } ] },
              "esquecida": { "frames": [ { "id": "b", "type": "instance", "component": "nada", "rect": [0, 0, 10, 10] } ] }
            } },
            "pages": [ { "frames": [] }, { "master": "usada", "frames": [] }, { "master": "usada", "frames": [] } ]
        }"#,
    )
    .unwrap();
    let list = engine.layout(&doc);

    let said = coded(&list, "unknownComponent");
    let a = said
        .iter()
        .find(|d| d.frame.as_deref() == Some("a"))
        .unwrap();
    assert_eq!(a.page, Some(1));
    let b = said
        .iter()
        .find(|d| d.frame.as_deref() == Some("b"))
        .unwrap();
    assert_eq!(b.page, None);
}

#[test]
fn the_display_list_says_which_shape_it_speaks() {
    let Some(engine) = engine() else { return };
    let list = engine.layout(&overfull_word());
    assert_eq!(list.version, diagramador::display::DISPLAY_VERSION);
    assert_eq!(list.version, 2);

    let json = serde_json::to_value(&list).unwrap();
    let fit = &json["pages"][0]["frames"][0]["fit"];
    for key in ["contentH", "boxH", "overflowX", "overflowY", "clipped"] {
        assert!(!fit[key].is_null(), "{key} ausente em {fit}");
    }
}
