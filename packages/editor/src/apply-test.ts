/**
 * Verifies that every control in the properties panel writes what it claims.
 *
 * The earlier harness proved that no control *breaks* the document. This one
 * proves each one *lands*: for each control it sets a value, reads the document
 * back, and compares. Controls are found by the `data-field` they declare, so
 * a control that is renamed or dropped fails loudly instead of being skipped.
 */

import { Engine } from "./engine";
import { Store, normalize } from "./store";
import { TextEditor } from "./text";
import { Inspector } from "./inspector";
import type { DocumentSpec, Frame, ImageFrame, ShapeFrame, TextFrame } from "./types";

const results: { name: string; error: string | null }[] = [];
const thrown: string[] = [];
window.addEventListener("error", (event) => thrown.push(event.message));

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

const DOC: DocumentSpec = {
  meta: { title: "Verificação" },
  page: { size: "A4", margins: 40 },
  style: { fontFamily: "corpo", fontSize: 11 },
  pages: [
    {
      frames: [
        {
          id: "texto",
          type: "text",
          rect: [40, 40, 300, 200],
          fill: "#eeeeee",
          border: { width: 1, color: "#333333" },
          blocks: [{ type: "paragraph", content: [{ type: "text", text: "Ação e coração" }] }],
        },
        { id: "outro", type: "text", rect: [40, 300, 300, 100] },
        { id: "forma", type: "shape", shape: "rect", rect: [360, 40, 120, 120] },
        { id: "foto", type: "image", rect: [360, 200, 200, 150], src: "terra.jpg",
          wrap: { mode: { kind: "box" }, padding: 4 } },
      ],
    },
  ],
};

/** One control: how to drive it, and what the document should say afterwards. */
interface Probe {
  field: string;
  /** `select` and `check` are driven by their own element type. */
  as: "number" | "text" | "color" | "select" | "check" | "button";
  set?: string | number | boolean;
  /** Only for buttons and selects in a segmented row. */
  value?: string;
  read: (store: Store) => unknown;
  expect: unknown;
  /** Compare with a tolerance instead of exactly. */
  near?: boolean;
}

const frameOf = (store: Store, id: string) => store.frame(id) as Frame;
const textOf = (store: Store, id: string) => store.frame(id) as TextFrame;

const FRAME_PROBES: Probe[] = [
  { field: "rect.x", as: "number", set: 123, read: (s) => Number(frameOf(s, "texto").rect[0]), expect: 123 },
  { field: "rect.y", as: "number", set: 77, read: (s) => Number(frameOf(s, "texto").rect[1]), expect: 77 },
  { field: "rect.w", as: "number", set: 250, read: (s) => Number(frameOf(s, "texto").rect[2]), expect: 250 },
  { field: "rect.h", as: "number", set: 180, read: (s) => Number(frameOf(s, "texto").rect[3]), expect: 180 },
  { field: "rotation", as: "number", set: 15, read: (s) => frameOf(s, "texto").rotation, expect: 15 },
  { field: "radius", as: "number", set: 8, read: (s) => frameOf(s, "texto").radius, expect: 8 },
  { field: "opacity", as: "number", set: 40, read: (s) => frameOf(s, "texto").opacity, expect: 0.4, near: true },
  { field: "padding", as: "text", set: "6mm 8mm", read: (s) => JSON.stringify(frameOf(s, "texto").padding), expect: '["6mm","8mm"]' },
  { field: "clip", as: "check", read: (s) => frameOf(s, "texto").clip, expect: true },
  { field: "fill", as: "color", set: "#336699", read: (s) => frameOf(s, "texto").fill, expect: "#336699" },
  { field: "border.color", as: "color", set: "#aa0044", read: (s) => frameOf(s, "texto").border?.color, expect: "#aa0044" },
  { field: "border.width", as: "number", set: 3, read: (s) => Number(frameOf(s, "texto").border?.width), expect: 3 },
  { field: "border.style", as: "select", value: "dashed", read: (s) => frameOf(s, "texto").border?.style, expect: "dashed" },
  { field: "border.sides.top", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.top, expect: false },
  { field: "border.sides.right", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.right, expect: false },
  { field: "border.sides.bottom", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.bottom, expect: false },
  { field: "border.sides.left", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.left, expect: false },
  { field: "columns", as: "number", set: 2, read: (s) => textOf(s, "texto").columns, expect: 2 },
  { field: "columnGap", as: "number", set: 20, read: (s) => Number(textOf(s, "texto").columnGap), expect: 20 },
  { field: "verticalAlign", as: "button", value: "middle", read: (s) => textOf(s, "texto").verticalAlign, expect: "middle" },
  { field: "overflow", as: "select", value: "grow", read: (s) => textOf(s, "texto").overflow, expect: "grow" },
  { field: "threadNext", as: "select", value: "outro", read: (s) => textOf(s, "texto").threadNext, expect: "outro" },
  { field: "autoFlow", as: "check", read: (s) => textOf(s, "texto").autoFlow, expect: true },
  { field: "ignoreWrap", as: "check", read: (s) => textOf(s, "texto").ignoreWrap, expect: true },
  { field: "style.fontFamily", as: "select", value: "plex", read: (s) => textOf(s, "texto").style?.fontFamily, expect: "plex" },
  { field: "style.fontSize", as: "number", set: 14, read: (s) => Number(textOf(s, "texto").style?.fontSize), expect: 14 },
  { field: "style.lineHeight", as: "number", set: 1.8, read: (s) => Number(textOf(s, "texto").style?.lineHeight), expect: 1.8 },
  { field: "style.fontWeight", as: "select", value: "bold", read: (s) => textOf(s, "texto").style?.fontWeight, expect: "bold" },
  { field: "style.textAlign", as: "button", value: "justify", read: (s) => textOf(s, "texto").style?.textAlign, expect: "justify" },
  { field: "style.color", as: "color", set: "#112233", read: (s) => textOf(s, "texto").style?.color, expect: "#112233" },
  { field: "style.spaceBefore", as: "number", set: 9, read: (s) => Number(textOf(s, "texto").style?.spaceBefore), expect: 9 },
  { field: "style.spaceAfter", as: "number", set: 11, read: (s) => Number(textOf(s, "texto").style?.spaceAfter), expect: 11 },
  { field: "style.indentFirst", as: "number", set: 13, read: (s) => Number(textOf(s, "texto").style?.indentFirst), expect: 13 },
];

