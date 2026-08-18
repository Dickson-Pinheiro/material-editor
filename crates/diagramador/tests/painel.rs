//! O painel: uma moldura que flui.
//!
//! O que se testa aqui é justamente o que a tabela de uma célula não dava —
//! raio de canto, borda que não se compartilha com o vizinho, espaço antes e
//! depois — mais o que ela já dava e não se pode perder: quebrar entre páginas
//! junto com o texto.

use diagramador::Engine;
use diagramador::display::{DisplayItem, DisplayList, RectItem};
use diagramador::spec::{Document, FontWeight};
use diagramador::units::Corners;

fn engine() -> Option<Engine> {
    let mut engine = Engine::new();
    for (file, weight) in [("DejaVuSans.ttf", 400u16), ("DejaVuSans-Bold.ttf", 700)] {
        let bytes = std::fs::read(format!("../../fonts/{file}"))
            .or_else(|_| std::fs::read(format!("fonts/{file}")))
            .ok()?;
        engine.add_font("corpo", bytes, Some(FontWeight(weight)), Some(false)).ok()?;
    }
    Some(engine)
}

fn layout(json: &str) -> Option<DisplayList> {
    let doc: Document = serde_json::from_str(json).expect("documento válido");
    Some(engine()?.layout(&doc))
}

/// Todo retângulo pintado, por página.
fn rects(list: &DisplayList, page: usize) -> Vec<RectItem> {
    fn walk(items: &[DisplayItem], out: &mut Vec<RectItem>) {
        for item in items {
            match item {
                DisplayItem::Rect(rect) => out.push(rect.clone()),
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&list.pages[page].items, &mut out);
    out
}

/// Todo run de glifos, por página. Eles vivem dentro dos grupos que os frames
/// criam, então a busca tem de descer.
fn glyphs(list: &DisplayList, page: usize) -> Vec<diagramador::display::GlyphRun> {
    fn walk(items: &[DisplayItem], out: &mut Vec<diagramador::display::GlyphRun>) {
        for item in items {
            match item {
                DisplayItem::Glyphs(run) => out.push(run.clone()),
                DisplayItem::Group(group) => walk(&group.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&list.pages[page].items, &mut out);
    out
}

/// Um documento de uma página com os blocos dados no corpo.
fn page_with(blocks: &str) -> String {
    format!(
        r#"{{
            "style": {{"fontFamily": "corpo", "fontSize": 11}},
            "pages": [{{"frames": [
                {{"type": "text", "rect": [40, 40, 300, 700], "blocks": [{blocks}]}}
            ]}}]
        }}"#
    )
}

#[test]
fn a_moldura_carrega_o_raio_que_a_tabela_nao_tinha() {
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "fill": "#eef4fb", "radius": [8, 8, 2, 2],
             "inset": 10, "blocks": ["Dentro da moldura"]}"##,
    )) else {
        return;
    };

    let pintado: Vec<_> = rects(&list, 0).into_iter().filter(|r| r.fill.is_some()).collect();
    assert_eq!(pintado.len(), 1, "a moldura é um retângulo só");
    assert_eq!(pintado[0].radius, Corners::new(8.0, 8.0, 2.0, 2.0));
}

#[test]
fn a_moldura_cresce_com_o_que_esta_dentro() {
    let curto = layout(&page_with(
        r##"{"type": "panel", "fill": "#eeeeee", "inset": 10, "blocks": ["uma linha"]}"##,
    ));
    let longo = layout(&page_with(
        r##"{"type": "panel", "fill": "#eeeeee", "inset": 10,
             "blocks": ["uma linha", "outra linha", "e mais uma terceira linha"]}"##,
    ));
    let (Some(curto), Some(longo)) = (curto, longo) else {
        return;
    };

    let altura = |l: &DisplayList| rects(l, 0).into_iter().find(|r| r.fill.is_some()).unwrap().rect.h;
    assert!(altura(&longo) > altura(&curto), "mais conteúdo, moldura mais alta");
}

#[test]
fn o_recuo_afasta_o_conteudo_da_moldura_dos_quatro_lados() {
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "fill": "#eeeeee", "inset": 20, "blocks": ["texto"]}"##,
    )) else {
        return;
    };

    let moldura = rects(&list, 0).into_iter().find(|r| r.fill.is_some()).unwrap();
    let glifos = glyphs(&list, 0);
    assert!(!glifos.is_empty(), "o texto foi desenhado");
    let primeiro = &glifos[0];
    assert!(
        primeiro.x >= moldura.rect.x + 19.0,
        "o texto começa depois do recuo esquerdo"
    );
}

#[test]
fn duas_molduras_vizinhas_nao_compartilham_a_borda() {
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "border": {"width": 2, "color": "#1f4e79"}, "inset": 8, "blocks": ["um"]},
            {"type": "panel", "border": {"width": 2, "color": "#1f4e79"}, "inset": 8, "blocks": ["dois"]}"##,
    )) else {
        return;
    };

    let com_traco: Vec<_> = rects(&list, 0).into_iter().filter(|r| r.stroke.is_some()).collect();
    assert_eq!(com_traco.len(), 2, "cada moldura tem a sua borda");

    // A de cima acaba antes de a de baixo começar: nenhuma linha em comum.
    let (a, b) = (&com_traco[0].rect, &com_traco[1].rect);
    assert!(a.y + a.h <= b.y + 0.01, "as molduras não se sobrepõem");
}

