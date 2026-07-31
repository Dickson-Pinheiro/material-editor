/**
 * Panel preview harness.
 *
 * Mounts the layers and inspector panels against a real document with a frame
 * already selected, so the states that need a selection can be looked at —
 * something the app itself only reaches through a click.
 */

import { Engine } from "./engine";
import { Store, normalize } from "./store";
import { TextEditor } from "./text";
import { Inspector } from "./inspector";
import { LayersPanel } from "./layers";
import type { DocumentSpec } from "./types";
import STARTER from "./two-pages.json";

async function run(): Promise<void> {
  const engine = new Engine();
  await engine.start();

  const store = new Store(engine, normalize(STARTER as unknown as DocumentSpec));
  const text = new TextEditor(store);

  const selected = new Set<string>(["quadro-atividade"]);

  const inspector = new Inspector(
    document.querySelector<HTMLElement>("#inspector")!,
    store,
    text,
    {
      frameChange: () => {},
      docChange: () => {},
      textStyle: () => {},
      align: () => {},
      distribute: () => {},
      fontFamilies: () => engine.fontFamilies(),
    },
  );

  const layers = new LayersPanel(document.querySelector<HTMLElement>("#layers")!, store, {
    select: () => {},
    focusPage: () => {},
    changed: () => {},
  });

  inspector.render({ selected: [...selected], list: store.list, editing: null });
  layers.render({ selected, activePage: 0, list: store.list });

  // Paint the document too, so the page stacking can be looked at.
  const { Renderer, documentExtent } = await import("./renderer");
  const canvas = document.querySelector<HTMLCanvasElement>("#canvas")!;
  const renderer = new Renderer(canvas, engine);
  renderer.resize();
  const extent = documentExtent(store.list);
  const zoom = Math.min(
    (canvas.clientWidth - 40) / extent.width,
    (canvas.clientHeight - 40) / extent.height,
  );
  renderer.render(
    store.list,
    { zoom, panX: (canvas.clientWidth - extent.width * zoom) / 2, panY: 20 },
    {
      selected,
      hovered: null,
      editing: null,
      caret: null,
      caretVisible: false,
      highlights: [],
      guides: [],
      marquee: null,
    },
  );
}

void run();