const SHAPE_PROBES: Probe[] = [
  { field: "shape", as: "button", value: "ellipse", read: (s) => (frameOf(s, "forma") as ShapeFrame).shape, expect: "ellipse" },
];

const IMAGE_PROBES: Probe[] = [
  // Traçar primeiro: as sondas partilham o documento, e trocar `src` depois
  // deixaria a imagem sem pixels para ler.
  { field: "image.wrap.trace", as: "button", read: (s) => (frameOf(s, "foto") as ImageFrame).wrap?.mode.kind, expect: "contour" },
  { field: "image.src", as: "text", set: "b.png", read: (s) => (frameOf(s, "foto") as ImageFrame).src, expect: "b.png" },
  { field: "image.fit", as: "select", value: "cover", read: (s) => (frameOf(s, "foto") as ImageFrame).fit, expect: "cover" },
  { field: "image.wrap.mode", as: "select", value: "contour", read: (s) => (frameOf(s, "foto") as ImageFrame).wrap?.mode.kind, expect: "contour" },
  { field: "image.wrap.padding", as: "text", set: "9", read: (s) => (frameOf(s, "foto") as ImageFrame).wrap?.padding, expect: 9 },
];

const DOC_PROBES: Probe[] = [
  { field: "page.size", as: "select", value: "A5", read: (s) => s.doc.page?.size, expect: "A5" },
  { field: "page.orientation", as: "button", value: "landscape", read: (s) => String(s.doc.page?.size).includes("landscape"), expect: true },
  { field: "page.margins", as: "text", set: "25mm", read: (s) => s.doc.page?.margins, expect: "25mm" },
  { field: "page.facing", as: "check", read: (s) => s.doc.page?.facing, expect: true },
];

const SELECTION_PROBES: Probe[] = [
  { field: "sel.bold", as: "button", read: (s) => runStyle(s)?.fontWeight, expect: "bold" },
  { field: "sel.italic", as: "button", read: (s) => runStyle(s)?.fontStyle, expect: "italic" },
  { field: "sel.underline", as: "button", read: (s) => runStyle(s)?.underline, expect: true },
  { field: "sel.strike", as: "button", read: (s) => runStyle(s)?.strikethrough, expect: true },
  { field: "sel.fontSize", as: "number", set: 18, read: (s) => Number(runStyle(s)?.fontSize), expect: 18 },
  { field: "sel.color", as: "color", set: "#ee0055", read: (s) => runStyle(s)?.color, expect: "#ee0055" },
  { field: "sel.fontFamily", as: "select", value: "plex", read: (s) => runStyle(s)?.fontFamily, expect: "plex" },
];

/** Style of the first run of the first paragraph — where marks land. */
function runStyle(store: Store) {
  const frame = store.frame("texto") as TextFrame;
  const block = frame.blocks?.[0];
  if (block?.type !== "paragraph") return undefined;
  const inline = block.content[0];
  return inline?.type === "text" ? inline.style : undefined;
}

