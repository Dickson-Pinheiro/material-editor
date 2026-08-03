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
import { place } from "./table";
import type {
  ChartFrame,
  DocumentSpec,
  Frame,
  ImageFrame,
  ShapeFrame,
  TableBlock,
  TextFrame,
} from "./types";

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
        {
          id: "quadro",
          type: "text",
          rect: [40, 420, 300, 120],
          blocks: [
            {
              type: "table",
              columns: ["auto", "auto"],
              cells: [
                { blocks: [{ type: "paragraph", content: [{ type: "text", text: "Estado" }] }] },
                { blocks: [{ type: "paragraph", content: [{ type: "text", text: "Mudança" }] }] },
              ],
            },
          ],
        },
        {
          id: "grafico",
          type: "chart",
          rect: [360, 420, 240, 160],
          data: [
            { mes: "jan", v: 12 },
            { mes: "fev", v: 19 },
          ],
          encoding: { x: { field: "mes", kind: "categorical" }, y: { field: "v" } },
        },
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

/** A frame's padding spread over the four sides, however it was written. */
const inset = (store: Store, id: string): [unknown, unknown, unknown, unknown] => {
  const value = frameOf(store, id).padding ?? 0;
  const list = Array.isArray(value) ? value : [value];
  const [a = 0, b = a, c = a, d = b] = list;
  return [a, b, c, d];
};

/** The page margins spread over the four sides, however they were written. */
const sides = (store: Store): [unknown, unknown, unknown, unknown] => {
  const value = store.doc.page?.margins ?? 0;
  const list = Array.isArray(value) ? value : [value];
  const [a = 0, b = a, c = a, d = b] = list;
  return [a, b, c, d];
};
const textOf = (store: Store, id: string) => store.frame(id) as TextFrame;

