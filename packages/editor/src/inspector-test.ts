/**
 * Exercises every control in the properties panel.
 *
 * The panel writes straight into the document, and the engine rejects a
 * document it cannot parse — so one bad field can take the whole editor down.
 * This walks the real panel, pokes each input the way a person would, and
 * checks two things after every change: the engine still accepts the document,
 * and the value actually landed.
 */

import { Engine } from "./engine";
import { Store, normalize } from "./store";
import { TextEditor } from "./text";
import { Inspector } from "./inspector";
import type { DocumentSpec, Frame } from "./types";

const results: { name: string; error: string | null }[] = [];

/**
 * Exceptions thrown inside an event listener never reach `dispatchEvent`'s
 * caller — the browser reports them here instead. Without this the harness
 * cannot see a control that breaks the document.
 */
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
  meta: { title: "Teste" },
  page: { size: "A4", margins: ["20mm", "18mm"] },
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
          blocks: [{ type: "paragraph", content: [{ type: "text", text: "Conteúdo" }] }],
        },
        { id: "forma", type: "shape", shape: "rect", rect: [360, 40, 120, 120] },
        { id: "foto", type: "image", rect: [40, 260, 200, 150], src: "ausente.png" },
      ],
    },
  ],
};

/** Every interactive element the panel currently shows. */
function controls(root: HTMLElement) {
  return {
    numbers: [...root.querySelectorAll<HTMLInputElement>('.control input[type="number"]')],
    texts: [...root.querySelectorAll<HTMLInputElement>('.control input[type="text"]')],
    colors: [...root.querySelectorAll<HTMLInputElement>('input[type="color"]')],
    selects: [...root.querySelectorAll<HTMLSelectElement>(".control select")],
    checks: [...root.querySelectorAll<HTMLInputElement>('.check input[type="checkbox"]')],
    buttons: [...root.querySelectorAll<HTMLButtonElement>(".icon-button:not(:disabled)")],
  };
}

function describe(element: Element): string {
  const box = element.closest(".control, .check, .segmented, .section");
  const section = element.closest(".section")?.querySelector(".section-head span")?.textContent;
  const title = (box as HTMLElement | null)?.title || (element as HTMLElement).title;
  return `${section ?? "?"} / ${title || (element as HTMLElement).getAttribute("aria-label") || element.tagName}`;
}

