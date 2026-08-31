//! O painel: uma moldura que flui.
//!
//! O que se testa aqui é justamente o que a tabela de uma célula não dava —
//! raio de canto, borda que não se compartilha com o vizinho, espaço antes e
//! depois — mais o que ela já dava e não se pode perder: quebrar entre páginas
//! junto com o texto.

use diagramador::Engine;
use diagramador::display::{PathCommand, DisplayItem, DisplayList, RectItem};
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

    // Uma caixa com quatro lados vira retângulo tracejado; um subconjunto vira
    // caminho — é o mesmo caminho que os frames seguem.
    let com_traco: Vec<_> = rects(&list, 0).into_iter().filter(|r| r.stroke.is_some()).collect();
    assert!(com_traco.is_empty(), "um lado só não vira retângulo tracejado");

    let caminhos: Vec<_> = list.pages[0]
        .items
        .iter()
        .flat_map(|item| match item {
            DisplayItem::Group(g) => g.items.clone(),
            other => vec![other.clone()],
        })
        .filter_map(|i| match i {
            DisplayItem::Path(p) => Some(p),
            _ => None,
        })
        .collect();

    assert_eq!(caminhos.len(), 1, "uma aresta, um caminho");
    // Sem raio declarado, os cantos são retos: só o traço da aresta.
    assert_eq!(caminhos[0].commands.len(), 2, "ir até o começo e riscar");
}

#[test]
fn a_barra_lateral_acompanha_o_raio_da_moldura() {
    // A queixa que originou a correção: o preenchimento saía arredondado e a
    // barra saía reta, e as duas discordavam no canto.
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "fill": "#faf5ff", "inset": 8, "radius": 10,
             "border": {"width": 3, "color": "#9333ea",
                        "sides": {"top": false, "right": false, "bottom": false, "left": true}},
             "blocks": ["destaque"]}"##,
    )) else {
        return;
    };

    let preenchido = rects(&list, 0)
        .into_iter()
        .find(|r| r.fill.is_some())
        .expect("a moldura tem preenchimento");
    assert_eq!(preenchido.radius, Corners::all(10.0), "o preenchimento arredonda");

    let caminho = list.pages[0]
        .items
        .iter()
        .flat_map(|item| match item {
            DisplayItem::Group(g) => g.items.clone(),
            other => vec![other.clone()],
        })
        .find_map(|i| match i {
            DisplayItem::Path(p) => Some(p),
            _ => None,
        })
        .expect("a barra é um caminho");

    let curvas = caminho
        .commands
        .iter()
        .filter(|c| matches!(c, PathCommand::CurveTo { .. }))
        .count();

    assert_eq!(curvas, 2, "a barra arredonda nos dois cantos que toca");
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

#[test]
fn uma_moldura_vazia_pode_ser_clicada() {
    // Um painel sem glifo é uma coisa que se vê e, sem esta geometria, não se
    // consegue entrar: o cursor é colocado achando o glifo mais próximo, e não
    // há nenhum. A célula de tabela já emitia a própria área; a moldura não.
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "fill": "#eef2ff", "inset": 8, "blocks": []}"##,
    )) else {
        return;
    };

    let area = rects(&list, 0)
        .into_iter()
        .find(|r| r.fill.is_none() && r.stroke.is_none())
        .expect("a moldura declara onde está");

    assert!(area.rect.w > 0.0, "a área tem largura");
    assert!(area.rect.h > 0.0, "a área tem altura");
    assert!(
        area.source.as_ref().is_some_and(|s| !s.cells.is_empty()),
        "e carrega a trilha que leva para dentro dela"
    );
}

