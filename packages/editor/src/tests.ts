/**
 * Browser-side tests for the parts of the editor that are easy to get subtly
 * wrong: byte/index conversion, caret mapping through the display list, and the
 * document mutations that back typing.
 *
 * They run against the real WebAssembly engine and real fonts, because a mocked
 * display list would prove nothing about the mapping.
 *
 * Open `/tests.html`, or check it headlessly:
 * `google-chrome --headless --dump-dom http://127.0.0.1:5180/tests.html`
 */

import { Engine } from "./engine";
import { Store, normalize, parseLen } from "./store";
import { TextEditor, byteToIndex, indexToByte, utf8Length } from "./text";
import { caretAt, caretGeometry, collectRuns, compareCarets, frameAt } from "./hit";
import { placement, toFrame, trace } from "./contour";
import { PAGE_GAP, Renderer, placePages, pointIn } from "./renderer";
import { LayersPanel } from "./layers";
import type { LayersState } from "./layers";
import type { Block, Caret, DisplayItem, DisplayList, DocumentSpec, DisplayPage, Paragraph } from "./types";

const results: { name: string; error: string | null }[] = [];

function check(name: string, body: () => void): void {
  try {
    body();
    results.push({ name, error: null });
  } catch (error) {
    results.push({ name, error: error instanceof Error ? error.message : String(error) });
  }
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function equal<T>(actual: T, expected: T, message: string): void {
  if (actual !== expected) {
    throw new Error(`${message}: esperado ${JSON.stringify(expected)}, veio ${JSON.stringify(actual)}`);
  }
}

function near(actual: number, expected: number, tolerance: number, message: string): void {
  if (Math.abs(actual - expected) > tolerance) {
    throw new Error(`${message}: esperado ~${expected}, veio ${actual}`);
  }
}

// ─────────────────────────────────────────────────────────────────────────────

const SIMPLE: DocumentSpec = {
  style: { fontFamily: "corpo", fontSize: 12 },
  pages: [
    {
      frames: [
        {
          id: "caixa",
          type: "text",
          rect: [40, 40, 400, 200],
          blocks: [
            {
              type: "paragraph",
              content: [
                { type: "text", text: "Ação e coração " },
                { type: "text", text: "negrito", style: { fontWeight: "bold" } },
              ],
            },
            { type: "paragraph", content: [{ type: "text", text: "Segundo parágrafo" }] },
          ],
        },
      ],
    },
  ],
};

const THREADED: DocumentSpec = {
  style: { fontFamily: "corpo", fontSize: 12 },
  resources: {
    stories: {
      corpo: [
        { type: "paragraph", content: [{ type: "text", text: "curto" }] },
        {
          type: "paragraph",
          content: [
            {
              type: "text",
              text: "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima",
            },
          ],
        },
      ],
    },
  },
  pages: [
    {
      frames: [
        { id: "a", type: "text", rect: [0, 0, 120, 50], story: "corpo", threadNext: "b" },
        { id: "b", type: "text", rect: [200, 0, 120, 400] },
      ],
    },
  ],
};

async function run(): Promise<void> {
  const engine = new Engine();
  await engine.start();

  // ── Offset conversion ─────────────────────────────────────────────────────

  check("utf8Length conta bytes, não unidades UTF-16", () => {
    equal(utf8Length("abc"), 3, "ascii");
    equal(utf8Length("ção"), 5, "dois caracteres de 2 bytes");
    equal(utf8Length("😀"), 4, "par substituto");
  });

  check("byteToIndex e indexToByte são inversas", () => {
    const text = "Ação e coração";
    for (let index = 0; index <= text.length; index += 1) {
      const byte = indexToByte(text, index);
      // Only exact code-point boundaries round-trip; skip the low surrogate.
      if (byteToIndex(text, byte) !== index) {
        throw new Error(`índice ${index} não voltou (byte ${byte})`);
      }
    }
  });

  check("byteToIndex satura nos limites", () => {
    equal(byteToIndex("ação", -5), 0, "negativo");
    equal(byteToIndex("ação", 999), 4, "além do fim");
  });

  // ── Caret mapping ─────────────────────────────────────────────────────────

  const store = new Store(engine, normalize(SIMPLE));
  const page = store.list.pages[0]!;

  check("o frame é encontrado pelo ponto", () => {
    const hit = frameAt(page, 60, 60);
    assert(hit, "nada sob o ponto");
    equal(hit.id, "caixa", "frame errado");
    equal(frameAt(page, 5, 5), null, "fora do frame deveria dar nulo");
  });

  check("clicar no início do texto dá deslocamento 0", () => {
    const runs = collectRuns(page);
    assert(runs.length >= 2, "esperava ao menos dois runs");
    const first = runs[0]!;
    const caret = caretAt(store.list, page, first.run.x + 0.5, first.run.y - 2, "caixa");
    assert(caret, "sem caret");
    equal(caret.block, 0, "bloco");
    equal(caret.inline, 0, "inline");
    equal(caret.offset, 0, "deslocamento");
  });

  check("cada fronteira de glifo volta ao mesmo caret", () => {
    const runs = collectRuns(page);
    const target = runs[0]!;
    const base = target.source.offset ?? 0;

    for (const glyph of target.run.glyphs) {
      const expected: Caret = {
        frame: "caixa",
        story: null,
        block: target.source.block ?? 0,
        inline: target.source.inline ?? 0,
        offset: base + glyph.cluster,
      };
      const geometry = caretGeometry(store.list, page, expected);
      assert(geometry, `sem geometria para o deslocamento ${expected.offset}`);
      near(geometry.x, target.run.x + glyph.x, 0.001, "x do caret");

      const back = caretAt(store.list, page, geometry.x, geometry.top + geometry.height / 2, "caixa");
      assert(back, "não voltou");
      equal(
        compareCarets(back, expected),
        0,
        `ida e volta falhou no deslocamento ${expected.offset}`,
      );
    }
  });

  check("o caret cai no run do segundo parágrafo", () => {
    const runs = collectRuns(page).filter((placed) => placed.source.block === 1);
    assert(runs.length > 0, "segundo parágrafo não foi pintado");
    const target = runs[0]!;
    const caret = caretAt(store.list, page, target.run.x + 1, target.run.y - 2, "caixa");
    assert(caret, "sem caret");
    equal(caret.block, 1, "bloco");
  });

  // ── Mutations ─────────────────────────────────────────────────────────────

  check("digitar insere no lugar certo, depois de acentos", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const editor = new TextEditor(local);
    // "Ação" ocupa 6 bytes (A=1, ç=2, ã=2, o=1); o caret vai logo depois.
    editor.enter("caixa", { frame: "caixa", story: null, block: 0, inline: 0, offset: 6 });
    editor.insert("X");

    const paragraph = local.doc.pages[0]!.frames[0] as { blocks: Paragraph[] };
    const run = paragraph.blocks[0]!.content[0] as { text: string };
    equal(run.text, "AçãoX e coração ", "texto após inserção");
    equal(editor.caret!.offset, 7, "o caret avançou um byte");
  });

  check("backspace remove um caractere inteiro, não meio acento", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const editor = new TextEditor(local);
    editor.enter("caixa", { frame: "caixa", story: null, block: 0, inline: 0, offset: 6 });
    editor.deleteBackward();

    const frame = local.doc.pages[0]!.frames[0] as { blocks: Paragraph[] };
    const run = frame.blocks[0]!.content[0] as { text: string };
    equal(run.text, "Açã e coração ", "o 'o' saiu inteiro");
    equal(editor.caret!.offset, 5, "o caret recuou os bytes do caractere");
  });

  check("Enter divide o parágrafo", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const editor = new TextEditor(local);
    editor.enter("caixa", { frame: "caixa", story: null, block: 0, inline: 0, offset: 6 });
    editor.splitParagraph();

    const frame = local.doc.pages[0]!.frames[0] as { blocks: Paragraph[] };
    equal(frame.blocks.length, 3, "número de blocos");
    equal((frame.blocks[0]!.content[0] as { text: string }).text, "Ação", "cabeça");
    equal((frame.blocks[1]!.content[0] as { text: string }).text, " e coração ", "cauda");
    equal(editor.caret!.block, 1, "o caret foi para o novo bloco");
  });

  check("quebra de página divide o parágrafo e insere o bloco", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const editor = new TextEditor(local);
    editor.enter("caixa", { frame: "caixa", story: null, block: 0, inline: 0, offset: 6 });
    editor.insertPageBreak();

    const frame = local.doc.pages[0]!.frames[0] as { blocks: Block[] };
    // paragraph(cabeça) · pageBreak · paragraph(cauda) · paragraph original
    equal(frame.blocks.length, 4, "número de blocos");
    equal(frame.blocks[1]!.type, "pageBreak", "o bloco do meio é a quebra");

    const head = frame.blocks[0] as Paragraph;
    const tail = frame.blocks[2] as Paragraph;
    equal((head.content[0] as { text: string }).text, "Ação", "cabeça");
    equal((tail.content[0] as { text: string }).text, " e coração ", "cauda");

    // O caret segue o texto para depois da quebra.
    equal(editor.caret!.block, 2, "bloco do caret");
  });

  check("uma quebra de página realmente cria uma página", () => {
    const doc: DocumentSpec = {
      style: { fontFamily: "corpo", fontSize: 12 },
      pages: [
        {
          frames: [
            {
              id: "corpo",
              type: "text",
              rect: [0, 0, 300, 400],
              autoFlow: true,
              blocks: [
                { type: "paragraph", content: [{ type: "text", text: "antes" }] },
                { type: "pageBreak" },
                { type: "paragraph", content: [{ type: "text", text: "depois" }] },
              ],
            },
          ],
        },
      ],
    };

    const local = new Store(engine, normalize(doc));
    equal(local.list.pages.length, 2, "número de páginas");

    const textOf = (index: number) =>
      collectRuns(local.list.pages[index]!)
        .map((placed) => placed.run.text)
        .join("");
    assert(textOf(0).includes("antes"), "a primeira página perdeu o início");
    assert(textOf(1).includes("depois"), "a segunda página não recebeu o resto");
  });

  check("aplicar estilo divide o run e marca só a seleção", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const editor = new TextEditor(local);
    editor.enter("caixa", { frame: "caixa", story: null, block: 0, inline: 0, offset: 0 });
    editor.place({ frame: "caixa", story: null, block: 0, inline: 0, offset: 6 }, true);
    editor.applyStyle({ fontWeight: "bold" });

    const frame = local.doc.pages[0]!.frames[0] as { blocks: Paragraph[] };
    const content = frame.blocks[0]!.content as { text: string; style?: { fontWeight?: unknown } }[];
    equal(content[0]!.text, "Ação", "trecho marcado");
    equal(content[0]!.style?.fontWeight, "bold", "peso aplicado");
    equal(content[1]!.text, " e coração ", "resto intacto");
    assert(content[1]!.style?.fontWeight === undefined, "o resto não deveria estar em negrito");
  });

  check("desfazer volta ao estado anterior", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const editor = new TextEditor(local);
    editor.enter("caixa", { frame: "caixa", story: null, block: 0, inline: 0, offset: 0 });
    editor.insert("Z");
    assert(local.canUndo(), "deveria haver o que desfazer");
    local.undo();

    const frame = local.doc.pages[0]!.frames[0] as { blocks: Paragraph[] };
    equal((frame.blocks[0]!.content[0] as { text: string }).text, "Ação e coração ", "texto restaurado");
  });

  const NESTED: DocumentSpec = {
    style: { fontFamily: "corpo", fontSize: 12 },
    pages: [
      {
        frames: [
          { id: "a", type: "shape", shape: "rect", rect: [10, 10, 50, 50] },
          { id: "b", type: "shape", shape: "rect", rect: [100, 40, 50, 50] },
          { id: "c", type: "shape", shape: "rect", rect: [200, 10, 50, 50] },
        ],
      },
    ],
  };

  check("agrupar envolve os frames sem movê-los na página", () => {
    const local = new Store(engine, normalize(NESTED));
    const before = local.list.pages[0]!.frames.find((f) => f.id === "a")!.rect;

    const groupId = local.group(["a", "b"]);
    assert(groupId, "não agrupou");

    const page = local.doc.pages[0]!;
    equal(page.frames.length, 2, "grupo mais o frame solto");

    const group = page.frames.find((f) => f.id === groupId)!;
    assert(group.type === "group", "não é um grupo");
    // Caixa envolvente: de (10,10) a (150,90).
    equal(Number(group.rect[0]), 10, "x do grupo");
    equal(Number(group.rect[1]), 10, "y do grupo");
    equal(Number(group.rect[2]), 140, "largura do grupo");
    equal(Number(group.rect[3]), 80, "altura do grupo");

    // O que importa: nada se moveu na página.
    const after = local.list.pages[0]!.frames.find((f) => f.id === "a")!.rect;
    near(after.x, before.x, 0.01, "x de 'a' na página");
    near(after.y, before.y, 0.01, "y de 'a' na página");
  });

  check("desagrupar devolve as coordenadas absolutas", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;
    const inside = local.list.pages[0]!.frames.find((f) => f.id === "b")!.rect;

    const ids = local.ungroup(groupId);
    equal(ids.length, 2, "dois filhos liberados");
    equal(local.doc.pages[0]!.frames.length, 3, "voltou a três frames");

    const after = local.list.pages[0]!.frames.find((f) => f.id === "b")!.rect;
    near(after.x, inside.x, 0.01, "x preservado");
    near(after.y, inside.y, 0.01, "y preservado");
  });

  check("mover para dentro de um grupo reescreve as coordenadas", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;
    const before = local.list.pages[0]!.frames.find((f) => f.id === "c")!.rect;

    local.moveFrame("c", 0, groupId, 0);

    const located = local.locate("c")!;
    // Agora é filho: o rect passa a ser relativo à origem do grupo (10,10).
    equal(Number(located.frame.rect[0]), 190, "x relativo");
    equal(Number(located.frame.rect[1]), 0, "y relativo");

    // E, de novo, nada se moveu na página.
    const after = local.list.pages[0]!.frames.find((f) => f.id === "c")!.rect;
    near(after.x, before.x, 0.01, "x na página");
    near(after.y, before.y, 0.01, "y na página");
  });

  check("um grupo não pode ser solto dentro de si mesmo", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;
    local.moveFrame(groupId, 0, groupId, 0);
    // Continua na página, não sumiu dentro de si.
    assert(local.locate(groupId), "o grupo desapareceu");
    equal(local.doc.pages[0]!.frames.length, 2, "estrutura intacta");
  });

  check("avançar um nível troca com o vizinho", () => {
    const local = new Store(engine, normalize(NESTED));
    equal(local.locate("a")!.index, 0, "posição inicial");
    local.nudgeOrder("a", 1);
    equal(local.locate("a")!.index, 1, "subiu um");
    local.nudgeOrder("a", -1);
    equal(local.locate("a")!.index, 0, "voltou");
    // Nos extremos não faz nada.
    local.nudgeOrder("a", -1);
    equal(local.locate("a")!.index, 0, "parou no fim da lista");
  });

  check("parseLen resolve as unidades do schema", () => {
    equal(parseLen(12), 12, "número");
    equal(parseLen("12"), 12, "número em texto");
    equal(parseLen("1in"), 72, "polegada");
    near(parseLen("10mm"), 28.3465, 0.001, "milímetro");
    near(parseLen("1cm"), 28.3465, 0.001, "centímetro");
    equal(parseLen("16px"), 12, "pixel CSS");
    // Nunca devolve NaN: um NaN vira `null` no JSON e o motor recusa o documento.
    equal(parseLen("nada"), 0, "lixo");
    equal(parseLen(undefined), 0, "ausente");
  });

  check("um documento escrito em milímetros sobrevive a copiar e agrupar", () => {
    const inUnits: DocumentSpec = {
      style: { fontFamily: "corpo", fontSize: 12 },
      pages: [
        {
          frames: [
            { id: "a", type: "shape", shape: "rect", rect: ["10mm", "10mm", "20mm", "20mm"] },
            { id: "b", type: "shape", shape: "rect", rect: ["40mm", "10mm", "20mm", "20mm"] },
          ],
        },
      ],
    };

    const local = new Store(engine, normalize(inUnits));

    // Depois da normalização, a geometria é numérica em pontos.
    const first = local.frame("a")!;
    near(Number(first.rect[0]), 28.3465, 0.01, "10mm em pontos");

    const ids = local.insertFrames(local.cloneFrames(["a"]), 0, 12, 12);
    equal(ids.length, 1, "copiou");
    assert(local.list.diagnostics.every((d) => d.severity !== "error"), "o motor recusou o documento");

    const groupId = local.group(["a", "b"]);
    assert(groupId, "não agrupou");
    const group = local.frame(groupId!)!;
    near(Number(group.rect[2]), parseLen("50mm"), 0.01, "largura do grupo");
  });

  // ── Resilience ────────────────────────────────────────────────────────────

  check("uma mutação inválida é desfeita, não deixa o editor quebrado", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const before = JSON.stringify(local.doc);

    // "abc" não é uma medida; o motor recusa o documento inteiro.
    const accepted = local.commit((doc) => void ((doc.page ??= {}).margins = "abc"));

    equal(accepted, false, "a mudança deveria ser recusada");
    equal(JSON.stringify(local.doc), before, "o documento deveria voltar ao que era");
    assert(local.lastError, "deveria haver uma explicação");
    assert(local.lastError!.includes("abc"), `mensagem inútil: ${local.lastError}`);

    // E o editor continua funcionando.
    assert(local.list.pages.length === 1, "o layout anterior foi preservado");
    equal(local.commit((doc) => void ((doc.page ??= {}).margins = 30)), true, "segue aceitando");
  });

  check("uma mutação válida limpa o erro anterior", () => {
    const local = new Store(engine, normalize(SIMPLE));
    local.commit((doc) => void ((doc.page ??= {}).margins = "abc"));
    assert(local.lastError, "erro esperado");
    local.commit((doc) => void ((doc.page ??= {}).margins = 20));
    equal(local.lastError, null, "o erro deveria sumir");
  });

  check("uma recusa não entra na pilha de desfazer", () => {
    const local = new Store(engine, normalize(SIMPLE));
    local.commit((doc) => void ((doc.page ??= {}).margins = 25));
    const depth = local.canUndo();
    local.commit((doc) => void ((doc.page ??= {}).margins = "20mmm"));
    // Desfazer deve voltar à margem 25, não a um passo fantasma.
    assert(depth, "deveria haver o que desfazer");
    local.undo();
    assert(local.doc.page?.margins !== "20mmm", "o passo recusado vazou para o histórico");
  });

  // ── Pages ─────────────────────────────────────────────────────────────────

  check("mover um frame para outra página o transfere de verdade", () => {
    const twoPages: DocumentSpec = {
      style: { fontFamily: "corpo", fontSize: 12 },
      pages: [
        { frames: [{ id: "solto", type: "shape", shape: "rect", rect: [10, 10, 50, 50] }] },
        { frames: [] },
      ],
    };

    const local = new Store(engine, normalize(twoPages));
    equal(local.locate("solto")!.page, 0, "começa na página 1");

    local.moveToPage(["solto"], 1, 0, 0);

    equal(local.locate("solto")!.page, 1, "terminou na página 2");
    equal(local.doc.pages[0]!.frames.length, 0, "saiu da página 1");
    equal(local.doc.pages[1]!.frames.length, 1, "entrou na página 2");

    // E aparece no display list da página certa.
    const onSecond = local.list.pages[1]!.frames.some((f) => f.id === "solto");
    assert(onSecond, "o frame não foi pintado na página 2");
  });

  check("mover para a mesma página não faz nada", () => {
    const local = new Store(engine, normalize(NESTED));
    const before = JSON.stringify(local.doc);
    local.moveToPage(["a"], 0, 5, 5);
    equal(JSON.stringify(local.doc), before, "não deveria ter mexido");
  });

  // ── Hierarchy ─────────────────────────────────────────────────────────────

  check("clicar dentro de um grupo seleciona o grupo, não o filho", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;
    const page = local.list.pages[0]!;

    // Um ponto claramente dentro do frame "a" (10,10 a 60,60).
    const hit = frameAt(page, 20, 20);
    assert(hit, "nada sob o ponto");
    equal(hit.id, groupId, "deveria selecionar o grupo");
  });

  check("arrastar o grupo leva os filhos junto", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;
    const before = local.list.pages[0]!.frames.find((f) => f.id === "a")!.rect;

    local.commit(() => {
      const group = local.frame(groupId)!;
      group.rect[0] = Number(group.rect[0]) + 30;
      group.rect[1] = Number(group.rect[1]) + 15;
    });

    const after = local.list.pages[0]!.frames.find((f) => f.id === "a")!.rect;
    near(after.x, before.x + 30, 0.01, "o filho acompanhou em x");
    near(after.y, before.y + 15, 0.01, "o filho acompanhou em y");
  });

  check("a marca de seleção por área ignora filhos de grupo", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;
    const page = local.list.pages[0]!;

    const selectable = page.frames.filter((f) => f.ancestors.length === 0 && !f.locked);
    const ids = selectable.map((f) => f.id).sort();
    equal(ids.join(","), [groupId, "c"].sort().join(","), "só o grupo e o frame solto");
  });

  check("copiar produz cópias independentes com ids novos", () => {
    const local = new Store(engine, normalize(NESTED));
    const copies = local.cloneFrames(["a", "b"]);
    equal(copies.length, 2, "duas cópias");

    const ids = local.insertFrames(copies, 0, 12, 12);
    equal(ids.length, 2, "dois ids");
    assert(!ids.includes("a") && !ids.includes("b"), "ids precisam ser novos");
    equal(local.doc.pages[0]!.frames.length, 5, "três originais mais duas cópias");

    // A cópia sai deslocada, e o original fica onde estava.
    const original = local.list.pages[0]!.frames.find((f) => f.id === "a")!.rect;
    const copy = local.list.pages[0]!.frames.find((f) => f.id === ids[0])!.rect;
    near(copy.x, original.x + 12, 0.01, "x deslocado");
    near(copy.y, original.y + 12, 0.01, "y deslocado");
    near(original.x, 10, 0.01, "o original não se moveu");
  });

  check("copiar um filho de grupo devolve coordenadas absolutas", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;
    const before = local.list.pages[0]!.frames.find((f) => f.id === "b")!.rect;

    const ids = local.insertFrames(local.cloneFrames(["b"]), 0);
    const copy = local.list.pages[0]!.frames.find((f) => f.id === ids[0])!.rect;

    // Fora do grupo, mas exatamente no mesmo lugar da página.
    near(copy.x, before.x, 0.01, "x absoluto");
    near(copy.y, before.y, 0.01, "y absoluto");
    assert(local.locate(ids[0]!)!.siblings === local.doc.pages[0]!.frames, "deveria estar na raiz");
    void groupId;
  });

  check("copiar um grupo leva os filhos junto, todos renomeados", () => {
    const local = new Store(engine, normalize(NESTED));
    const groupId = local.group(["a", "b"])!;

    const ids = local.insertFrames(local.cloneFrames([groupId]), 0, 20, 20);
    const copy = local.frame(ids[0]!)!;
    assert(copy.type === "group", "a cópia deveria ser um grupo");
    equal(copy.children.length, 2, "dois filhos");

    const childIds = copy.children.map((c) => c.id);
    assert(!childIds.includes("a") && !childIds.includes("b"), "filhos precisam de ids novos");
  });

  check("a cópia não herda o encadeamento do original", () => {
    const local = new Store(engine, normalize(THREADED));
    const ids = local.insertFrames(local.cloneFrames(["a"]), 0, 10, 10);
    const copy = local.frame(ids[0]!)!;
    assert(copy.type === "text", "é um frame de texto");
    equal(copy.threadNext, undefined, "threadNext precisa ser limpo");
  });

  check("copiar texto devolve só o trecho selecionado", () => {
    const local = new Store(engine, normalize(SIMPLE));
    const editor = new TextEditor(local);
    editor.enter("caixa", { frame: "caixa", story: null, block: 0, inline: 0, offset: 0 });
    editor.place({ frame: "caixa", story: null, block: 0, inline: 0, offset: 6 }, true);
    equal(editor.selectedText(), "Ação", "trecho copiado");

    // Recortar devolve o mesmo e remove do documento.
    equal(editor.cut(), "Ação", "trecho recortado");
    const frame = local.doc.pages[0]!.frames[0] as { blocks: Paragraph[] };
    equal((frame.blocks[0]!.content[0] as { text: string }).text, " e coração ", "sobra");
  });

  // ── Threading ─────────────────────────────────────────────────────────────

  check("caret no frame encadeado aponta para a story original", () => {
    const threaded = new Store(engine, normalize(THREADED));
    const threadedPage: DisplayPage = threaded.list.pages[0]!;

    const carried = collectRuns(threadedPage).filter((placed) => placed.source.frame === "b");
    assert(carried.length > 0, "nada transbordou para o segundo frame");

    const target = carried[0]!;
    const caret = caretAt(threaded.list, threadedPage, target.run.x + 1, target.run.y - 2, "b");
    assert(caret, "sem caret");
    equal(caret.story, "corpo", "a story deveria ser preservada");

    const story = threaded.doc.resources!.stories!.corpo as Paragraph[];
    const source = story[caret.block] as Paragraph;
    const inline = source.content[caret.inline] as { text: string };
    const index = byteToIndex(inline.text, caret.offset);
    assert(
      inline.text.slice(index).startsWith(target.run.text.slice(0, 4)),
      `o deslocamento ${caret.offset} não bate com o texto pintado`,
    );
  });

  check("editar pelo segundo frame altera a story", () => {
    const threaded = new Store(engine, normalize(THREADED));
    const threadedPage = threaded.list.pages[0]!;
    const carried = collectRuns(threadedPage).filter((placed) => placed.source.frame === "b");
    assert(carried.length > 0, "nada transbordou");

    const editor = new TextEditor(threaded);
    const target = carried[0]!;
    const caret = caretAt(threaded.list, threadedPage, target.run.x + 1, target.run.y - 2, "b")!;
    editor.enter("b", caret);
    editor.insert("§");

    const story = threaded.doc.resources!.stories!.corpo as Paragraph[];
    const text = (story[caret.block]!.content[caret.inline] as { text: string }).text;
    assert(text.includes("§"), "a story não recebeu o caractere");
  });

  // ── Path primitive ────────────────────────────────────────────────────────

  /**
   * Paint one display list through the real renderer and read pixels back.
   *
   * Building a `Path2D` in the test would only prove the browser works. This
   * goes through `Renderer.render`, which is the code that has to agree with
   * the PDF emitter.
   */
  function pintado(items: DisplayItem[]): (x: number, y: number) => number {
    const canvas = document.createElement("canvas");
    canvas.width = 200;
    canvas.height = 200;
    // The renderer reads clientWidth/clientHeight, which are zero for a
    // detached element, so it has to be in the document to paint anything.
    canvas.style.width = "200px";
    canvas.style.height = "200px";
    canvas.style.position = "fixed";
    canvas.style.left = "-500px";
    document.body.append(canvas);

    const renderer = new Renderer(canvas, engine);
    const list: DisplayList = {
      version: 1,
      fonts: [],
      diagnostics: [],
      pages: [
        {
          index: 0,
          width: 200,
          height: 200,
          marginBox: { x: 0, y: 0, w: 200, h: 200 },
          frames: [],
          items,
        },
      ],
    };
    renderer.render(list, { zoom: 1, panX: 0, panY: 0 }, {
      selected: new Set(),
      hovered: null,
      editing: null,
      caret: null,
      caretVisible: false,
      highlights: [],
      guides: [],
      marquee: null,
      contours: new Map(),
    });

    const context = canvas.getContext("2d")!;
    const ratio = window.devicePixelRatio || 1;
    const [place] = placePages(list);

    // Darkness, not alpha: the renderer paints the white sheet before the
    // items, so every pixel inside the page is opaque and alpha measures the
    // paper. 0 is black ink, 255 is bare paper.
    return (x, y) => {
      const px = Math.round((place!.x + x) * ratio);
      const py = Math.round((place!.y + y) * ratio);
      return context.getImageData(px, py, 1, 1).data[0]!;
    };
  }

  check("o renderizador pinta um caminho onde a display list o põe", () => {
    const at = pintado([
      {
        type: "path",
        fill: "#000000",
        commands: [
          { op: "moveTo", x: 100, y: 20 },
          { op: "lineTo", x: 160, y: 140 },
          { op: "lineTo", x: 40, y: 140 },
          { op: "close" },
        ],
      },
    ]);

    assert(at(100, 110) < 40, "o interior do triângulo tem de estar pintado");
    assert(at(100, 8) > 200, "acima do ápice tem de ficar papel");
    assert(at(15, 30) > 200, "o canto de fora tem de ficar papel");
  });

  check("a regra par-ímpar abre buraco, a não-nula não", () => {
    // Two nested rings, same winding: only even-odd leaves the middle empty.
    const rings: DisplayItem = {
      type: "path",
      fill: "#000000",
      commands: [
        { op: "moveTo", x: 20, y: 20 },
        { op: "lineTo", x: 180, y: 20 },
        { op: "lineTo", x: 180, y: 180 },
        { op: "lineTo", x: 20, y: 180 },
        { op: "close" },
        { op: "moveTo", x: 70, y: 70 },
        { op: "lineTo", x: 130, y: 70 },
        { op: "lineTo", x: 130, y: 130 },
        { op: "lineTo", x: 70, y: 130 },
        { op: "close" },
      ],
    };

    const vazado = pintado([{ ...rings, fillRule: "evenOdd" }]);
    const cheio = pintado([{ ...rings, fillRule: "nonZero" }]);

    assert(vazado(100, 100) > 200, "par-ímpar tem de deixar o miolo em papel");
    assert(cheio(100, 100) < 40, "não-nula tem de preencher o miolo");
    assert(vazado(40, 100) < 40, "e a moldura fica pintada nos dois casos");
  });

  // ── Text wrap ─────────────────────────────────────────────────────────────

  const WRAPPED: DocumentSpec = {
    style: { fontFamily: "corpo", fontSize: 10 },
    pages: [
      {
        frames: [
          {
            id: "foto",
            type: "image",
            src: "ausente.png",
            rect: [200, 0, 120, 60],
            wrap: { mode: { kind: "contour", points: [[0, 0], [1, 0], [1, 1], [0, 1]] }, padding: 6 },
          },
          {
            id: "corpo",
            type: "text",
            rect: [0, 0, 440, 400],
            blocks: [
              {
                type: "paragraph",
                content: [
                  {
                    type: "text",
                    text: "Um parágrafo bem comprido que precisa correr dos dois lados da fotografia posta no meio da coluna.",
                  },
                ],
              },
            ],
          },
        ],
      },
    ],
  };

  check("normalizar preserva o contorno em vez de descartá-lo", () => {
    const doc = normalize(structuredClone(WRAPPED));
    const image = doc.pages[0]!.frames[0]!;
    assert(image.type === "image", "o primeiro frame é a imagem");
    assert(image.wrap, "o wrap sumiu na normalização");
    equal(image.wrap!.mode.kind, "contour", "o modo tem de sobreviver");
    assert(
      image.wrap!.mode.kind === "contour" && image.wrap!.mode.points.length === 4,
      "o anel tem de sobreviver inteiro",
    );
  });

  check("o contorno atravessa o editor e chega ao motor", () => {
    const local = new Store(engine, normalize(structuredClone(WRAPPED)));
    const runs = collectRuns(local.list.pages[0]!);
    assert(runs.length >= 2, "esperava várias linhas");

    // A picture at 200..320 plus 6 of clearance leaves 0..194 and 326..440.
    const first = runs[0]!.run;
    const beside = runs.find((placed) => Math.abs(placed.run.y - first.y) < 0.01
      && placed.run.x > 300);
    assert(
      beside,
      "sem um trecho à direita da foto o contorno não chegou ao motor",
    );
    assert(
      Math.abs(beside.run.x - 326) < 0.01,
      `o trecho da direita começa em ${beside.run.x}, esperava 326`,
    );
  });

  check("a figura declarada depois do texto é a que apanha o clique", () => {
    // A text frame's box covers the whole column, and it is transparent where
    // the picture sits. Whichever is declared last paints on top and takes the
    // click, so a picture meant to be dragged has to come after the text —
    // otherwise it is unreachable on the canvas and only the layers panel can
    // select it.
    const build = (imageFirst: boolean): DocumentSpec => {
      const foto = {
        id: "foto",
        type: "image" as const,
        src: "ausente.png",
        rect: [200, 40, 120, 60] as [number, number, number, number],
        wrap: { mode: { kind: "box" as const }, padding: 6 },
      };
      const corpo = {
        id: "corpo",
        type: "text" as const,
        rect: [0, 0, 440, 300] as [number, number, number, number],
        blocks: [{ type: "paragraph" as const, content: [{ type: "text" as const, text: "Texto" }] }],
      };
      return {
        style: { fontFamily: "corpo", fontSize: 10 },
        pages: [{ frames: imageFirst ? [foto, corpo] : [corpo, foto] }],
      };
    };

    const abaixo = new Store(engine, normalize(build(true)));
    const acima = new Store(engine, normalize(build(false)));
    // A point squarely inside the picture.
    const ponto: [number, number] = [260, 70];

    equal(
      frameAt(abaixo.list.pages[0]!, ...ponto)?.id,
      "corpo",
      "com a figura por baixo, o texto rouba o clique",
    );
    equal(
      frameAt(acima.list.pages[0]!, ...ponto)?.id,
      "foto",
      "com a figura por cima, ela é agarrável",
    );
  });

  check("mover uma imagem para outra página mantém-na visível", () => {
    const duas: DocumentSpec = {
      style: { fontFamily: "corpo", fontSize: 10 },
      page: { size: "A4" },
      pages: [
        {
          frames: [
            { id: "foto", type: "image", src: "terra.jpg", rect: [100, 200, 120, 80] },
          ],
        },
        { frames: [{ id: "alvo", type: "text", rect: [0, 0, 400, 300], blocks: [] }] },
      ],
    };

    // Pelo painel de camadas: soltar a linha na lista da outra página.
    const painel = new Store(engine, normalize(structuredClone(duas)));
    painel.moveFrame("foto", 1, null, 0);
    const naSegunda = painel.doc.pages[1]!.frames.find((f) => f.id === "foto");
    assert(naSegunda, "a imagem não chegou à segunda página");
    assert(
      painel.doc.pages[0]!.frames.every((f) => f.id !== "foto"),
      "a imagem ficou nas duas páginas",
    );
    equal(Number(naSegunda.rect[0]), 100, "x preservado");
    equal(Number(naSegunda.rect[1]), 200, "y preservado");

    const pintada = painel.list.pages[1]!.frames.find((f) => f.id === "foto");
    assert(pintada, "a imagem não aparece na display list da segunda página");
    assert(
      pintada.rect.y >= 0 && pintada.rect.y < painel.list.pages[1]!.height,
      `a imagem caiu fora da página: y=${pintada.rect.y}`,
    );
  });

  check("arrastar uma imagem para a página de baixo aterra no sítio certo", () => {
    const duas: DocumentSpec = {
      style: { fontFamily: "corpo", fontSize: 10 },
      page: { size: "A4" },
      pages: [
        { frames: [{ id: "foto", type: "image", src: "terra.jpg", rect: [100, 200, 120, 80] }] },
        { frames: [{ id: "alvo", type: "text", rect: [0, 0, 400, 300], blocks: [] }] },
      ],
    };
    const arraste = new Store(engine, normalize(structuredClone(duas)));
    const altura = arraste.list.pages[0]!.height;

    // O que o canvas faz: durante o arraste o rect segue o ponteiro para além
    // do fim da página; ao soltar, desconta-se a distância entre as páginas.
    const GAP = 28;
    arraste.commit((doc) => {
      const foto = doc.pages[0]!.frames[0]!;
      foto.rect[1] = altura + GAP + 150;
    });
    arraste.moveToPage(["foto"], 1, 0, -(altura + GAP));

    const pintada = arraste.list.pages[1]!.frames.find((f) => f.id === "foto");
    assert(pintada, "a imagem não chegou à segunda página");
    assert(
      Math.abs(pintada.rect.y - 150) < 1,
      `esperava y perto de 150 na página nova, veio ${pintada.rect.y}`,
    );
  });

  check("o arraste continua a medir na página onde começou", () => {
    // The bug this pins down: `toPage` answers in the coordinates of whichever
    // page the pointer is over, and those restart at zero on each sheet. A
    // drag that crossed onto the next page measured its delta between two
    // different origins, so the frame leapt — and dropping it there put it
    // hundreds of points above the paper, out of sight.
    const duas: DocumentSpec = {
      style: { fontFamily: "corpo", fontSize: 10 },
      page: { size: "A4" },
      pages: [
        { frames: [{ id: "a", type: "shape", shape: "rect", rect: [0, 0, 10, 10] }] },
        { frames: [{ id: "b", type: "shape", shape: "rect", rect: [0, 0, 10, 10] }] },
      ],
    };
    const local = new Store(engine, normalize(duas));
    const places = placePages(local.list);
    assert(places.length === 2, "esperava duas páginas colocadas");

    const segunda = places[1]!;
    // Um ponto a 150 pontos do topo da SEGUNDA página, em coordenadas do mundo.
    const worldX = segunda.x + 40;
    const worldY = segunda.y + 150;

    const naSegunda = pointIn(places, 1, worldX, worldY)!;
    near(naSegunda.y, 150, 0.01, "na página de destino, o ponto está a 150 do topo");

    const naPrimeira = pointIn(places, 0, worldX, worldY)!;
    assert(
      naPrimeira.y > local.list.pages[0]!.height,
      `medido a partir da primeira página o mesmo ponto tem de estar para lá do fim dela, veio ${naPrimeira.y}`,
    );

    // É esta a diferença que faz o arraste funcionar: contínua, e não a saltar.
    near(
      naPrimeira.y - naSegunda.y,
      local.list.pages[0]!.height + PAGE_GAP,
      0.01,
      "a distância entre as duas leituras é exactamente a altura da página mais o intervalo",
    );
  });

  check("um documento sem wrap continua sem wrap", () => {
    const local = new Store(engine, normalize(structuredClone(SIMPLE)));
    const frame = local.doc.pages[0]!.frames[0]!;
    assert(!("wrap" in frame) || !(frame as { wrap?: unknown }).wrap, "wrap apareceu do nada");
  });

  // ── Tracing a silhouette ──────────────────────────────────────────────────

  /** A bitmap drawn here, so the shape under test is known exactly. */
  async function painted(
    width: number,
    height: number,
    draw: (context: OffscreenCanvasRenderingContext2D) => void,
  ): Promise<ImageBitmap> {
    const canvas = new OffscreenCanvas(width, height);
    const context = canvas.getContext("2d")!;
    context.clearRect(0, 0, width, height);
    draw(context);
    return createImageBitmap(canvas);
  }

  const triangle = await painted(200, 200, (context) => {
    // Apex at the top, base along the bottom, transparent either side.
    context.fillStyle = "#000";
    context.beginPath();
    context.moveTo(100, 0);
    context.lineTo(200, 200);
    context.lineTo(0, 200);
    context.closePath();
    context.fill();
  });

  check("a silhueta de um triângulo estreita no topo e alarga na base", () => {
    const result = trace(triangle);
    assert(result, "não traçou nada");
    assert(result.points.length >= 6, `poucos pontos: ${result.points.length}`);
    equal(result.opaque, false, "há transparência nos cantos");

    // Width of the ring at a given height, from the two edges it carries.
    const widthAt = (target: number) => {
      const near = result.points.filter((p) => Math.abs(p[1] - target) < 0.06);
      const xs = near.map((p) => p[0]);
      return Math.max(...xs) - Math.min(...xs);
    };

    const top = widthAt(0.1);
    const bottom = widthAt(0.9);
    assert(
      bottom > top * 2,
      `a base (${bottom.toFixed(3)}) deveria ser bem mais larga que o topo (${top.toFixed(3)})`,
    );
  });

  check("traçar duas vezes a mesma imagem dá o mesmo anel", () => {
    const a = trace(triangle);
    const b = trace(triangle);
    equal(JSON.stringify(a), JSON.stringify(b), "o traçado precisa ser determinístico");
  });

  const solid = await painted(120, 80, (context) => {
    context.fillStyle = "#000";
    context.fillRect(0, 0, 120, 80);
  });

  check("uma imagem opaca é reconhecida como caixa, não como silhueta", () => {
    const result = trace(solid);
    assert(result, "uma imagem opaca ainda tem anel");
    equal(result.opaque, true, "não há nada a recortar");

    const xs = result.points.map((p) => p[0]);
    assert(
      Math.min(...xs) < 0.01 && Math.max(...xs) > 0.99,
      "o anel de uma imagem opaca cobre a largura toda",
    );
  });

  const blank = await painted(60, 60, () => {});

  check("uma imagem toda transparente não produz anel nenhum", () => {
    equal(trace(blank), null, "não há silhueta a traçar");
  });

  check("o anel é levado para dentro do frame conforme o ajuste da imagem", () => {
    const result = trace(triangle)!;
    // A 200×200 image inside a 400×200 frame, contained and centred: it
    // occupies the middle half of the frame, so the ring has to as well.
    const frame = {
      type: "image" as const,
      src: "x.png",
      rect: [0, 0, 400, 200] as [number, number, number, number],
      fit: "contain" as const,
      align: "center",
    };
    const box = placement(frame, { width: 200, height: 200 });
    equal(Math.round(box.w * 1000) / 1000, 0.5, "metade da largura do frame");
    equal(Math.round(box.x * 1000) / 1000, 0.25, "centrada");

    const moved = toFrame(result.points, box);
    const xs = moved.map((p) => p[0]);
    assert(
      Math.min(...xs) >= 0.249 && Math.max(...xs) <= 0.751,
      `o anel saiu da caixa da imagem: ${Math.min(...xs)}..${Math.max(...xs)}`,
    );
  });

  // ── Layers panel ──────────────────────────────────────────────────────────

  /** A panel over a detached root, wired the way `main.ts` wires the real one. */
  function mountLayers(spec: DocumentSpec) {
    const local = new Store(engine, normalize(spec));
    const root = document.createElement("div");
    const state: LayersState = { selected: new Set<string>(), activePage: 0, list: local.list };

    const paint = (): void => {
      state.list = local.list;
      panel.render(state);
    };

    const panel = new LayersPanel(root, local, {
      select: (id) => {
        state.selected = new Set([id]);
        paint();
      },
      focusPage: () => {},
      changed: paint,
    });

    paint();
    return { local, root, state, paint };
  }

  const rowIds = (root: HTMLElement): string[] =>
    [...root.querySelectorAll<HTMLElement>(".layer-row")].map((row) => row.dataset.id ?? "");

  const caretOf = (root: HTMLElement, id: string): HTMLButtonElement =>
    root.querySelector<HTMLButtonElement>(`.layer-row[data-id="${id}"] .layer-caret`)!;

  const pageCaret = (root: HTMLElement): HTMLButtonElement =>
    root.querySelector<HTMLButtonElement>(".page-row .layer-caret")!;

  check("os filhos de um grupo só aparecem quando ele é aberto", () => {
    const ui = mountLayers(NESTED);
    const groupId = ui.local.group(["a", "b"])!;
    ui.paint();

    assert(rowIds(ui.root).includes(groupId), "a linha do grupo sumiu");
    equal(rowIds(ui.root).includes("a"), false, "filho visível com o grupo fechado");

    caretOf(ui.root, groupId).click();
    assert(rowIds(ui.root).includes("a"), "abrir deveria mostrar os filhos");

    caretOf(ui.root, groupId).click();
    equal(rowIds(ui.root).includes("a"), false, "fechar deveria escondê-los de novo");
  });

  check("selecionar um frame abre o grupo que o contém", () => {
    const ui = mountLayers(NESTED);
    ui.local.group(["a", "b"]);
    ui.paint();
    equal(rowIds(ui.root).includes("b"), false, "deveria começar fechado");

    ui.state.selected = new Set(["b"]);
    ui.paint();
    assert(rowIds(ui.root).includes("b"), "a seleção deveria ter aberto o grupo");
  });

  check("um grupo fechado com a seleção dentro continua fechado", () => {
    const ui = mountLayers(NESTED);
    const groupId = ui.local.group(["a", "b"])!;
    ui.paint();

    ui.state.selected = new Set(["b"]);
    ui.paint();
    caretOf(ui.root, groupId).click();

    equal(rowIds(ui.root).includes("b"), false, "fechar não pode se desfazer no render seguinte");
    ui.paint();
    equal(rowIds(ui.root).includes("b"), false, "nem no render depois desse");
  });

  check("recolher a página esconde suas camadas e devolve depois", () => {
    const ui = mountLayers(NESTED);
    equal(rowIds(ui.root).length, 3, "três camadas na raiz");

    pageCaret(ui.root).click();
    equal(rowIds(ui.root).length, 0, "nenhuma camada com a página recolhida");
    assert(ui.root.querySelector(".page-row"), "a linha da página tem de continuar");

    pageCaret(ui.root).click();
    equal(rowIds(ui.root).length, 3, "reabrir devolve as camadas");
  });

  check("recolher tudo fecha páginas e grupos de uma vez", () => {
    const ui = mountLayers(NESTED);
    const groupId = ui.local.group(["a", "b"])!;
    ui.paint();
    caretOf(ui.root, groupId).click();
    assert(rowIds(ui.root).includes("a"), "o grupo deveria estar aberto");

    ui.root.querySelector<HTMLButtonElement>(".panel-title button")!.click();
    equal(rowIds(ui.root).length, 0, "tudo recolhido");

    ui.root.querySelector<HTMLButtonElement>(".panel-title button")!.click();
    assert(rowIds(ui.root).includes("a"), "expandir tudo abre também os grupos");
  });

  // ── Report ────────────────────────────────────────────────────────────────

  const container = document.querySelector("#results")!;
  container.replaceChildren();

  for (const result of results) {
    const line = document.createElement("div");
    line.className = result.error ? "fail" : "pass";
    line.textContent = result.error
      ? `FAIL  ${result.name} — ${result.error}`
      : `PASS  ${result.name}`;
    container.append(line);
  }

  const failed = results.filter((result) => result.error).length;
  const summary = document.querySelector("#summary")!;
  summary.textContent =
    failed === 0
      ? `TODOS OS ${results.length} TESTES PASSARAM`
      : `${failed} DE ${results.length} FALHARAM`;
  summary.className = failed === 0 ? "pass" : "fail";
}

run().catch((error) => {
  const summary = document.querySelector("#summary")!;
  summary.className = "fail";
  summary.textContent = `ERRO FATAL: ${String(error)}`;
  console.error(error);
});