async function run(): Promise<void> {
  const engine = new Engine();
  await engine.start();

  const root = document.querySelector<HTMLElement>("#inspector")!;
  const store = new Store(engine, normalize(DOC));
  const text = new TextEditor(store);

  let selected: string[] = [];
  let editing: string | null = null;

  const inspector = new Inspector(root, store, text, {
    frameChange(id, mutate) {
      store.commit(() => {
        const frame = store.frame(id);
        if (frame) mutate(frame);
      });
    },
    docChange: (mutate) => store.commit(mutate),
    textStyle: (patch) => text.applyStyle(patch),
    align: () => {},
    distribute: () => {},
    fontFamilies: () => engine.fontFamilies(),
    bitmapFor: (src) => engine.images().get(src) ?? null,
  });

  const render = () => inspector.render({ selected, list: store.list, editing });

  function drive(probe: Probe): void {
    const host = root.querySelector<HTMLElement>(`[data-field="${probe.field}"]`);

    if (probe.as === "button") {
      const selector = probe.value
        ? `button[data-field="${probe.field}"][data-value="${probe.value}"]`
        : `button[data-field="${probe.field}"]`;
      const button = root.querySelector<HTMLButtonElement>(selector);
      assert(button, `botão "${probe.field}" não está no painel`);
      button.click();
      return;
    }

    assert(host, `controle "${probe.field}" não está no painel`);

    if (probe.as === "select") {
      const select = host.querySelector("select")!;
      const options = [...select.options].map((o) => o.value);
      assert(options.includes(probe.value!), `"${probe.value}" não é opção de ${probe.field}`);
      select.value = probe.value!;
      select.dispatchEvent(new Event("change", { bubbles: true }));
      return;
    }

    if (probe.as === "check") {
      const input = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
      input.checked = true;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      return;
    }

    const input =
      probe.as === "color"
        ? host.querySelector<HTMLInputElement>('input[type="color"]')!
        : host.querySelector<HTMLInputElement>("input")!;
    input.value = String(probe.set);
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function verify(label: string, probes: Probe[], before: () => void): void {
    for (const probe of probes) {
      check(`${label} · ${probe.field}`, () => {
        before();
        render();

        const errorsBefore = thrown.length;
        drive(probe);
        assert(
          thrown.length === errorsBefore,
          `lançou: ${thrown[thrown.length - 1]}`,
        );

        const actual = probe.read(store);
        const ok = probe.near
          ? Math.abs(Number(actual) - Number(probe.expect)) < 0.001
          : actual === probe.expect;

        assert(
          ok,
          `esperava ${JSON.stringify(probe.expect)}, documento diz ${JSON.stringify(actual)}`,
        );
      });
    }
  }

  verify("texto", FRAME_PROBES, () => {
    selected = ["texto"];
    editing = null;
  });
  verify("forma", SHAPE_PROBES, () => {
    selected = ["forma"];
    editing = null;
  });
  verify("imagem", IMAGE_PROBES, () => {
    selected = ["foto"];
    editing = null;
  });
  verify("documento", DOC_PROBES, () => {
    selected = [];
    editing = null;
  });

  verify("seleção de texto", SELECTION_PROBES, () => {
    selected = ["texto"];
    editing = "texto";
    // Select "Ação" so the marks have something to apply to.
    text.enter("texto", { frame: "texto", story: null, block: 0, inline: 0, offset: 0 });
    text.place({ frame: "texto", story: null, block: 0, inline: 0, offset: 6 }, true);
  });

  // Nothing may be left untested: every declared field must appear above.
  check("todo controle do painel está coberto", () => {
    const covered = new Set(
      [...FRAME_PROBES, ...SHAPE_PROBES, ...IMAGE_PROBES, ...DOC_PROBES, ...SELECTION_PROBES].map(
        (probe) => probe.field,
      ),
    );

    const seen = new Set<string>();
    for (const [ids, editingId] of [
      [["texto"], null],
      [["forma"], null],
      [["foto"], null],
      [[], null],
      [["texto"], "texto"],
    ] as [string[], string | null][]) {
      selected = ids;
      editing = editingId;
      if (editingId) {
        text.enter("texto", { frame: "texto", story: null, block: 0, inline: 0, offset: 0 });
        text.place({ frame: "texto", story: null, block: 0, inline: 0, offset: 6 }, true);
      }
      render();
      for (const element of root.querySelectorAll<HTMLElement>("[data-field]")) {
        seen.add(element.dataset.field!);
      }
    }

    const missing = [...seen].filter((field) => !covered.has(field));
    assert(missing.length === 0, `sem verificação: ${missing.join(", ")}`);
  });

  const box = document.querySelector("#results")!;
  box.textContent = "";
  for (const result of results) {
    const line = document.createElement("div");
    line.textContent = result.error ? `FAIL  ${result.name} — ${result.error}` : `PASS  ${result.name}`;
    line.style.color = result.error ? "#c2255c" : "#0f7b3f";
    box.append(line);
  }
  const failed = results.filter((r) => r.error).length;
  const summary = document.createElement("div");
  summary.style.fontWeight = "700";
  summary.textContent =
    failed === 0 ? `TODOS OS ${results.length} PASSARAM` : `${failed} DE ${results.length} FALHARAM`;
  box.append(summary);
}

void run();
