/**
 * The layers panel.
 *
 * A document's paint order *is* its frame array, so this panel is a direct view
 * of `doc.pages[].frames` shown bottom-up — the last frame paints on top, so it
 * appears first. Reordering here and reordering on the canvas are the same
 * operation because there is only one place that order lives.
 *
 * Pages and groups fold, the way they do in Figma: a document of fifty boxes is
 * only navigable if you can put away the parts you are not working on. The fold
 * state lives on the panel instance, not in the document — it is a property of
 * the reader, not of the material.
 */

import { icon } from "./icons";
import type { Store } from "./store";
import type { DisplayList, Frame, Page } from "./types";

export interface LayersState {
  selected: Set<string>;
  activePage: number;
  list: DisplayList;
}

export interface LayersHandlers {
  /** `additive` comes from shift-click. */
  select(id: string, additive: boolean): void;
  focusPage(index: number): void;
  changed(): void;
}

/** Where a dragged row would land. */
type Drop =
  | { kind: "sibling"; page: number; parent: string | null; index: number }
  | { kind: "inside"; page: number; parent: string };

export class LayersPanel {
  private dragging: string | null = null;
  private drop: Drop | null = null;

  /**
   * Fold state, kept out of the document and out of the render.
   *
   * The panel is rebuilt from scratch on every change, so a group that sprang
   * open each time you nudged a number would be unusable. The two sets are
   * opposites on purpose: pages start open, groups start closed — a page is a
   * place you work, a group is a thing you made.
   */
  private readonly foldedPages = new Set<string>();
  private readonly openGroups = new Set<string>();
  /** The selection last revealed, so folding a group with it inside sticks. */
  private revealed = "";
  /**
   * Which row is being renamed, and when it was last clicked.
   *
   * Both live on the panel rather than in the DOM because the panel is rebuilt
   * from scratch on every change — and selecting a row *is* a change. A plain
   * `dblclick` listener never fired: the first click replaced the element the
   * second one needed to land on. So the double click is recognised here, by
   * id and by clock, and the open editor is re-created on each render.
   */
  private renaming: string | null = null;
  private lastClick: { id: string; at: number } | null = null;

  constructor(
    private readonly root: HTMLElement,
    private readonly store: Store,
    private readonly handlers: LayersHandlers,
  ) {}

  render(state: LayersState): void {
    const pages = this.store.doc.pages ?? [];
    this.reveal(state);

    // replaceChildren empties the scroll box, which would send the reader back
    // to the top on every keystroke.
    const scroll = this.root.scrollTop;
    this.root.replaceChildren();

    this.root.append(this.heading(pages));

    pages.forEach((page, index) => {
      const key = pageKey(page, index);
      const open = !this.foldedPages.has(key);
      this.root.append(this.pageRow(page, index, open, state));

      if (!open) return;
      const list = document.createElement("div");
      list.className = "layer-list";
      // Bottom-up: the topmost frame is the last one painted.
      this.appendRows(list, page.frames ?? [], index, null, state, 0);
      this.root.append(list);
    });

    if (pages.length === 0) {
      const empty = document.createElement("p");
      empty.className = "note";
      empty.textContent = "Documento sem páginas.";
      this.root.append(empty);
    }

    this.root.scrollTop = scroll;
  }

  // ── Chrome ──────────────────────────────────────────────────────────────────

  private heading(pages: Page[]): HTMLElement {
    const heading = document.createElement("div");
    heading.className = "section-head panel-title";

    const label = document.createElement("span");
    label.textContent = "Páginas e camadas";

    const allFolded =
      pages.length > 0 &&
      pages.every((page, index) => this.foldedPages.has(pageKey(page, index)));

    const fold = document.createElement("button");
    fold.type = "button";
    fold.title = allFolded ? "Expandir tudo" : "Recolher tudo";
    fold.setAttribute("aria-label", fold.title);
    fold.append(icon(allFolded ? "down" : "up", 14));
    fold.addEventListener("click", () => {
      if (allFolded) {
        this.foldedPages.clear();
        for (const page of pages) forEachGroup(page.frames ?? [], (id) => this.openGroups.add(id));
      } else {
        this.openGroups.clear();
        pages.forEach((page, index) => this.foldedPages.add(pageKey(page, index)));
      }
      this.handlers.changed();
    });

    heading.append(label, fold);
    return heading;
  }