#[test]
fn o_espaco_antes_e_depois_vale_para_a_moldura_inteira() {
    let sem = layout(&page_with(
        r##""antes",
            {"type": "panel", "fill": "#eeeeee", "inset": 6, "blocks": ["meio"]}"##,
    ));
    let com = layout(&page_with(
        r##""antes",
            {"type": "panel", "fill": "#eeeeee", "inset": 6, "style": {"spaceBefore": 30},
             "blocks": ["meio"]}"##,
    ));
    let (Some(sem), Some(com)) = (sem, com) else {
        return;
    };

    let topo = |l: &DisplayList| rects(l, 0).into_iter().find(|r| r.fill.is_some()).unwrap().rect.y;
    let diferenca = topo(&com) - topo(&sem);
    assert!(
        (diferenca - 30.0).abs() < 0.5,
        "spaceBefore desceu a moldura em 30pt, e desceu {diferenca}"
    );
}

#[test]
fn uma_moldura_alta_demais_continua_na_pagina_seguinte() {
    // Vinte parágrafos numa moldura que não cabe numa página só.
    let paragrafos: Vec<String> = (0..20)
        .map(|i| format!(r#""Parágrafo número {i} dentro da moldura, com texto suficiente para ocupar linha.""#))
        .collect();

    let json = format!(
        r##"{{
            "style": {{"fontFamily": "corpo", "fontSize": 11}},
            "pages": [{{"frames": [
                {{"type": "text", "rect": [40, 40, 300, 200], "autoFlow": true,
                  "blocks": [{{"type": "panel", "fill": "#eeeeee", "inset": 8,
                               "blocks": [{}]}}]}}
            ]}}]
        }}"##,
        paragrafos.join(",")
    );

    let Some(list) = layout(&json) else { return };

    assert!(list.pages.len() > 1, "a moldura atravessou a página");

    // Cada página que a moldura alcança tem a sua própria pintura.
    for (index, _) in list.pages.iter().enumerate() {
        let molduras: Vec<_> = rects(&list, index).into_iter().filter(|r| r.fill.is_some()).collect();
        assert_eq!(molduras.len(), 1, "página {index} tem uma moldura");
    }

    // Nada se perdeu no corte.
    let total: usize = (0..list.pages.len()).map(|i| glyphs(&list, i).len()).sum();
    assert!(total > 20, "o texto inteiro foi desenhado, e não só o começo, e saíram {total} runs");
}

#[test]
fn molduras_aninham() {
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "fill": "#eeeeee", "inset": 10, "blocks": [
              "de fora",
              {"type": "panel", "fill": "#dddddd", "inset": 6, "blocks": ["de dentro"]}
            ]}"##,
    )) else {
        return;
    };

    let molduras: Vec<_> = rects(&list, 0).into_iter().filter(|r| r.fill.is_some()).collect();
    assert_eq!(molduras.len(), 2);

    let (fora, dentro) = (&molduras[0].rect, &molduras[1].rect);
    assert!(dentro.x > fora.x && dentro.right() < fora.right(), "a de dentro cabe na de fora");
}

#[test]
fn a_moldura_volta_do_json_como_entrou() {
    let json = r##"{"type":"panel","blocks":["olá"],"fill":"#eef4fb","radius":[8,0,8,0],"inset":10}"##;
    let bloco: diagramador::spec::Block = serde_json::from_str(json).unwrap();

    let volta = serde_json::to_string(&bloco).unwrap();
    let outra: diagramador::spec::Block = serde_json::from_str(&volta).unwrap();
    assert_eq!(bloco, outra);
}

#[test]
fn uma_barra_lateral_e_uma_borda_de_um_lado_so() {
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "fill": "#faf5ff", "inset": 8,
             "border": {"width": 3, "color": "#9333ea",
                        "sides": {"top": false, "right": false, "bottom": false, "left": true}},
             "blocks": ["destaque"]}"##,
    )) else {
        return;
    };

    // Uma caixa com quatro lados vira retângulo tracejado; um lado só vira
    // linha — é o mesmo caminho que os frames seguem.
    let com_traco: Vec<_> = rects(&list, 0).into_iter().filter(|r| r.stroke.is_some()).collect();
    assert!(com_traco.is_empty(), "um lado só não vira retângulo tracejado");

    let linhas: Vec<_> = list.pages[0]
        .items
        .iter()
        .flat_map(|item| match item {
            DisplayItem::Group(g) => g.items.clone(),
            other => vec![other.clone()],
        })
        .filter(|i| matches!(i, DisplayItem::Line(_)))
        .collect();
    assert_eq!(linhas.len(), 1, "uma aresta, uma linha");
}

#[test]
fn os_quatro_lados_ligados_mantem_o_raio() {
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "inset": 8, "radius": 6,
             "border": {"width": 2, "color": "#1f4e79"}, "blocks": ["caixa"]}"##,
    )) else {
        return;
    };

    let moldura = rects(&list, 0).into_iter().find(|r| r.stroke.is_some()).unwrap();
    assert_eq!(moldura.radius, Corners::all(6.0), "borda inteira mantém o canto");
}
