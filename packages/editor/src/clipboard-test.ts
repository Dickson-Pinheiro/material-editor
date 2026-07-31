/**
 * End-to-end check of the clipboard wiring.
 *
 * The unit tests cover what gets copied; this covers whether the keystroke
 * reaches it at all — the part that was broken, because browsers do not fire a
 * `copy` event when the focused field has no selection.
 *
 * It drives the real application: selects through the layers panel, then
 * dispatches the actual key events.
 */

const written: string[] = [];

// Stand in for the system clipboard before the app can capture a reference.
Object.defineProperty(navigator, "clipboard", {
  configurable: true,
  value: {
    writeText: (value: string) => {
      written.push(value);
      return Promise.resolve();
    },
    readText: () => Promise.resolve(written[written.length - 1] ?? ""),
  },
});

const results: string[] = [];
const errors: string[] = [];

// `dispatchEvent` swallows listener exceptions, so they are captured here or
// a broken handler looks like a silent no-op.
window.addEventListener("error", (event) => {
  errors.push(event.message + " @ " + (event.error?.stack ?? "").split("\n")[1]);
});

function check(name: string, body: () => void): void {
  try {
    body();
    results.push(`PASS  ${name}`);
  } catch (error) {
    results.push(`FAIL  ${name} — ${error instanceof Error ? error.message : String(error)}`);
  }
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function press(key: string, options: KeyboardEventInit = {}): void {
  window.dispatchEvent(
    new KeyboardEvent("keydown", { key, ctrlKey: true, bubbles: true, cancelable: true, ...options }),
  );
}

async function run(): Promise<void> {
  await import("./main");
  // Give the engine time to fetch its fonts and lay the document out.
  await wait(2500);

  const rows = [...document.querySelectorAll<HTMLElement>(".layer-row")];
  const frameCount = () => document.querySelectorAll(".layer-row").length;

  check("o app subiu com camadas visíveis", () => {
    assert(rows.length >= 2, `esperava camadas, veio ${rows.length}`);
  });

  const first = rows[0];
  if (first) {
    first.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await wait(120);

    check("Ctrl+C copia o frame selecionado", () => {
      written.length = 0;
      press("c");
      assert(written.length === 1, "nada foi escrito na área de transferência");
      const payload = JSON.parse(written[0]!) as { kind?: string; frames?: unknown[] };
      assert(payload.kind?.startsWith("diagramador/frames"), `carga inesperada: ${written[0]}`);
      assert(Array.isArray(payload.frames) && payload.frames.length === 1, "um frame copiado");
    });

    const before = frameCount();

    check("Ctrl+D duplica o frame selecionado", () => {
      press("d");
      assert(frameCount() === before + 1, `esperava ${before + 1} camadas, veio ${frameCount()}`);
    });

    check("Ctrl+V cola o que foi copiado", () => {
      const now = frameCount();
      document.dispatchEvent(
        Object.assign(new Event("paste", { bubbles: true, cancelable: true }), {
          clipboardData: { getData: () => written[0] ?? "" },
        }),
      );
      assert(frameCount() === now + 1, `colar não acrescentou camada (${frameCount()})`);
    });
  }

  for (const error of errors) results.push(`ERRO  ${error}`);

  const box = document.querySelector("#results")!;
  box.textContent = "";
  for (const line of results) {
    const div = document.createElement("div");
    div.textContent = line;
    div.style.color = line.startsWith("PASS") ? "#0f7b3f" : "#c2255c";
    box.append(div);
  }
  const failed = results.filter((r) => r.startsWith("FAIL")).length;
  const summary = document.createElement("div");
  summary.style.fontWeight = "700";
  summary.textContent =
    failed === 0 ? `TODOS OS ${results.length} TESTES PASSARAM` : `${failed} FALHARAM`;
  box.append(summary);
}

void run();