  private pageRow(page: Page, index: number, open: boolean, state: LayersState): HTMLElement {
    const row = document.createElement("div");
    row.className = index === state.activePage ? "page-row active" : "page-row";

    const key = pageKey(page, index);
    row.append(
      this.caret(open, `${open ? "Recolher" : "Expandir"} página`, (deep) => {
        if (open) this.foldedPages.add(key);
        else this.foldedPages.delete(key);
        // Alt folds the page and everything in it, so reopening it is quiet.
        if (deep) forEachGroup(page.frames ?? [], (id) => this.openGroups.delete(id));
        this.handlers.changed();
      }),
    );

    const name = document.createElement("button");
    name.type = "button";
    name.className = "page-name";
    name.append(icon("page", 14));
    const text = document.createElement("span");
    text.textContent = page.name ?? `Página ${index + 1}`;
    name.append(text);
    name.addEventListener("click", () => this.handlers.focusPage(index));
    row.append(name);

    const count = document.createElement("span");
    count.className = "page-count";
    count.textContent = String((page.frames ?? []).length);
    row.append(count);

    return row;
  }

  /**
   * The fold arrow. `deep` is the alt-click variant, which Figma uses to fold
   * or unfold a whole subtree at once.
   */
  private caret(open: boolean, title: string, onToggle: (deep: boolean) => void): HTMLElement {
    const button = document.createElement("button");
    button.type = "button";
    button.className = open ? "layer-caret open" : "layer-caret";
    button.title = title;
    button.setAttribute("aria-label", title);
    button.setAttribute("aria-expanded", String(open));
    button.append(icon("chevron", 10));
    button.addEventListener("pointerdown", (event) => event.stopPropagation());
    button.addEventListener("click", (event) => {
      event.stopPropagation();
      onToggle(event.altKey);
    });
    return button;
  }

  /** A spacer that keeps leaf rows aligned with the ones that have a caret. */
  private stub(): HTMLElement {
    const stub = document.createElement("span");
    stub.className = "layer-caret stub";
    return stub;
  }

  // ── Rows ────────────────────────────────────────────────────────────────────

  private appendRows(
    list: HTMLElement,
    frames: Frame[],
    page: number,
    parent: string | null,
    state: LayersState,
    depth: number,
  ): void {
    for (let index = frames.length - 1; index >= 0; index -= 1) {
      const frame = frames[index]!;
      const id = frame.id ?? "";
      list.append(this.row(frame, id, page, parent, index, state, depth));

      if (frame.type === "group" && this.openGroups.has(id)) {
        this.appendRows(list, frame.children ?? [], page, id, state, depth + 1);
      }
    }
  }

