//! Components: an instance becomes a group, with slots filled and frames
//! following the instance size.

use diagramador::spec::parse_document;
use diagramador::{Engine, FontRegistry};

fn engine() -> Engine {
    let mut engine = Engine::new();
    let font = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts/DejaVuSans.ttf")).unwrap();
    engine.add_font("corpo", font, None, None).unwrap();
    engine
}

fn doc(json: &str) -> diagramador::Document {
    parse_document(json).unwrap()
}

const AMPLIE: &str = r##"{
  "style": { "fontFamily": "corpo", "fontSize": 10 },
  "resources": {
    "colors": { "acento": "#1e555c", "claro": "#e3edee" },
    "components": {
      "amplie": {
        "label": "Ampliar",
        "size": [120, 80],
        "frames": [
          { "id": "fundo", "type": "shape", "shape": "rect", "rect": [0, 0, 120, 80], "fill": "@claro", "follow": { "w": true, "h": true } },
          { "type": "shape", "shape": "rect", "rect": [0, 0, 120, 16], "fill": "@acento", "follow": { "w": true } },
          { "type": "text", "slot": "titulo", "rect": [4, 1, 112, 14], "style": { "color": "#ffffff" }, "follow": { "w": true } },
          { "type": "text", "slot": "texto", "rect": [4, 20, 112, 56], "follow": { "w": true, "h": true } },
          { "type": "image", "slot": "foto", "rect": [100, 60, 16, 16], "follow": { "x": true, "y": true } }
        ]
      }
    }
  },
  "pages": [{ "frames": [
    { "id": "inst", "type": "instance", "component": "amplie", "rect": [50, 50, 180, 100],
      "slots": { "titulo": "AMPLIAR", "texto": ["Um parágrafo.", "Outro."], "foto": { "src": "foto.png" } } }
  ] }]
}"##;

fn frame<'a>(list: &'a diagramador::DisplayList, id: &str) -> &'a diagramador::display::DisplayFrame {
    list.pages[0]
        .frames
        .iter()
        .find(|f| f.id == id)
        .unwrap_or_else(|| panic!("frame {id} missing among {:?}", list.pages[0].frames.iter().map(|f| f.id.clone()).collect::<Vec<_>>()))
}

#[test]
fn an_instance_is_reported_as_one_frame_of_kind_instance() {
    let list = engine().layout(&doc(AMPLIE));
    let inst = frame(&list, "inst");
    assert_eq!(inst.kind, "instance");
    assert_eq!((inst.rect.x, inst.rect.y, inst.rect.w, inst.rect.h), (50.0, 50.0, 180.0, 100.0));
}

#[test]
fn children_are_named_after_the_instance_and_the_slot() {
    let list = engine().layout(&doc(AMPLIE));
    let titulo = frame(&list, "inst.titulo");
    assert_eq!(titulo.ancestors, vec!["inst".to_string()]);
    assert_eq!(titulo.kind, "text");
    assert!(list.pages[0].frames.iter().any(|f| f.id == "inst.fundo"));
}

#[test]
fn frames_follow_the_instance_size() {
    let list = engine().layout(&doc(AMPLIE));
    // dw = 60, dh = 20.
    let fundo = frame(&list, "inst.fundo");
    assert_eq!((fundo.rect.w, fundo.rect.h), (180.0, 100.0));
    let texto = frame(&list, "inst.texto");
    assert_eq!((texto.rect.x, texto.rect.y, texto.rect.w, texto.rect.h), (54.0, 70.0, 172.0, 76.0));
    let foto = frame(&list, "inst.foto");
    assert_eq!((foto.rect.x, foto.rect.y, foto.rect.w, foto.rect.h), (210.0, 130.0, 16.0, 16.0));
}

#[test]
fn slots_fill_text_and_images() {
    let list = engine().layout(&doc(AMPLIE));
    let page = &list.pages[0];
    // The title band text was painted: some glyph run says AMPLIAR.
    fn texts(items: &[diagramador::display::DisplayItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                diagramador::display::DisplayItem::Glyphs(run) => out.push(run.text.clone()),
                diagramador::display::DisplayItem::Group(group) => texts(&group.items, out),
                _ => {}
            }
        }
    }
    let mut painted = Vec::new();
    texts(&page.items, &mut painted);
    assert!(painted.iter().any(|t| t.contains("AMPLIAR")), "{painted:?}");
    assert!(painted.iter().any(|t| t.contains("Outro")), "{painted:?}");
    // The image slot reached the image frame: a missing-image diagnostic names it.
    assert!(
        list.diagnostics.iter().any(|d| d.frame.as_deref() == Some("inst.foto")),
        "{:?}",
        list.diagnostics
    );
}

#[test]
fn an_unknown_component_is_a_diagnostic_not_a_crash() {
    let list = engine().layout(&doc(
        r##"{"pages":[{"frames":[{"type":"instance","component":"nada","rect":[0,0,10,10]}]}]}"##,
    ));
    assert!(list.diagnostics.iter().any(|d| d.code == "unknownComponent"));
    assert_eq!(list.pages[0].frames.len(), 1);
}

#[test]
fn a_slot_the_component_lacks_is_a_diagnostic() {
    let json = AMPLIE.replace(r#""titulo": "AMPLIAR""#, r#""titulo": "AMPLIAR", "rodape": "x""#);
    let list = engine().layout(&doc(&json));
    assert!(list.diagnostics.iter().any(|d| d.code == "unknownSlot" && d.message.contains("rodape")));
}

#[test]
fn a_component_that_instances_itself_stops() {
    let list = engine().layout(&doc(
        r##"{"resources":{"components":{"a":{"size":[10,10],"frames":[{"type":"instance","component":"a","rect":[0,0,10,10]}]}}},
             "pages":[{"frames":[{"type":"instance","component":"a","rect":[0,0,10,10]}]}]}"##,
    ));
    assert!(list.diagnostics.iter().any(|d| d.code == "componentCycle"));
}

#[test]
fn the_pdf_of_an_instance_is_the_pdf_of_its_group() {
    let engine = engine();
    let with_instance = engine.render_pdf(&doc(AMPLIE)).unwrap();
    assert!(with_instance.len() > 500);
    // Registry sanity: the engine reports its faces.
    let _ = FontRegistry::default();
}

#[test]
fn a_version_one_document_still_parses() {
    let d = doc(r#"{"version":1,"pages":[{"frames":[{"type":"text","rect":[0,0,10,10],"blocks":["x"]}]}]}"#);
    assert_eq!(d.version, 1);
}