const FRAME_PROBES: Probe[] = [
  { field: "rect.x", as: "number", set: 123, read: (s) => Number(frameOf(s, "texto").rect[0]), expect: 123 },
  { field: "rect.y", as: "number", set: 77, read: (s) => Number(frameOf(s, "texto").rect[1]), expect: 77 },
  { field: "rect.w", as: "number", set: 250, read: (s) => Number(frameOf(s, "texto").rect[2]), expect: 250 },
  { field: "rect.h", as: "number", set: 180, read: (s) => Number(frameOf(s, "texto").rect[3]), expect: 180 },
  { field: "rotation", as: "number", set: 15, read: (s) => frameOf(s, "texto").rotation, expect: 15 },
  { field: "radius", as: "number", set: 8, read: (s) => frameOf(s, "texto").radius, expect: 8 },
  { field: "opacity", as: "number", set: 40, read: (s) => frameOf(s, "texto").opacity, expect: 0.4, near: true },
  { field: "padding.top", as: "text", set: "6mm", read: (s) => inset(s, "texto")[0], expect: "6mm" },
  { field: "padding.right", as: "text", set: "8mm", read: (s) => inset(s, "texto")[1], expect: "8mm" },
  { field: "padding.bottom", as: "text", set: "10mm", read: (s) => inset(s, "texto")[2], expect: "10mm" },
  { field: "padding.left", as: "text", set: "12mm", read: (s) => inset(s, "texto")[3], expect: "12mm" },
  { field: "clip", as: "check", read: (s) => frameOf(s, "texto").clip, expect: true },
  { field: "fill", as: "color", set: "#336699", read: (s) => frameOf(s, "texto").fill, expect: "#336699" },
  { field: "border.color", as: "color", set: "#aa0044", read: (s) => frameOf(s, "texto").border?.color, expect: "#aa0044" },
  { field: "border.width", as: "number", set: 3, read: (s) => Number(frameOf(s, "texto").border?.width), expect: 3 },
  { field: "border.style", as: "select", value: "dashed", read: (s) => frameOf(s, "texto").border?.style, expect: "dashed" },
  { field: "border.sides.top", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.top, expect: false },
  { field: "border.sides.right", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.right, expect: false },
  { field: "border.sides.bottom", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.bottom, expect: false },
  { field: "border.sides.left", as: "button", read: (s) => frameOf(s, "texto").border?.sides?.left, expect: false },
  // Colunas primeiro: a medianiz só existe no painel quando há mais de uma.
  { field: "columns", as: "button", value: "3", read: (s) => textOf(s, "texto").columns, expect: 3 },
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
  // Uma por lado. A ordem importa: cada sonda parte do que a anterior deixou,
  // e o documento guarda a forma mais curta que ainda significa o mesmo.
  { field: "page.margins.top", as: "text", set: "25mm", read: (s) => sides(s)[0], expect: "25mm" },
  { field: "page.margins.right", as: "text", set: "30mm", read: (s) => sides(s)[1], expect: "30mm" },
  { field: "page.margins.bottom", as: "text", set: "35mm", read: (s) => sides(s)[2], expect: "35mm" },
  { field: "page.margins.left", as: "text", set: "40mm", read: (s) => sides(s)[3], expect: "40mm" },
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

/** The table in the `quadro` frame. */
const tableOf = (store: Store): TableBlock =>
  (store.frame("quadro") as TextFrame).blocks![0] as TableBlock;

/** The chart frame, whichever way it is being read. */
const chartOf = (store: Store): ChartFrame => store.frame("grafico") as ChartFrame;

/** How many places the table's grid resolves to. */
const gridOf = (store: Store) => place(tableOf(store));

const TABLE_PROBES: Probe[] = [
  // Order matters: each probe starts from what the last one left, and the
  // caret stays in cell 0 throughout, so every insertion is measured against
  // the column and row that cell is in.
  { field: "table.columnAfter", as: "button", read: (s) => gridOf(s).columns, expect: 3 },
  { field: "table.rowAfter", as: "button", read: (s) => gridOf(s).rows, expect: 2 },
  { field: "table.columnBefore", as: "button", read: (s) => gridOf(s).columns, expect: 4 },
  { field: "table.rowBefore", as: "button", read: (s) => gridOf(s).rows, expect: 3 },
  { field: "table.columnRemove", as: "button", read: (s) => gridOf(s).columns, expect: 3 },
  { field: "table.rowRemove", as: "button", read: (s) => gridOf(s).rows, expect: 2 },
  { field: "table.trackKind", as: "select", value: "fraction", read: (s) => tableOf(s).columns?.[0], expect: "1fr" },
  { field: "table.trackAmount", as: "number", set: 2, read: (s) => tableOf(s).columns?.[0], expect: "2fr" },
  { field: "table.inset", as: "number", set: 7, read: (s) => tableOf(s).inset, expect: 7 },
  { field: "table.gap", as: "number", set: 5, read: (s) => tableOf(s).columnGap, expect: 5 },
  { field: "table.header", as: "check", read: (s) => tableOf(s).header?.rows, expect: 1 },
  { field: "table.stripe", as: "check", read: (s) => tableOf(s).stripe?.every, expect: 2 },
  { field: "table.cellAlign", as: "select", value: "middle", read: (s) => tableOf(s).cells?.[0]?.verticalAlign, expect: "middle" },
];

const CHART_PROBES: Probe[] = [
  { field: "chart.mark", as: "button", value: "line", read: (s) => chartOf(s).mark, expect: "line" },
  { field: "chart.x.field", as: "select", value: "v", read: (s) => chartOf(s).encoding?.x?.field, expect: "v" },
  { field: "chart.x.kind", as: "select", value: "categorical", read: (s) => chartOf(s).encoding?.x?.kind, expect: "categorical" },
  { field: "chart.y.field", as: "select", value: "mes", read: (s) => chartOf(s).encoding?.y?.field, expect: "mes" },
  { field: "chart.y.kind", as: "select", value: "quantitative", read: (s) => chartOf(s).encoding?.y?.kind, expect: "quantitative" },
  { field: "chart.color.field", as: "select", value: "mes", read: (s) => chartOf(s).encoding?.color?.field, expect: "mes" },
  { field: "chart.color.kind", as: "select", value: "categorical", read: (s) => chartOf(s).encoding?.color?.kind, expect: "categorical" },
  { field: "chart.axes.x.title", as: "text", set: "Mês", read: (s) => chartOf(s).axes?.x?.title, expect: "Mês" },
  { field: "chart.axes.x.visible", as: "check", read: (s) => chartOf(s).axes?.x?.visible, expect: true },
  { field: "chart.axes.x.grid", as: "check", read: (s) => chartOf(s).axes?.x?.grid, expect: true },
  { field: "chart.axes.y.title", as: "text", set: "Valor", read: (s) => chartOf(s).axes?.y?.title, expect: "Valor" },
  { field: "chart.axes.y.visible", as: "check", read: (s) => chartOf(s).axes?.y?.visible, expect: true },
  { field: "chart.axes.y.grid", as: "check", read: (s) => chartOf(s).axes?.y?.grid, expect: true },
  { field: "chart.y.scale.kind", as: "select", value: "log", read: (s) => chartOf(s).encoding?.y?.scale?.kind, expect: "log" },
  { field: "chart.y.scale.zero", as: "check", read: (s) => chartOf(s).encoding?.y?.scale?.zero, expect: true },
  { field: "chart.legend.visible", as: "check", read: (s) => chartOf(s).legend?.visible, expect: true },
  { field: "chart.legend.position", as: "select", value: "bottom", read: (s) => chartOf(s).legend?.position, expect: "bottom" },
  { field: "data.0.mes", as: "text", set: "mar", read: (s) => chartOf(s).data?.[0]?.mes, expect: "mar" },
  { field: "data.0.v", as: "text", set: "42", read: (s) => chartOf(s).data?.[0]?.v, expect: 42 },
  { field: "data.addRow", as: "button", read: (s) => chartOf(s).data?.length, expect: 3 },
  { field: "data.addField", as: "button", read: (s) => Object.keys(chartOf(s).data?.[0] ?? {}).length, expect: 3 },
  { field: "data.2.remove", as: "button", read: (s) => chartOf(s).data?.length, expect: 2 },
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

  verify("tabela", TABLE_PROBES, () => {
    selected = ["quadro"];
    editing = "quadro";
    // The caret in the first cell is what puts the table in the panel.
    text.enter("quadro", {
      frame: "quadro",
      story: null,
      cells: [{ block: 0, cell: 0 }],
      block: 0,
      inline: 0,
      offset: 0,
    });
  });

  verify("gráfico", CHART_PROBES, () => {
    selected = ["grafico"];
    editing = null;
    text.exit();
  });

  verify("seleção de texto", SELECTION_PROBES, () => {
    selected = ["texto"];
    editing = "texto";
    // Select "Ação" so the marks have something to apply to.
    text.enter("texto", { cells: [], frame: "texto", story: null, block: 0, inline: 0, offset: 0 });
    text.place({ cells: [], frame: "texto", story: null, block: 0, inline: 0, offset: 6 }, true);
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
        text.enter("texto", { cells: [], frame: "texto", story: null, block: 0, inline: 0, offset: 0 });
        text.place({ cells: [], frame: "texto", story: null, block: 0, inline: 0, offset: 6 }, true);
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
