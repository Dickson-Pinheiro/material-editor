/**
 * Wiring: pointer, keyboard, toolbar and the render loop.
 *
 * The loop is deliberately dumb — state changes, the store re-lays-out through
 * the engine, and the renderer paints whatever came back. No incremental
 * invalidation, because a full re-layout of a page-sized document is fast and
 * always correct.
 */

import { Engine } from "./engine";
import {
  Store,
  newFrameId,
  parseLen,
  newImageFrame,
  newShapeFrame,
  newTextFrame,
  normalize,
} from "./store";
import { Renderer, HANDLE_SIZE, documentExtent, handlePositions, placePages } from "./renderer";
import type { HandleName, Overlay, View } from "./renderer";
import { caretAt, caretGeometry, frameAt, framesAt, rangeRects } from "./hit";
import { TextEditor } from "./text";
import { Inspector } from "./inspector";
import type { Alignment } from "./inspector";
import { LayersPanel } from "./layers";
import { iconButton } from "./icons";
import type { DisplayFrame, DocumentSpec, Frame, Page, Rect, Style } from "./types";
// One source of truth: the same file `cargo run --example render` reads.
import STARTER from "../../../examples/material.json";

/** How close, in points, two edges must be to snap together. */
const SNAP_TOLERANCE = 5;

type Gesture =
  | { kind: "none" }
  | { kind: "pan"; startX: number; startY: number; panX: number; panY: number }
  | { kind: "move"; page: number; startX: number; startY: number; origins: Map<string, Rect> }
  | { kind: "resize"; page: number; id: string; handle: HandleName; start: Rect }
  | { kind: "marquee"; page: number; startX: number; startY: number }
  | { kind: "text"; page: number };