#[test]
fn um_paragrafo_vazio_declara_onde_esta() {
    // Um bloco recém-inserido não pinta glifo nenhum. Sem esta geometria ele
    // é uma coisa que não se vê e na qual não se consegue escrever.
    let Some(list) = layout(&page_with(r##"{"type": "paragraph", "content": []}"##)) else {
        return;
    };

    let area = rects(&list, 0)
        .into_iter()
        .find(|r| r.fill.is_none() && r.stroke.is_none())
        .expect("o parágrafo vazio declara onde está");

    assert!(area.rect.h > 0.0, "com altura de linha, não zero");
    assert_eq!(
        area.source.as_ref().and_then(|s| s.inline),
        Some(0),
        "e aponta para o começo do texto"
    );
}

#[test]
fn um_paragrafo_com_texto_nao_ganha_retangulo() {
    // Um por parágrafo engordaria a display list de todo documento, e o texto
    // já se endereça pelas próprias letras.
    let Some(list) = layout(&page_with(r##"{"type": "paragraph", "content": ["escrito"]}"##)) else {
        return;
    };

    let vazios: Vec<_> = rects(&list, 0)
        .into_iter()
        .filter(|r| r.fill.is_none() && r.stroke.is_none())
        .collect();

    assert!(vazios.is_empty(), "nenhum retângulo a mais");
}

#[test]
fn reguas_seguidas_respeitam_o_espaco_pedido() {
    // Quatro linhas de resposta de uma atividade saíam a 0,75 pt uma da outra
    // — a espessura delas — porque a régua ignorava o próprio `style`.
    // Pareciam uma linha grossa só, e não havia onde escrever.
    let Some(list) = layout(&page_with(
        r##"{"type": "panel", "inset": 6, "blocks": [
             {"type": "rule", "thickness": 0.75, "style": {"spaceBefore": 10}},
             {"type": "rule", "thickness": 0.75, "style": {"spaceBefore": 10}},
             {"type": "rule", "thickness": 0.75, "style": {"spaceBefore": 10}}
           ]}"##,
    )) else {
        return;
    };

    let mut alturas: Vec<f64> = list.pages[0]
        .items
        .iter()
        .flat_map(|item| match item {
            DisplayItem::Group(g) => g.items.clone(),
            other => vec![other.clone()],
        })
        .filter_map(|i| match i {
            DisplayItem::Line(l) => Some(l.y1),
            _ => None,
        })
        .collect();

    alturas.sort_by(f64::total_cmp);
    assert_eq!(alturas.len(), 3, "as três réguas foram desenhadas");

    for par in alturas.windows(2) {
        let vao = par[1] - par[0];
        assert!(
            vao > 9.0,
            "o espaço pedido é de 10 pt; veio {vao:.2}"
        );
    }
}

/// A altura da moldura pintada — a área que o painel declara.
fn altura_da_moldura(blocos: &str) -> Option<f64> {
    let list = layout(&page_with(blocos))?;
    rects(&list, 0)
        .into_iter()
        .filter(|r| r.fill.is_none() && r.stroke.is_none())
        .map(|r| r.rect.h)
        .fold(None, |maior: Option<f64>, h| Some(maior.map_or(h, |m: f64| m.max(h))))
}

/// A moldura cresce com o conteúdo, mas pode ter um piso.
///
/// É o que permite deixar um quadro de atividade com espaço para o aluno
/// escrever sem enchê-lo de blocos vazios só para empurrar a borda.
#[test]
fn uma_moldura_pode_ter_altura_minima() {
    let curto = r##"{"type": "panel", "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "curto"}]}]}"##;
    let alto = r##"{"type": "panel", "minHeight": 200, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "curto"}]}]}"##;

    let (Some(natural), Some(com_piso)) = (altura_da_moldura(curto), altura_da_moldura(alto)) else {
        return;
    };

    assert!(natural < 100.0, "o texto curto não chega perto do piso: {natural}");
    assert!(
        (com_piso - 200.0).abs() < 0.5,
        "a moldura devia parar no piso pedido, e mede {com_piso}"
    );
}

/// O piso é piso, não teto: conteúdo maior continua crescendo.
#[test]
fn o_piso_nao_corta_o_conteudo() {
    let paragrafos: Vec<String> = (0..20)
        .map(|i| format!(r##"{{"type": "paragraph", "content": [{{"type": "text", "text": "linha {i}"}}]}}"##))
        .collect();
    let muito = format!(r##"{{"type": "panel", "minHeight": 40, "blocks": [{}]}}"##, paragrafos.join(","));

    let Some(altura) = altura_da_moldura(&muito) else { return };
    assert!(altura > 40.0, "vinte parágrafos passam de 40pt; o piso não pode cortá-los, e mede {altura}");
}

/// Um piso maior que a mancha para na mancha.
#[test]
fn o_piso_nao_passa_do_que_a_coluna_tem() {
    let gigante = r##"{"type": "panel", "minHeight": 5000, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "curto"}]}]}"##;

    let Some(altura) = altura_da_moldura(gigante) else { return };
    assert!(
        altura < 800.0,
        "uma moldura de 5000pt tem de parar no que a coluna oferece, e mede {altura}"
    );
}