  private row(
    frame: Frame,
    id: string,
    page: number,
    parent: string | null,
    index: number,
    state: LayersState,
    depth: number,
  ): HTMLElement {
    const row = document.createElement("div");
    row.className = "layer-row";
    if (state.selected.has(id)) row.classList.add("selected");
    if (frame.visible === false) row.classList.add("hidden");
    row.style.setProperty("--depth", String(depth));
    if (depth > 0) row.classList.add("nested");
    row.draggable = true;
    row.dataset.id = id;

    const overset = state.list.pages
      .flatMap((p) => p.frames)
      .find((f) => f.id === id)?.overset;

    if (frame.type === "group") {
      const open = this.openGroups.has(id);
      row.append(
        this.caret(open, `${open ? "Recolher" : "Expandir"} grupo`, (deep) => {
          if (open) this.openGroups.delete(id);
          else this.openGroups.add(id);
          if (deep) forEachGroup(frame.children ?? [], (child) => {
            if (open) this.openGroups.delete(child);
            else this.openGroups.add(child);
          });
          this.handlers.changed();
        }),
      );
    } else {
      row.append(this.stub());
    }

    const kind = document.createElement("span");
    kind.className = "layer-icon";
    kind.append(
      icon(
        frame.type === "text"
          ? "text"
          : frame.type === "image"
            ? "image"
            : frame.type === "group"
              ? "group"
              : frame.type === "chart"
                ? "chart"
                : (frame.shape ?? "shape"),
        14,
      ),
    );

    const label = this.renaming === id
      ? this.renameField(frame, id)
      : this.nameLabel(frame, id, overset === true);

    const visible = frame.visible !== false;
    const eye = this.toggle(visible ? "visible" : "hidden", "Mostrar/ocultar", !visible, () => {
      this.store.commit(() => {
        const target = this.store.frame(id);
        if (target) target.visible = target.visible === false ? undefined : false;
      });
    });

    const lock = this.toggle(frame.locked ? "locked" : "unlocked", "Travar/destravar", frame.locked === true, () => {
      this.store.commit(() => {
        const target = this.store.frame(id);
        if (target) target.locked = target.locked ? undefined : true;
      });
    });

    row.append(kind, label, eye, lock);

    row.addEventListener("pointerdown", (event) => {
      if ((event.target as HTMLElement).closest(".layer-toggle, .layer-caret")) return;
      if ((event.target as HTMLElement).closest(".layer-rename")) return;

      const now = Date.now();
      const again =
        this.lastClick !== null && this.lastClick.id === id && now - this.lastClick.at < 500;
      this.lastClick = { id, at: now };

      if (again && !event.shiftKey) {
        // Without this the browser's own focus handling for the press moves
        // focus to the rebuilt row, the field that render is about to create
        // loses it at once, and its blur closes the rename before a key can
        // be typed. The field opened and vanished in the same frame.
        event.preventDefault();
        this.renaming = id;
        this.handlers.changed();
        return;
      }
      this.handlers.select(id, event.shiftKey);
    });

    // A folded group opens on double-click, the shortcut for "let me in".
    if (frame.type === "group") {
      row.addEventListener("dblclick", (event) => {
        if ((event.target as HTMLElement).closest(".layer-name, .layer-toggle")) return;
        if (this.openGroups.has(id)) this.openGroups.delete(id);
        else this.openGroups.add(id);
        this.handlers.changed();
      });
    }

    // ── Drag to reorder ─────────────────────────────────────────────────────
    row.addEventListener("dragstart", (event) => {
      this.dragging = id;
      event.dataTransfer?.setData("text/plain", id);
      row.classList.add("dragging");
    });

    row.addEventListener("dragend", () => {
      this.dragging = null;
      this.drop = null;
      row.classList.remove("dragging");
      this.handlers.changed();
    });

    row.addEventListener("dragover", (event) => {
      if (!this.dragging || this.dragging === id) return;
      event.preventDefault();

      const box = row.getBoundingClientRect();
      const position = (event.clientY - box.top) / box.height;

      row.classList.remove("drop-above", "drop-below", "drop-inside");

      // The middle third of a group row means "put it inside".
      if (frame.type === "group" && position > 0.3 && position < 0.7) {
        this.drop = { kind: "inside", page, parent: id };
        row.classList.add("drop-inside");
      } else if (position < 0.5) {
        // Rows read top-down but the array is bottom-up, so "above" is +1.
        this.drop = { kind: "sibling", page, parent, index: index + 1 };
        row.classList.add("drop-above");
      } else {
        this.drop = { kind: "sibling", page, parent, index };
        row.classList.add("drop-below");
      }
    });

    row.addEventListener("dragleave", () => {
      row.classList.remove("drop-above", "drop-below", "drop-inside");
    });

    row.addEventListener("drop", (event) => {
      event.preventDefault();
      row.classList.remove("drop-above", "drop-below", "drop-inside");

      const moving = this.dragging;
      const target = this.drop;
      if (!moving || !target) return;

      if (target.kind === "inside") {
        const children = this.store.frame(target.parent);
        const count = children?.type === "group" ? (children.children ?? []).length : 0;
        this.store.moveFrame(moving, target.page, target.parent, count);
        // Show where it went, rather than swallowing it into a closed group.
        this.openGroups.add(target.parent);
      } else {
        this.store.moveFrame(moving, target.page, target.parent, target.index);
      }

      this.dragging = null;
      this.drop = null;
      this.handlers.changed();
    });

    return row;
  }