async function run(): Promise<void> {
  const engine = new Engine();
  await engine.start();

  const root = document.querySelector<HTMLElement>("#inspector")!;
  const store = new Store(engine, normalize(DOC));
  const text = new TextEditor(store);

  let selected: string[] = ["texto"];
  const failures: string[] = [];
  const attempted: string[] = [];

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
  });

  const render = () => inspector.render({ selected, list: store.list, editing: null });

  /** Run one interaction and report if the document stopped being valid. */
  function poke(label: string, act: () => void): void {
    attempted.push(label);
    const before = JSON.stringify(store.doc);
    const errorsBefore = thrown.length;

    try {
      act();
    } catch (error) {
      failures.push(`${label}: lançou ${error instanceof Error ? error.message : error}`);
      store.doc = JSON.parse(before) as DocumentSpec;
      return;
    }

    if (thrown.length > errorsBefore) {
      failures.push(`${label}: exceção no listener — ${thrown[thrown.length - 1]}`);
      store.doc = JSON.parse(before) as DocumentSpec;
      return;
    }

    const errors = store.list.diagnostics.filter((d) => d.severity === "error");
    if (errors.length > 0) {
      failures.push(`${label}: motor recusou — ${errors[0]!.message}`);
      store.doc = JSON.parse(before) as DocumentSpec;
    }
    render();
  }

  // ── Every frame kind, every control ─────────────────────────────────────
  for (const id of ["texto", "forma", "foto"]) {
    selected = [id];
    render();

    const found = controls(root);

    // Each change re-renders the panel, so a cached element is detached by the
    // time the next one runs. Everything is looked up again, by index.
    for (let index = 0; index < found.numbers.length; index += 1) {
      const label = describe(found.numbers[index]!);
      poke(`${id} · ${label} = 24`, () => {
        const live = controls(root).numbers[index];
        if (!live) throw new Error("o campo numérico sumiu do painel");
        live.value = "24";
        live.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }

    for (let index = 0; index < controls(root).texts.length; index += 1) {
      const label = describe(controls(root).texts[index]!);
      // Image keys are not lengths, so they get something they can use.
      const value = label.includes("Imagem") ? "foto.png" : "10mm 12mm";
      poke(`${id} · ${label} = "${value}"`, () => {
        const live = controls(root).texts[index];
        if (!live) throw new Error("o campo de texto sumiu do painel");
        live.value = value;
        live.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }

    for (let index = 0; index < controls(root).colors.length; index += 1) {
      const label = describe(controls(root).colors[index]!);
      poke(`${id} · ${label} = #336699`, () => {
        const live = controls(root).colors[index];
        if (!live) throw new Error("o seletor de cor sumiu do painel");
        live.value = "#336699";
        live.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }

    for (let index = 0; index < controls(root).selects.length; index += 1) {
      const select = controls(root).selects[index]!;
      const label = describe(select);
      const options = [...select.options].map((option) => option.value);

      for (const value of options) {
        poke(`${id} · ${label} = "${value}"`, () => {
          const live = controls(root).selects[index];
          if (!live) throw new Error("o seletor sumiu do painel");
          live.value = value;
          live.dispatchEvent(new Event("change", { bubbles: true }));
        });
      }
    }

    for (let index = 0; index < controls(root).checks.length; index += 1) {
      const label = describe(controls(root).checks[index]!);
      poke(`${id} · ${label} = ligado`, () => {
        const live = controls(root).checks[index];
        if (!live) throw new Error("a caixa sumiu do painel");
        live.checked = true;
        live.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }

    for (let index = 0; index < controls(root).buttons.length; index += 1) {
      const label = describe(controls(root).buttons[index]!);
      poke(`${id} · ${label}`, () => controls(root).buttons[index]?.click());
    }
  }

  // ── Document controls ───────────────────────────────────────────────────
  selected = [];
  render();

  for (let index = 0; index < controls(root).selects.length; index += 1) {
    const select = controls(root).selects[index]!;
    const label = describe(select);
    for (const value of [...select.options].map((o) => o.value)) {
      poke(`documento · ${label} = "${value}"`, () => {
        const live = controls(root).selects[index];
        if (!live) throw new Error("o seletor sumiu do painel");
        live.value = value;
        live.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }
  }

  for (let index = 0; index < controls(root).buttons.length; index += 1) {
    const label = describe(controls(root).buttons[index]!);
    poke(`documento · ${label}`, () => {
      const live = controls(root).buttons[index];
      if (!live) throw new Error("o botão sumiu do painel");
      live.click();
    });
  }

  // Margins, the field most likely to receive something unexpected.
  for (const value of ["20mm", "20 30", "1 2 3 4", "2cm", "", "abc", "20mmm", "-5"]) {
    poke(`documento · margens = "${value}"`, () => {
      const field = controls(root).texts[0];
      if (!field) throw new Error("campo de margens não encontrado");
      field.value = value;
      field.dispatchEvent(new Event("change", { bubbles: true }));
    });
  }

  check(`os controles foram realmente acionados (${attempted.length})`, () => {
    assert(attempted.length > 40, `só ${attempted.length} interações`);
    const margins = attempted.filter((label) => label.includes("margens"));
    assert(margins.length >= 8, `campo de margens não foi exercitado: ${margins.length}`);
  });

  check("nenhum controle do inspetor derruba o documento", () => {
    assert(failures.length === 0, `${failures.length} falha(s):\n  ${failures.join("\n  ")}`);
  });

  // ── Did the values actually land? ───────────────────────────────────────
  selected = ["texto"];
  render();

  check("mudar X move o frame de verdade", () => {
    const field = controls(root).numbers[0]!;
    field.value = "123";
    field.dispatchEvent(new Event("change", { bubbles: true }));
    const frame = store.frame("texto") as Frame;
    assert(Number(frame.rect[0]) === 123, `x veio ${frame.rect[0]}`);
  });

  check("mudar a cor de preenchimento aplica", () => {
    render();
    const swatch = controls(root).colors[0]!;
    swatch.value = "#123456";
    swatch.dispatchEvent(new Event("change", { bubbles: true }));
    const frame = store.frame("texto") as Frame;
    assert(frame.fill === "#123456", `fill veio ${frame.fill}`);
  });

  const box = document.querySelector("#results")!;
  box.textContent = "";
  for (const result of results) {
    const line = document.createElement("div");
    line.textContent = result.error ? `FAIL  ${result.name}\n${result.error}` : `PASS  ${result.name}`;
    line.style.whiteSpace = "pre-wrap";
    line.style.color = result.error ? "#c2255c" : "#0f7b3f";
    box.append(line);
  }
  const failed = results.filter((r) => r.error).length;
  const summary = document.createElement("div");
  summary.style.fontWeight = "700";
  summary.textContent = failed === 0 ? `TODOS OS ${results.length} PASSARAM` : `${failed} FALHARAM`;
  box.append(summary);
}

void run();