async function boot(): Promise<void> {
  const canvas = document.querySelector<HTMLCanvasElement>("#canvas")!;
  const statusBar = document.querySelector<HTMLElement>("#status")!;
  const hintBar = document.querySelector<HTMLElement>("#hint")!;
  const inspectorRoot = document.querySelector<HTMLElement>("#inspector")!;
  const layersRoot = document.querySelector<HTMLElement>("#layers")!;
  const toolsRoot = document.querySelector<HTMLElement>("#tools")!;
  const actionsRoot = document.querySelector<HTMLElement>("#actions")!;
  const titleInput = document.querySelector<HTMLInputElement>("#doc-title")!;

  const engine = new Engine();
  try {
    await engine.start();
  } catch (error) {
    statusBar.textContent = `Falha ao iniciar o motor: ${String(error)}`;
    statusBar.classList.add("error");
    return;
  }

  const store = new Store(engine, normalize(STARTER as unknown as DocumentSpec));
  const renderer = new Renderer(canvas, engine);

  // The engine registered these before the first layout; the canvas needs the
  // pixels as well.
  for (const [key, bitmap] of engine.images()) {
    renderer.setImage(key, bitmap);
  }
  const text = new TextEditor(store);

  const view: View = { zoom: 1, panX: 40, panY: 40 };
  const selected = new Set<string>();
  /** The page commands act on — whatever was last clicked. */
  let activePage = 0;
  let hovered: string | null = null;
  let gesture: Gesture = { kind: "none" };
  let marquee: Rect | null = null;
  let guides: Overlay["guides"] = [];
  let caretVisible = true;
  let spaceHeld = false;
  /** Where the pointer last was, so a drag knows which page it ended over. */
  let lastLocated: { page: number; x: number; y: number } | null = null;

  // A hidden field carries keyboard input, so dead keys and IME composition —
  // "ç", "ã" — behave exactly as they do in a real text field.
  const keyboard = document.createElement("textarea");
  keyboard.className = "hidden-input";
  keyboard.setAttribute("aria-hidden", "true");
  document.body.append(keyboard);

  const inspector = new Inspector(inspectorRoot, store, text, {
    frameChange(id, mutate) {
      store.commit(() => {
        const frame = store.frame(id);
        if (frame) mutate(frame);
      });
    },
    docChange(mutate) {
      store.commit(mutate);
    },
    textStyle(patch: Style) {
      text.applyStyle(patch);
    },
    align: (kind) => alignSelection(kind),
    distribute: (axis) => distributeSelection(axis),
    fontFamilies: () => engine.fontFamilies(),
  });

  const layers = new LayersPanel(layersRoot, store, {
    select(id, additive) {
      if (!additive) selected.clear();
      if (additive && selected.has(id)) selected.delete(id);
      else selected.add(id);
      const located = store.locate(id);
      if (located) activePage = located.page;
      refresh();
    },
    focusPage(index) {
      activePage = index;
      zoomToPage(index);
      refresh();
    },
    changed: () => refresh(),
  });

  // ── Coordinates ─────────────────────────────────────────────────────────────

  function toWorld(event: { clientX: number; clientY: number }): [number, number] {
    const box = canvas.getBoundingClientRect();
    return [
      (event.clientX - box.left - view.panX) / view.zoom,
      (event.clientY - box.top - view.panY) / view.zoom,
    ];
  }

  /** World point → page index plus page-local coordinates. */
  function toPage(worldX: number, worldY: number): { page: number; x: number; y: number } | null {
    const placements = placePages(store.list);
    let nearest: { page: number; x: number; y: number } | null = null;
    let nearestDistance = Infinity;

    for (const placement of placements) {
      const x = worldX - placement.x;
      const y = worldY - placement.y;
      const inside =
        x >= 0 && y >= 0 && x <= placement.page.width && y <= placement.page.height;
      if (inside) return { page: placement.page.index, x, y };

      const dx = Math.max(0, Math.max(-x, x - placement.page.width));
      const dy = Math.max(0, Math.max(-y, y - placement.page.height));
      const distance = Math.hypot(dx, dy);
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearest = { page: placement.page.index, x, y };
      }
    }
    return nearest;
  }

  function pageOf(index: number) {
    return store.list.pages.find((page) => page.index === index) ?? null;
  }

  // ── Rendering ───────────────────────────────────────────────────────────────

  function draw(): void {
    renderer.resize();

    const caret = buildCaret();
    const highlights = buildHighlights();

    const overlay: Overlay = {
      selected,
      hovered,
      editing: text.frameId,
      caret,
      caretVisible: caretVisible && text.active() && !text.hasSelection(),
      highlights,
      guides,
      marquee,
    };

    renderer.render(store.list, view, overlay);
    updateStatus();
  }

  function buildCaret(): Overlay["caret"] {
    if (!text.caret || !text.frameId) return null;
    for (const page of store.list.pages) {
      const geometry = caretGeometry(store.list, page, text.caret);
      if (geometry) return { page: page.index, ...geometry };
    }
    return null;
  }

  function buildHighlights(): Overlay["highlights"] {
    const range = text.range();
    if (!range) return [];
    const out: Overlay["highlights"] = [];
    for (const page of store.list.pages) {
      for (const rect of rangeRects(store.list, page, range[0], range[1])) {
        out.push({ page: page.index, rect });
      }
    }
    return out;
  }

  /** The last refusal already reported, so panning does not replay it. */
  let shownError: string | null = null;

  function updateStatus(): void {
    // A refused change is rolled back; without this it would look like the
    // field simply ignored what was typed.
    if (store.lastError && store.lastError !== shownError) {
      hint(`Não aplicado: ${store.lastError}`);
    }
    shownError = store.lastError;

    const errors = store.list.diagnostics.filter((d) => d.severity === "error").length;
    const warnings = store.list.diagnostics.length - errors;
    const parts = [
      `página ${Math.min(activePage + 1, store.list.pages.length)}/${store.list.pages.length}`,
      `${store.list.pages.reduce((n, p) => n + p.frames.length, 0)} frames`,
      `zoom ${Math.round(view.zoom * 100)}%`,
    ];
    if (warnings > 0) parts.push(`${warnings} aviso(s)`);
    if (errors > 0) parts.push(`${errors} erro(s)`);
    statusBar.textContent = parts.join(" · ");
    statusBar.classList.toggle("error", errors > 0);

    const zoomLabel = document.querySelector("#zoom-label");
    if (zoomLabel) zoomLabel.textContent = `${Math.round(view.zoom * 100)}%`;
  }

  function refresh(): void {
    inspector.render({ selected: [...selected], list: store.list, editing: text.frameId });
    layers.render({ selected, activePage, list: store.list });
    if (document.activeElement !== titleInput) {
      titleInput.value = store.doc.meta?.title ?? "Sem título";
    }
    draw();
  }

  store.subscribe(refresh);

  // ── Hit testing ─────────────────────────────────────────────────────────────

  function displayFrame(id: string): DisplayFrame | null {
    for (const page of store.list.pages) {
      const found = page.frames.find((frame) => frame.id === id);
      if (found) return found;
    }
    return null;
  }

  function handleAt(_pageIndex: number, x: number, y: number): { id: string; handle: HandleName } | null {
    if (selected.size !== 1) return null;
    const id = [...selected][0]!;
    const frame = displayFrame(id);
    if (!frame || frame.locked) return null;

    const reach = HANDLE_SIZE / view.zoom;
    for (const [handle, [hx, hy]] of Object.entries(handlePositions(frame.rect))) {
      if (Math.abs(x - hx) <= reach && Math.abs(y - hy) <= reach) {
        return { id, handle: handle as HandleName };
      }
    }
    return null;
  }

  // ── Snapping ────────────────────────────────────────────────────────────────

  /** Candidate edges to snap to: the margin box and every other frame. */
  function snapTargets(pageIndex: number, moving: Set<string>): { xs: number[]; ys: number[] } {
    const page = pageOf(pageIndex);
    if (!page) return { xs: [], ys: [] };

    const xs = [page.marginBox.x, page.marginBox.x + page.marginBox.w, page.width / 2];
    const ys = [page.marginBox.y, page.marginBox.y + page.marginBox.h, page.height / 2];

    for (const frame of page.frames) {
      if (moving.has(frame.id) || frame.ancestors.length > 0) continue;
      xs.push(frame.rect.x, frame.rect.x + frame.rect.w, frame.rect.x + frame.rect.w / 2);
      ys.push(frame.rect.y, frame.rect.y + frame.rect.h, frame.rect.y + frame.rect.h / 2);
    }
    return { xs, ys };
  }

  function snap(value: number, candidates: number[]): { value: number; guide: number | null } {
    const tolerance = SNAP_TOLERANCE / view.zoom;
    let best = value;
    let guide: number | null = null;
    let bestDistance = tolerance;

    for (const candidate of candidates) {
      const distance = Math.abs(value - candidate);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = candidate;
        guide = candidate;
      }
    }
    return { value: best, guide };
  }

  // ── Pointer ─────────────────────────────────────────────────────────────────

  canvas.addEventListener("pointerdown", (event) => {
    canvas.setPointerCapture(event.pointerId);
    // Paste is delivered to the focused element; keeping the hidden field
    // focused means Ctrl+V lands whether or not text is being edited.
    keyboard.focus({ preventScroll: true });
    const [worldX, worldY] = toWorld(event);
    const located = toPage(worldX, worldY);
    if (!located) return;

    // Middle button, or space held, pans the view — the Figma convention.
    if (event.button === 1 || (event.button === 0 && spaceHeld)) {
      gesture = { kind: "pan", startX: event.clientX, startY: event.clientY, panX: view.panX, panY: view.panY };
      return;
    }

    const page = pageOf(located.page);
    if (!page) return;
    activePage = located.page;

    // Text editing takes priority inside the frame being edited.
    if (text.frameId) {
      const editingFrame = displayFrame(text.frameId);
      if (editingFrame && pointInFrame(editingFrame, located.x, located.y)) {
        const caret = caretAt(store.list, page, located.x, located.y, text.frameId);
        if (caret) {
          text.place(caret, event.shiftKey);
          text.rememberColumn(located.x);
          gesture = { kind: "text", page: located.page };
          keyboard.focus({ preventScroll: true });
          refresh();
          return;
        }
      }
      text.exit();
    }

    const grabbed = handleAt(located.page, located.x, located.y);
    if (grabbed) {
      const frame = displayFrame(grabbed.id)!;
      store.beginGesture();
      gesture = { kind: "resize", page: located.page, id: grabbed.id, handle: grabbed.handle, start: { ...frame.rect } };
      return;
    }

    // Ctrl-click drills past a group to the child actually under the cursor;
    // Alt is reserved for drag-to-duplicate, the way Figma assigns them.
    const hit =
      event.ctrlKey || event.metaKey
        ? (framesAt(page, located.x, located.y).sort(
            (a, b) => b.ancestors.length - a.ancestors.length,
          )[0] ?? null)
        : frameAt(page, located.x, located.y);
    if (hit) {
      if (event.shiftKey) {
        if (selected.has(hit.id)) selected.delete(hit.id);
        else selected.add(hit.id);
      } else if (!selected.has(hit.id)) {
        selected.clear();
        selected.add(hit.id);
      }

      // Alt turns the drag into a duplicate: copy first, then drag the copy.
      if (event.altKey) {
        const copies = store.insertFrames(store.cloneFrames([...selected]), located.page);
        if (copies.length > 0) {
          selected.clear();
          for (const id of copies) selected.add(id);
        }
      }

      const origins = new Map<string, Rect>();
      for (const id of selected) {
        const frame = displayFrame(id);
        if (frame && !frame.locked) origins.set(id, { ...frame.rect });
      }
      store.beginGesture();
      gesture = { kind: "move", page: located.page, startX: located.x, startY: located.y, origins };
      refresh();
      return;
    }

    if (!event.shiftKey) selected.clear();
    gesture = { kind: "marquee", page: located.page, startX: located.x, startY: located.y };
    refresh();
  });

  canvas.addEventListener("pointermove", (event) => {
    const [worldX, worldY] = toWorld(event);
    const located = toPage(worldX, worldY);

    if (gesture.kind === "pan") {
      view.panX = gesture.panX + (event.clientX - gesture.startX);
      view.panY = gesture.panY + (event.clientY - gesture.startY);
      draw();
      return;
    }

    if (!located) return;
    lastLocated = located;
    const page = pageOf(located.page);

    switch (gesture.kind) {
      case "none": {
        const next = page ? (frameAt(page, located.x, located.y)?.id ?? null) : null;
        if (next !== hovered) {
          hovered = next;
          draw();
        }
        canvas.style.cursor = cursorFor(located);
        break;
      }

      case "move": {
        const dx = located.x - gesture.startX;
        const dy = located.y - gesture.startY;
        applyMove(gesture, dx, dy, event.shiftKey);
        break;
      }

      case "resize": {
        applyResize(gesture, located.x, located.y, event.altKey);
        break;
      }

      case "marquee": {
        // Bound to a const so the narrowing survives into the callback below.
        const current = gesture;
        const box = normalizeRect(current.startX, current.startY, located.x, located.y);
        const placement = placePages(store.list).find((p) => p.page.index === current.page);
        marquee = placement
          ? { ...box, x: box.x + placement.x, y: box.y + placement.y }
          : box;
        draw();
        break;
      }

      case "text": {
        const caret = page ? caretAt(store.list, page, located.x, located.y, text.frameId!) : null;
        if (caret) {
          text.place(caret, true);
          draw();
        }
        break;
      }
    }
  });

  canvas.addEventListener("pointerup", () => {
    if (gesture.kind === "marquee" && marquee) {
      const current = gesture;
      const box = marquee;
      const page = pageOf(current.page);
      const placement = placePages(store.list).find((p) => p.page.index === current.page);
      if (page && placement) {
        const local: Rect = { ...box, x: box.x - placement.x, y: box.y - placement.y };
        for (const frame of page.frames) {
          if (frame.ancestors.length > 0 || frame.locked) continue;
          if (intersects(frame.rect, local)) selected.add(frame.id);
        }
      }
    }

    // A frame dropped over another page belongs to that page now. Bound to
    // consts so the narrowing survives into the callbacks below.
    const finished = gesture;
    const dropped = lastLocated;

    if (finished.kind === "move" && dropped && dropped.page !== finished.page) {
      const placements = placePages(store.list);
      const from = placements.find((p) => p.page.index === finished.page);
      const to = placements.find((p) => p.page.index === dropped.page);

      store.endGesture();
      if (from && to) {
        store.moveToPage([...selected], dropped.page, from.x - to.x, from.y - to.y);
        activePage = dropped.page;
      }
    } else if (finished.kind === "move" || finished.kind === "resize") {
      store.endGesture();
    }

    gesture = { kind: "none" };
    marquee = null;
    guides = [];
    refresh();
  });

  function pointInFrame(frame: DisplayFrame, x: number, y: number): boolean {
    const r = frame.rect;
    return x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h;
  }

  function cursorFor(located: { page: number; x: number; y: number }): string {
    if (handleAt(located.page, located.x, located.y)) return "nwse-resize";
    const page = pageOf(located.page);
    if (page && frameAt(page, located.x, located.y)) return "move";
    return "default";
  }

  function applyMove(
    current: Extract<Gesture, { kind: "move" }>,
    dx: number,
    dy: number,
    lockAxis: boolean,
  ): void {
    if (lockAxis) {
      if (Math.abs(dx) > Math.abs(dy)) dy = 0;
      else dx = 0;
    }

    const targets = snapTargets(current.page, new Set(current.origins.keys()));
    guides = [];

    // Snap using the first frame's edges; the rest follow rigidly.
    const first = current.origins.values().next().value as Rect | undefined;
    if (first) {
      const left = snap(first.x + dx, targets.xs);
      const right = snap(first.x + first.w + dx, targets.xs);
      const top = snap(first.y + dy, targets.ys);
      const bottom = snap(first.y + first.h + dy, targets.ys);

      if (left.guide !== null) {
        dx = left.value - first.x;
        guides.push({ page: current.page, x: left.guide });
      } else if (right.guide !== null) {
        dx = right.value - (first.x + first.w);
        guides.push({ page: current.page, x: right.guide });
      }
      if (top.guide !== null) {
        dy = top.value - first.y;
        guides.push({ page: current.page, y: top.guide });
      } else if (bottom.guide !== null) {
        dy = bottom.value - (first.y + first.h);
        guides.push({ page: current.page, y: bottom.guide });
      }
    }

    store.update(() => {
      for (const [id, origin] of current.origins) {
        const frame = store.frame(id);
        if (!frame) continue;
        // `origin` is in page coordinates but a frame's rect is relative to its
        // parent group, so the parent's own origin has to come back out.
        const parent = originOf(id);
        frame.rect[0] = round(origin.x + dx - parent.x);
        frame.rect[1] = round(origin.y + dy - parent.y);
      }
    });
  }

  function applyResize(
    current: Extract<Gesture, { kind: "resize" }>,
    x: number,
    y: number,
    fromCentre: boolean,
  ): void {
    const start = current.start;
    let { x: left, y: top, w, h } = start;
    const right = left + w;
    const bottom = top + h;

    const targets = snapTargets(current.page, new Set([current.id]));
    guides = [];

    const snappedX = snap(x, targets.xs);
    const snappedY = snap(y, targets.ys);
    if (snappedX.guide !== null) guides.push({ page: current.page, x: snappedX.guide });
    if (snappedY.guide !== null) guides.push({ page: current.page, y: snappedY.guide });

    const px = snappedX.value;
    const py = snappedY.value;

    if (current.handle.includes("w")) {
      left = Math.min(px, right - 8);
      w = right - left;
    }
    if (current.handle.includes("e")) {
      w = Math.max(8, px - left);
    }
    if (current.handle.includes("n")) {
      top = Math.min(py, bottom - 8);
      h = bottom - top;
    }
    if (current.handle.includes("s")) {
      h = Math.max(8, py - top);
    }

    if (fromCentre) {
      const cx = start.x + start.w / 2;
      const cy = start.y + start.h / 2;
      left = cx - w / 2;
      top = cy - h / 2;
    }

    store.update(() => {
      const frame = store.frame(current.id);
      if (!frame) return;
      const parent = originOf(current.id);
      frame.rect[0] = round(left - parent.x);
      frame.rect[1] = round(top - parent.y);
      frame.rect[2] = round(w);
      frame.rect[3] = round(h);
    });
  }

  // ── Double click: enter text ────────────────────────────────────────────────

  canvas.addEventListener("dblclick", (event) => {
    const [worldX, worldY] = toWorld(event);
    const located = toPage(worldX, worldY);
    if (!located) return;
    const page = pageOf(located.page);
    if (!page) return;

    const hit = frameAt(page, located.x, located.y);
    if (!hit) return;

    // Double-clicking a group steps inside it and takes the child under the
    // cursor — the only way into a group without the layers panel.
    if (hit.kind === "group") {
      const inside = framesAt(page, located.x, located.y)
        .filter((frame) => frame.ancestors.includes(hit.id))
        .sort((a, b) => b.ancestors.length - a.ancestors.length)[0];
      if (inside) {
        selected.clear();
        selected.add(inside.id);
        refresh();
      }
      return;
    }

    if (hit.kind !== "text") return;

    const caret = caretAt(store.list, page, located.x, located.y, hit.id) ?? {
      frame: hit.id,
      story: null,
      block: 0,
      inline: 0,
      offset: 0,
    };

    selected.clear();
    selected.add(hit.id);
    text.enter(hit.id, caret);
    text.rememberColumn(located.x);
    keyboard.focus({ preventScroll: true });
    refresh();
  });

  // ── Zoom and pan ────────────────────────────────────────────────────────────

  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const box = canvas.getBoundingClientRect();
        const before = toWorld(event);
        view.zoom = clamp(view.zoom * Math.exp(-event.deltaY / 400), 0.1, 8);
        // Keep the point under the cursor fixed.
        view.panX = event.clientX - box.left - before[0] * view.zoom;
        view.panY = event.clientY - box.top - before[1] * view.zoom;
      } else {
        view.panX -= event.deltaX;
        view.panY -= event.deltaY;
      }
      draw();
    },
    { passive: false },
  );

  function zoomTo(factor: number): void {
    view.zoom = clamp(factor, 0.1, 8);
    draw();
  }

  /** Fit the whole document — every page at once. */
  function zoomFit(): void {
    const extent = documentExtent(store.list);
    if (extent.width === 0 || extent.height === 0) return;
    const margin = 48;
    const zoom = Math.min(
      (canvas.clientWidth - margin) / extent.width,
      (canvas.clientHeight - margin) / extent.height,
    );
    view.zoom = clamp(zoom, 0.1, 8);
    view.panX = (canvas.clientWidth - extent.width * view.zoom) / 2;
    view.panY = 20;
    draw();
  }

  /**
   * Fit one page.
   *
   * This is what opening a document should do: fitting all ten pages of a
   * booklet gives a 10% zoom that nobody can read or click.
   */
  function zoomToPage(index: number): void {
    const placement = placePages(store.list).find((p) => p.page.index === index);
    if (!placement) return;

    const margin = 48;
    const zoom = clamp(
      Math.min(
        (canvas.clientWidth - margin) / placement.page.width,
        (canvas.clientHeight - margin) / placement.page.height,
      ),
      0.1,
      8,
    );

    view.zoom = zoom;
    view.panX = (canvas.clientWidth - placement.page.width * zoom) / 2 - placement.x * zoom;
    view.panY = margin / 2 - placement.y * zoom;
    draw();
  }

  // ── Keyboard ────────────────────────────────────────────────────────────────

  let composing = false;

  keyboard.addEventListener("compositionstart", () => {
    composing = true;
  });
  keyboard.addEventListener("compositionend", () => {
    composing = false;
    flushInput();
  });
  keyboard.addEventListener("input", () => {
    if (!composing) flushInput();
  });
  keyboard.addEventListener("paste", (event) => {
    if (!text.active()) return; // Not editing: the document handler takes it.

    const pasted = event.clipboardData?.getData("text/plain");
    if (!pasted) return;
    event.preventDefault();

    // Frames copied earlier are not text; dropping their JSON into a paragraph
    // would be nonsense, so it is ignored rather than pasted literally.
    if (pasted.includes(CLIPBOARD_KIND)) return;
    text.insert(pasted);
  });

  function flushInput(): void {
    const value = keyboard.value;
    keyboard.value = "";
    if (value.length > 0 && text.active()) text.insert(value);
  }

  window.addEventListener("keydown", (event) => {
    const meta = event.ctrlKey || event.metaKey;

    if (meta && event.key.toLowerCase() === "z") {
      event.preventDefault();
      if (event.shiftKey) store.redo();
      else store.undo();
      return;
    }
    if (meta && event.key.toLowerCase() === "e") {
      event.preventDefault();
      void exportPdf();
      return;
    }
    // Copy and cut work in both modes, so they come before the split.
    if (meta && (event.key.toLowerCase() === "c" || event.key.toLowerCase() === "x")) {
      const cutting = event.key.toLowerCase() === "x";

      if (text.active()) {
        const selection = cutting ? text.cut() : text.selectedText();
        if (selection) {
          event.preventDefault();
          writeClipboard(selection);
        }
        return;
      }

      if (selected.size > 0) {
        event.preventDefault();
        copySelection();
        if (cutting) deleteSelection();
      }
      return;
    }

    if (event.shiftKey && event.key === "!") {
      event.preventDefault();
      zoomFit();
      return;
    }

    // Everything below acts on frames, so it stops at the text boundary.
    if (text.active()) {
      handleTextKey(event, meta);
      return;
    }

    if (meta && event.key.toLowerCase() === "d") {
      event.preventDefault();
      duplicateSelection();
      return;
    }
    if (meta && event.key.toLowerCase() === "g") {
      event.preventDefault();
      if (event.shiftKey) ungroupSelection();
      else groupSelection();
      return;
    }
    if (meta && (event.key === "]" || event.key === "[")) {
      event.preventDefault();
      nudge(event.key === "]" ? 1 : -1);
      return;
    }

    switch (event.key) {
      case "Delete":
      case "Backspace":
        event.preventDefault();
        deleteSelection();
        break;
      case "Escape":
        selected.clear();
        refresh();
        break;
      case "ArrowLeft":
      case "ArrowRight":
      case "ArrowUp":
      case "ArrowDown": {
        if (selected.size === 0) return;
        event.preventDefault();
        const step = event.shiftKey ? 10 : 1;
        const dx = event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0;
        const dy = event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0;
        store.commit(() => {
          for (const id of selected) {
            const frame = store.frame(id);
            if (!frame) continue;
            // These are already parent-relative, so a plain offset is right.
            frame.rect[0] = round(parseLen(frame.rect[0]) + dx);
            frame.rect[1] = round(parseLen(frame.rect[1]) + dy);
          }
        });
        break;
      }
      case "t":
        addFrame("text");
        break;
      case "r":
        addFrame("shape");
        break;
      case " ":
        if (!spaceHeld) {
          spaceHeld = true;
          canvas.style.cursor = "grab";
        }
        event.preventDefault();
        break;
    }
  });

  // The document-level clipboard events are the reliable path: they carry
  // `clipboardData` without needing permissions, unlike the async API.
  document.addEventListener("copy", (event) => {
    if (text.active()) {
      const selection = text.selectedText();
      if (selection) {
        event.clipboardData?.setData("text/plain", selection);
        event.preventDefault();
      }
      return;
    }
    if (copySelection(event)) event.preventDefault();
  });

  document.addEventListener("cut", (event) => {
    if (text.active()) {
      const selection = text.cut();
      if (selection) {
        event.clipboardData?.setData("text/plain", selection);
        event.preventDefault();
      }
      return;
    }
    if (copySelection(event)) {
      event.preventDefault();
      deleteSelection();
    }
  });

  document.addEventListener("paste", (event) => {
    // While editing, the hidden field handles it as text.
    if (text.active()) return;
    if (pasteFrames(event.clipboardData?.getData("text/plain") ?? null)) {
      event.preventDefault();
    }
  });

  window.addEventListener("keyup", (event) => {
    if (event.key === " ") {
      spaceHeld = false;
      canvas.style.cursor = "default";
    }
  });

  function handleTextKey(event: KeyboardEvent, meta: boolean): void {
    if (meta && event.key.toLowerCase() === "a") {
      event.preventDefault();
      text.selectAll();
      refresh();
      return;
    }
    if (meta && event.key.toLowerCase() === "b") {
      event.preventDefault();
      text.applyStyle({ fontWeight: "bold" });
      return;
    }
    if (meta && event.key.toLowerCase() === "i") {
      event.preventDefault();
      text.applyStyle({ fontStyle: "italic" });
      return;
    }

    if (meta && event.key === "Enter") {
      event.preventDefault();
      text.insertPageBreak();
      return;
    }

    switch (event.key) {
      case "Escape":
        event.preventDefault();
        text.exit();
        canvas.focus();
        refresh();
        break;
      case "Backspace":
        event.preventDefault();
        text.deleteBackward();
        break;
      case "Delete":
        event.preventDefault();
        text.deleteForward();
        break;
      case "Enter":
        event.preventDefault();
        text.splitParagraph();
        break;
      case "ArrowLeft":
        event.preventDefault();
        text.moveHorizontal(-1, event.shiftKey);
        syncColumn();
        refresh();
        break;
      case "ArrowRight":
        event.preventDefault();
        text.moveHorizontal(1, event.shiftKey);
        syncColumn();
        refresh();
        break;
      case "ArrowUp":
      case "ArrowDown": {
        event.preventDefault();
        const page = store.list.pages.find((candidate) =>
          candidate.frames.some((frame) => frame.id === text.frameId),
        );
        if (page) {
          text.moveVertical(event.key === "ArrowUp" ? -1 : 1, event.shiftKey, store.list, page);
          refresh();
        }
        break;
      }
      case "Home":
        event.preventDefault();
        text.moveToLineEdge(-1, event.shiftKey);
        refresh();
        break;
      case "End":
        event.preventDefault();
        text.moveToLineEdge(1, event.shiftKey);
        refresh();
        break;
    }
  }

  function syncColumn(): void {
    const caret = buildCaret();
    if (caret) text.rememberColumn(caret.x);
  }

  // ── Commands ────────────────────────────────────────────────────────────────

  function deleteSelection(): void {
    if (selected.size === 0) return;
    store.commit(() => {
      for (const id of selected) {
        const located = store.locate(id);
        if (located) located.siblings.splice(located.index, 1);
      }
    });
    selected.clear();
    refresh();
  }

  function addFrame(kind: "text" | "shape" | "image", src?: string): void {
    const pageIndex = clamp(activePage, 0, Math.max(0, store.doc.pages.length - 1));
    const page = pageOf(pageIndex);
    const x = page ? page.marginBox.x + 20 : 40;
    const y = page ? page.marginBox.y + 20 : 40;

    const frame: Frame =
      kind === "text"
        ? newTextFrame(x, y)
        : kind === "shape"
          ? newShapeFrame(x, y)
          : newImageFrame(x, y, src ?? "");

    store.commit((doc) => {
      doc.pages[pageIndex]?.frames.push(frame);
    });
    selected.clear();
    selected.add(frame.id!);
    refresh();
  }

  function reorder(direction: 1 | -1): void {
    if (selected.size !== 1) return;
    const id = [...selected][0]!;
    store.commit(() => {
      const located = store.locate(id);
      if (!located) return;
      const [frame] = located.siblings.splice(located.index, 1);
      if (!frame) return;
      if (direction > 0) located.siblings.push(frame);
      else located.siblings.unshift(frame);
    });
  }

  // ── Align, distribute, group ────────────────────────────────────────────────

  /** Boxes of the current selection, in page coordinates. */
  function selectionBoxes(): { id: string; rect: Rect }[] {
    return [...selected]
      .map((id) => ({ id, rect: displayFrame(id)?.rect }))
      .filter((entry): entry is { id: string; rect: Rect } => entry.rect !== undefined);
  }

  /**
   * Align the selection.
   *
   * One object aligns to the page's margin box — which is what you almost
   * always mean by "align this to the left" on a single frame. Two or more
   * align to their common bounding box, as in every design tool.
   */
  function alignSelection(kind: Alignment): void {
    const boxes = selectionBoxes();
    if (boxes.length === 0) return;

    const page = pageOf(activePage);
    const bounds =
      boxes.length === 1 && page
        ? page.marginBox
        : {
            x: Math.min(...boxes.map((b) => b.rect.x)),
            y: Math.min(...boxes.map((b) => b.rect.y)),
            w:
              Math.max(...boxes.map((b) => b.rect.x + b.rect.w)) -
              Math.min(...boxes.map((b) => b.rect.x)),
            h:
              Math.max(...boxes.map((b) => b.rect.y + b.rect.h)) -
              Math.min(...boxes.map((b) => b.rect.y)),
          };

    store.commit(() => {
      for (const { id, rect } of boxes) {
        const frame = store.frame(id);
        if (!frame) continue;
        const origin = originOf(id);

        switch (kind) {
          case "left":
            frame.rect[0] = round(bounds.x - origin.x);
            break;
          case "centerX":
            frame.rect[0] = round(bounds.x + (bounds.w - rect.w) / 2 - origin.x);
            break;
          case "right":
            frame.rect[0] = round(bounds.x + bounds.w - rect.w - origin.x);
            break;
          case "top":
            frame.rect[1] = round(bounds.y - origin.y);
            break;
          case "centerY":
            frame.rect[1] = round(bounds.y + (bounds.h - rect.h) / 2 - origin.y);
            break;
          case "bottom":
            frame.rect[1] = round(bounds.y + bounds.h - rect.h - origin.y);
            break;
        }
      }
    });
  }

  /** Even gaps between three or more objects along one axis. */
  function distributeSelection(axis: "x" | "y"): void {
    const boxes = selectionBoxes();
    if (boxes.length < 3) return;

    const size = (rect: Rect) => (axis === "x" ? rect.w : rect.h);
    const start = (rect: Rect) => (axis === "x" ? rect.x : rect.y);

    const ordered = [...boxes].sort((a, b) => start(a.rect) - start(b.rect));
    const first = ordered[0]!;
    const last = ordered[ordered.length - 1]!;

    const span = start(last.rect) + size(last.rect) - start(first.rect);
    const occupied = ordered.reduce((total, entry) => total + size(entry.rect), 0);
    const gap = (span - occupied) / (ordered.length - 1);

    store.commit(() => {
      let cursor = start(first.rect);
      for (const entry of ordered) {
        const frame = store.frame(entry.id);
        if (frame) {
          const origin = originOf(entry.id);
          const value = round(cursor - (axis === "x" ? origin.x : origin.y));
          frame.rect[axis === "x" ? 0 : 1] = value;
        }
        cursor += size(entry.rect) + gap;
      }
    });
  }

  /** Origin of a frame's parent, so page coordinates can be written back. */
  function originOf(id: string): { x: number; y: number } {
    const frame = displayFrame(id);
    const located = store.locate(id);
    if (!frame || !located) return { x: 0, y: 0 };
    return {
      x: frame.rect.x - parseLen(located.frame.rect[0]),
      y: frame.rect.y - parseLen(located.frame.rect[1]),
    };
  }

  function groupSelection(): void {
    const id = store.group([...selected]);
    if (!id) {
      hint("Selecione dois ou mais objetos irmãos para agrupar.");
      return;
    }
    selected.clear();
    selected.add(id);
    refresh();
  }

  function ungroupSelection(): void {
    const ids = [...selected].flatMap((id) => store.ungroup(id));
    if (ids.length === 0) {
      hint("Selecione um grupo para desagrupar.");
      return;
    }
    selected.clear();
    for (const id of ids) selected.add(id);
    refresh();
  }

  let hintTimer = 0;
  function hint(message: string): void {
    hintBar.textContent = message;
    window.clearTimeout(hintTimer);
    hintTimer = window.setTimeout(() => (hintBar.textContent = ""), 3200);
  }

  // ── Clipboard ───────────────────────────────────────────────────────────────

  /** What the system clipboard carries between documents and tabs. */
  const CLIPBOARD_KIND = "diagramador/frames@1";

  /** Fallback for when the system clipboard is unavailable or empty. */
  let localClipboard: Frame[] = [];
  /** How far each successive paste of the same thing is nudged. */
  let pasteRun = 0;

  /**
   * Put text on the system clipboard.
   *
   * The `copy` event is not a usable trigger here: browsers only fire it when
   * the focused element has a selection, and ours is an empty off-screen
   * field. So the key handler writes directly — the async API first, falling
   * back to selecting the hidden field and letting the old command copy it.
   */
  function writeClipboard(payload: string): void {
    const legacy = () => {
      const previous = keyboard.value;
      keyboard.value = payload;
      keyboard.select();
      document.execCommand("copy");
      keyboard.value = previous;
    };

    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(payload).catch(legacy);
    } else {
      legacy();
    }
  }

  function copySelection(event?: ClipboardEvent): boolean {
    if (selected.size === 0) return false;

    const frames = store.cloneFrames([...selected]);
    if (frames.length === 0) return false;

    localClipboard = frames;
    pasteRun = 0;

    const payload = JSON.stringify({ kind: CLIPBOARD_KIND, frames });
    if (event?.clipboardData) event.clipboardData.setData("text/plain", payload);
    else writeClipboard(payload);

    return true;
  }

  function pasteFrames(payload: string | null): boolean {
    let frames = localClipboard;

    if (payload) {
      try {
        const parsed = JSON.parse(payload) as { kind?: string; frames?: Frame[] };
        if (parsed.kind === CLIPBOARD_KIND && Array.isArray(parsed.frames)) {
          frames = parsed.frames;
        }
      } catch {
        // Not ours — fall through to whatever was copied inside the editor.
      }
    }

    if (frames.length === 0) return false;

    // Successive pastes cascade, so copies do not stack invisibly.
    pasteRun += 1;
    const step = 12 * pasteRun;
    const ids = store.insertFrames(frames, activePage, step, step);

    selected.clear();
    for (const id of ids) selected.add(id);
    refresh();
    return true;
  }

  /** Ctrl+D: a copy nudged just enough to see, as in every design tool. */
  function duplicateSelection(): void {
    if (selected.size === 0) return;
    const frames = store.cloneFrames([...selected]);
    const ids = store.insertFrames(frames, activePage, 12, 12);
    selected.clear();
    for (const id of ids) selected.add(id);
    refresh();
  }

  // ── Pages ──────────────────────────────────────────────────────────────────

  /** Insert a page after the active one, optionally copying its contents. */
  function addPage(duplicate: boolean): void {
    const index = clamp(activePage, 0, Math.max(0, store.doc.pages.length - 1));

    store.commit((doc) => {
      const template = doc.pages[index];
      const page: Page =
        duplicate && template
          ? {
              ...structuredClone(template),
              id: undefined,
              // Fresh ids: two frames may not answer to the same name.
              frames: structuredClone(template.frames).map(reidentify),
            }
          : { master: template?.master, frames: [] };
      doc.pages.splice(index + 1, 0, page);
    });

    activePage = index + 1;
    selected.clear();
    refresh();
  }

  /** Give a frame and its descendants new ids. */
  function reidentify(frame: Frame): Frame {
    frame.id = newFrameId(frame.type);
    if (frame.type === "group") frame.children.forEach(reidentify);
    return frame;
  }

  function deletePage(): void {
    if (store.doc.pages.length <= 1) {
      statusBar.textContent = "Um documento precisa de ao menos uma página.";
      return;
    }
    const index = clamp(activePage, 0, store.doc.pages.length - 1);
    store.commit((doc) => void doc.pages.splice(index, 1));
    activePage = Math.max(0, index - 1);
    selected.clear();
    refresh();
  }

  async function importImage(): Promise<void> {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/png,image/jpeg";
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      if (!file) return;
      const bytes = new Uint8Array(await file.arrayBuffer());
      const key = file.name;

      engine.addImage(key, bytes);
      const bitmap = await createImageBitmap(new Blob([bytes], { type: file.type }));
      renderer.setImage(key, bitmap);
      addFrame("image", key);
    });
    input.click();
  }

  function exportPdf(): void {
    try {
      const bytes = engine.renderPdf(store.doc);
      // Copy into a plain ArrayBuffer: the wasm view is backed by memory that
      // can move under us the next time the module allocates.
      const buffer = bytes.slice().buffer as ArrayBuffer;
      download(new Blob([buffer], { type: "application/pdf" }), fileName("pdf"));
    } catch (error) {
      statusBar.textContent = `Falha ao gerar o PDF: ${String(error)}`;
      statusBar.classList.add("error");
    }
  }

  function exportJson(): void {
    const json = JSON.stringify(store.doc, null, 2);
    download(new Blob([json], { type: "application/json" }), fileName("json"));
  }

  function fileName(extension: string): string {
    const title = store.doc.meta?.title ?? "documento";
    return `${title.replace(/[^\p{L}\p{N}]+/gu, "-").toLowerCase()}.${extension}`;
  }

  function download(blob: Blob, name: string): void {
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = name;
    anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  // ── Toolbar ─────────────────────────────────────────────────────────────────

  toolsRoot.append(
    iconButton("text", "Caixa de texto — T", () => addFrame("text")),
    iconButton("shape", "Retângulo — R", () => addFrame("shape")),
    iconButton("image", "Imagem — I", () => void importImage()),
    iconButton("pageAdd", "Nova página", () => addPage(false)),
    iconButton("duplicate", "Duplicar página", () => addPage(true)),
    iconButton("trash", "Excluir página", () => deletePage()),
    iconButton("group", "Agrupar — Ctrl+G", () => groupSelection()),
    iconButton("ungroup", "Desagrupar — Ctrl+Shift+G", () => ungroupSelection()),
    iconButton("up", "Avançar um nível — Ctrl+]", () => nudge(1)),
    iconButton("down", "Recuar um nível — Ctrl+[", () => nudge(-1)),
    iconButton("front", "Trazer para a frente", () => reorder(1)),
    iconButton("back", "Enviar para trás", () => reorder(-1)),
    iconButton("undo", "Desfazer — Ctrl+Z", () => store.undo()),
    iconButton("redo", "Refazer — Ctrl+Shift+Z", () => store.redo()),
  );

  actionsRoot.append(
    iconButton("code", "Baixar o JSON", () => exportJson()),
    (() => {
      const button = iconButton("download", "Exportar PDF — Ctrl+E", () => exportPdf(), {
        label: "PDF",
      });
      button.classList.add("primary");
      return button;
    })(),
  );

  document.querySelector("#zoom-in")?.addEventListener("click", () => zoomTo(view.zoom * 1.25));
  document.querySelector("#zoom-out")?.addEventListener("click", () => zoomTo(view.zoom / 1.25));
  document.querySelector("#zoom-label")?.addEventListener("click", () => zoomFit());

  titleInput.addEventListener("change", () => {
    store.commit((doc) => void ((doc.meta ??= {}).title = titleInput.value));
  });

  function nudge(direction: 1 | -1): void {
    if (selected.size !== 1) return;
    store.nudgeOrder([...selected][0]!, direction);
  }

  // ── Loop ────────────────────────────────────────────────────────────────────

  window.addEventListener("resize", draw);
  setInterval(() => {
    if (!text.active()) return;
    caretVisible = !caretVisible;
    draw();
  }, 530);

  zoomToPage(0);
  refresh();
}

// ─────────────────────────────────────────────────────────────────────────────
// Small helpers
// ─────────────────────────────────────────────────────────────────────────────

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function round(value: number): number {
  return Math.round(value * 100) / 100;
}

function normalizeRect(x1: number, y1: number, x2: number, y2: number): Rect {
  return {
    x: Math.min(x1, x2),
    y: Math.min(y1, y2),
    w: Math.abs(x2 - x1),
    h: Math.abs(y2 - y1),
  };
}

function intersects(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

boot().catch((error) => {
  console.error(error);
  const status = document.querySelector("#status");
  if (status) status.textContent = `Erro: ${String(error)}`;
});