  private nameLabel(frame: Frame, id: string, overset: boolean): HTMLElement {
    const label = document.createElement("span");
    label.className = "layer-name";
    label.textContent = frame.name ?? defaultName(frame);
    if (overset) label.classList.add("overset");
    label.title = overset
      ? `${id} — conteúdo não coube`
      : `${id} — clique duas vezes para renomear`;
    return label;
  }

  /** Rename in place; a document of two hundred frames needs names. */
  private renameField(frame: Frame, id: string): HTMLElement {
    const input = document.createElement("input");
    input.className = "layer-rename";
    input.value = frame.name ?? "";
    input.placeholder = defaultName(frame);

    let done = false;
    const finish = (keep: boolean) => {
      if (done) return;
      done = true;
      this.renaming = null;
      const value = input.value.trim();
      if (keep && value !== (frame.name ?? "")) {
        this.store.commit(() => {
          const target = this.store.frame(id);
          if (target) target.name = value || undefined;
        });
      } else {
        this.handlers.changed();
      }
    };

    input.addEventListener("blur", () => finish(true));
    input.addEventListener("keydown", (key) => {
      if (key.key === "Enter") finish(true);
      if (key.key === "Escape") finish(false);
      key.stopPropagation();
    });

    // The panel was just rebuilt, so the field is new every render: it has to
    // take focus each time or typing would go to the canvas.
    queueMicrotask(() => {
      input.focus();
      input.select();
    });
    return input;
  }

  /** `pinned` keeps the button visible even when the row is not hovered. */
  private toggle(
    name: string,
    title: string,
    pinned: boolean,
    onClick: () => void,
  ): HTMLElement {
    const button = document.createElement("button");
    button.type = "button";
    button.className = pinned ? "layer-toggle on" : "layer-toggle";
    button.title = title;
    button.append(icon(name, 14));
    button.addEventListener("click", (event) => {
      event.stopPropagation();
      onClick();
    });
    return button;
  }

  /**
   * Unfold whatever holds the selection.
   *
   * Only when the selection actually changed: otherwise folding a group with a
   * selected frame inside would undo itself on the very next render.
   */
  private reveal(state: LayersState): void {
    const signature = [...state.selected].sort().join(" ");
    if (signature === this.revealed) return;
    this.revealed = signature;
    if (state.selected.size === 0) return;

    const display = new Map(
      state.list.pages.flatMap((page) => page.frames.map((frame) => [frame.id, frame] as const)),
    );

    for (const id of state.selected) {
      for (const ancestor of display.get(id)?.ancestors ?? []) this.openGroups.add(ancestor);
      const page = this.store.locate(id)?.page;
      if (page === undefined) continue;
      const spec = this.store.doc.pages?.[page];
      if (spec) this.foldedPages.delete(pageKey(spec, page));
    }
  }
}

/** Stable across reorders when the page has an id, positional otherwise. */
function pageKey(page: Page, index: number): string {
  return page.id ?? `#${index}`;
}

function forEachGroup(frames: Frame[], visit: (id: string) => void): void {
  for (const frame of frames) {
    if (frame.type !== "group") continue;
    if (frame.id) visit(frame.id);
    forEachGroup(frame.children ?? [], visit);
  }
}

function defaultName(frame: Frame): string {
  if (frame.type === "text") {
    const first = frame.blocks?.[0];
    if (first?.type === "paragraph") {
      const text = first.content
        .map((inline) => (inline.type === "text" ? inline.text : ""))
        .join("")
        .trim();
      if (text) return text.length > 24 ? `${text.slice(0, 24)}…` : text;
    }
    if (frame.story) return `story: ${frame.story}`;
    return "Texto vazio";
  }
  if (frame.type === "image") return frame.src || "Imagem";
  if (frame.type === "shape") return { rect: "Retângulo", ellipse: "Elipse", line: "Linha" }[
    frame.shape ?? "rect"
  ];
  if (frame.type === "chart") {
    return { bar: "Barras", line: "Linha", area: "Área", point: "Dispersão" }[
      frame.mark ?? "bar"
    ];
  }
  return `Grupo (${frame.children?.length ?? 0})`;
}
