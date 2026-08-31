/**
 * Document state: normalisation, mutation, undo, and re-layout.
 *
 * The document JSON is the single source of truth. Every edit — dragging a
 * frame, typing a character, changing a font size — mutates that JSON and asks
 * the engine to lay it out again. There is no second model that could drift.
 */

import type { Engine } from "./engine";
import { normalizeTable, sortCells } from "./table";
import type {
  Block,
  DisplayList,
  DocumentSpec,
  Frame,
  Inline,
  Len,
  Paragraph,
  SourceRef,
  TableBlock,
  TextFrame,
} from "./types";

/** Where a frame lives, so it can be moved, reordered or deleted. */
export interface FrameLocation {
  frame: Frame;
  siblings: Frame[];
  index: number;
  page: number;
}

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The human half of an engine error, without the wrapper noise. */
function messageOf(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  return raw.replace(/^documento inválido:\s*/, "").replace(/\s+at line \d+ column \d+$/, "");
}

function round(value: number): number {
  // A non-finite value would serialise as `null` and the engine would reject
  // the whole document, so it stops here rather than three layers down.
  return Number.isFinite(value) ? Math.round(value * 100) / 100 : 0;
}

const PT_PER: Record<string, number> = {
  pt: 1,
  mm: 72 / 25.4,
  cm: 72 / 2.54,
  in: 72,
  px: 72 / 96,
};

/**
 * Resolve a length to points.
 *
 * The schema accepts `"18mm"` wherever a number fits, which is lovely to author
 * and treacherous to compute with: `Number("18mm")` is `NaN`. Everything in the
 * editor that does arithmetic on geometry goes through here.
 */
export function parseLen(value: Len | undefined): number {
  if (typeof value === "number") return Number.isFinite(value) ? value : 0;
  if (typeof value !== "string") return 0;

  const match = value.trim().match(/^(-?\d*\.?\d+)\s*([a-z]*)$/i);
  if (!match) return 0;

  const amount = Number.parseFloat(match[1]!);
  const unit = (match[2] || "pt").toLowerCase();
  return Number.isFinite(amount) ? amount * (PT_PER[unit] ?? 1) : 0;
}

type Listener = () => void;

export class Store {
  doc: DocumentSpec;
  list: DisplayList;
  /** Why the last change was refused, for the interface to show. */
  lastError: string | null = null;

  private readonly listeners = new Set<Listener>();
  private readonly undoStack: string[] = [];
  private readonly redoStack: string[] = [];
  /** Snapshot taken when a drag began, committed when it ends. */
  private pending: string | null = null;

  constructor(
    private readonly engine: Engine,
    document: DocumentSpec,
  ) {
    this.doc = normalize(document);
    this.list = this.engine.layout(this.doc);
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    for (const listener of this.listeners) listener();
  }

  // ── Mutation ──────────────────────────────────────────────────────────────

  /**
   * Apply a change, and undo it if the engine will not accept the result.
   *
   * The engine refuses a document it cannot parse — one bad length in one field
   * is enough. Without a rollback the document would stay mutated and invalid,
   * every later layout would throw, and the editor would quietly stop
   * responding. So a mutation is a transaction: the snapshot goes back if the
   * layout fails, and the reason is put where the interface can show it.
   *
   * Returns whether the change stuck.
   */
  private apply(mutate: (doc: DocumentSpec) => void, undoable: boolean): boolean {
    const snapshot = JSON.stringify(this.doc);

    mutate(this.doc);

    try {
      this.list = this.engine.layout(this.doc);
    } catch (error) {
      this.doc = JSON.parse(snapshot) as DocumentSpec;
      this.lastError = messageOf(error);
      // The snapshot was valid, so this cannot fail in turn.
      this.list = this.engine.layout(this.doc);
      this.notify();
      return false;
    }

    if (undoable) {
      this.undoStack.push(snapshot);
      this.redoStack.length = 0;
      if (this.undoStack.length > 200) this.undoStack.shift();
    }

    this.lastError = null;
    this.notify();
    return true;
  }

  /** Apply a change as one undoable step. */
  commit(mutate: (doc: DocumentSpec) => void): boolean {
    return this.apply(mutate, true);
  }

  /**
   * Start a continuous gesture. Everything until {@link endGesture} collapses
   * into a single undo step, so dragging a frame is one entry, not two hundred.
   */
  beginGesture(): void {
    if (this.pending === null) this.pending = JSON.stringify(this.doc);
  }

  /** Apply an intermediate change without touching the undo stack. */
  update(mutate: (doc: DocumentSpec) => void): boolean {
    return this.apply(mutate, false);
  }

  endGesture(): void {
    if (this.pending === null) return;
    // Only record the gesture if it actually changed something.
    if (this.pending !== JSON.stringify(this.doc)) {
      this.undoStack.push(this.pending);
      this.redoStack.length = 0;
    }
    this.pending = null;
  }

  undo(): boolean {
    const previous = this.undoStack.pop();
    if (previous === undefined) return false;
    this.redoStack.push(JSON.stringify(this.doc));
    this.restore(previous);
    return true;
  }

  redo(): boolean {
    const next = this.redoStack.pop();
    if (next === undefined) return false;
    this.undoStack.push(JSON.stringify(this.doc));
    this.restore(next);
    return true;
  }

  private restore(snapshot: string): void {
    this.doc = JSON.parse(snapshot) as DocumentSpec;
    try {
      this.list = this.engine.layout(this.doc);
      this.lastError = null;
    } catch (error) {
      // Only reachable if a snapshot predates a schema change.
      this.lastError = messageOf(error);
    }
    this.notify();
  }

  canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  // ── Lookup ────────────────────────────────────────────────────────────────

  locate(id: string): FrameLocation | null {
    const pages = this.doc.pages ?? [];
    for (let page = 0; page < pages.length; page += 1) {
      const found = search(pages[page]?.frames ?? [], id, page);
      if (found) return found;
    }
    return null;

    function search(frames: Frame[], id: string, page: number): FrameLocation | null {
      for (let index = 0; index < frames.length; index += 1) {
        const frame = frames[index]!;
        if (frame.id === id) return { frame, siblings: frames, index, page };
        if (frame.type === "group") {
          const nested = search(frame.children, id, page);
          if (nested) return nested;
        }
      }
      return null;
    }
  }

  frame(id: string): Frame | null {
    return this.locate(id)?.frame ?? null;
  }

  /**
   * The block list a source reference points into.
   *
   * A named story, or the frame's own blocks — and then, for anything painted
   * inside a table, down the cell trail to the cell's own list. Every index
   * in the reference is read against whatever this returns, which is the
   * whole reason the trail exists: without it, a caret in a cell would edit
   * the frame's paragraph at the same index.
   */
  blocksOf(source: SourceRef): Block[] | null {
    let blocks: Block[] | null;
    if (source.story) {
      blocks = this.doc.resources?.stories?.[source.story] ?? null;
    } else {
      const frame = this.frame(source.frame);
      if (!frame || frame.type !== "text") return null;
      frame.blocks ??= [];
      blocks = frame.blocks;
    }

    // Um passo da trilha desce para dentro de um bloco que tem lista própria.
    // Tabela desce por `cells[step.cell]`; painel desce por `blocks`, e traz
    // sempre `cell: 0` porque tem um compartimento só. Qual dos dois é se lê
    // do bloco que está no índice, não do passo.
    for (const step of source.cells ?? []) {
      const container = blocks?.[step.block];
      if (!container) return null;

      if (container.type === "table") {
        const cell = container.cells?.[step.cell];
        if (!cell) return null;
        cell.blocks ??= [];
        blocks = cell.blocks;
        continue;
      }

      if (container.type === "panel") {
        container.blocks ??= [];
        blocks = container.blocks;
        continue;
      }

      return null;
    }
    return blocks;
  }

  /**
   * The table a trail passes through, at `depth` steps down.
   *
   * `null` when that step is a panel — the trail carries both kinds, and only
   * the caller that wants a table cares which one it landed on.
   */
  tableAt(source: SourceRef, depth = 0): TableBlock | null {
    const step = source.cells?.[depth];
    if (!step) return null;
    const outer: SourceRef = { ...source, cells: (source.cells ?? []).slice(0, depth) };
    const block = this.blocksOf(outer)?.[step.block];
    return block && block.type === "table" ? block : null;
  }

  // ── Hierarchy ─────────────────────────────────────────────────────────────

  /**
   * Move a frame to a new place in the tree.
   *
   * Coordinates are rewritten so the frame does not visibly jump: a frame's
   * rect is relative to its parent group, so changing parents means changing
   * the numbers.
   */
  moveFrame(id: string, page: number, parent: string | null, index: number): void {
    const source = this.locate(id);
    if (!source || parent === id) return;
    // Refuse to drop a group inside itself.
    if (parent && this.isAncestor(id, parent)) return;

    this.commit((doc) => {
      const from = this.locate(id);
      if (!from) return;

      const absolute = this.absoluteOrigin(id);
      const [frame] = from.siblings.splice(from.index, 1);
      if (!frame) return;

      const target = parent ? this.frame(parent) : null;
      const destination =
        target && target.type === "group"
          ? (target.children ??= [])
          : (doc.pages[page]?.frames ?? from.siblings);

      // Same list and moving down: the splice above shifted everything.
      const at =
        destination === from.siblings && from.index < index ? index - 1 : index;

      const parentOrigin = parent ? this.absoluteOrigin(parent) : { x: 0, y: 0 };
      frame.rect[0] = round(absolute.x - parentOrigin.x);
      frame.rect[1] = round(absolute.y - parentOrigin.y);

      destination.splice(Math.max(0, Math.min(at, destination.length)), 0, frame);
    });
  }

  /** Wrap frames in a new group, preserving their positions. */
  group(ids: string[]): string | null {
    if (ids.length < 2) return null;

    const located = ids.map((id) => this.locate(id)).filter((l): l is FrameLocation => l !== null);
    if (located.length < 2) return null;

    // Only siblings can be grouped; anything else would change the tree shape.
    const [first] = located;
    if (!first || located.some((l) => l.siblings !== first.siblings)) return null;

    const boxes = ids.map((id) => this.absoluteBox(id)).filter((b): b is Box => b !== null);
    if (boxes.length < 2) return null;

    const x = Math.min(...boxes.map((b) => b.x));
    const y = Math.min(...boxes.map((b) => b.y));
    const right = Math.max(...boxes.map((b) => b.x + b.w));
    const bottom = Math.max(...boxes.map((b) => b.y + b.h));

    const groupId = newFrameId("grupo");
    const at = Math.min(...located.map((l) => l.index));

    this.commit(() => {
      const siblings = first.siblings;
      const taken: Frame[] = [];

      // Back to front, so the indices stay valid while splicing.
      for (let index = siblings.length - 1; index >= 0; index -= 1) {
        const frame = siblings[index]!;
        if (frame.id && ids.includes(frame.id)) {
          taken.unshift(...siblings.splice(index, 1));
        }
      }

      for (const frame of taken) {
        frame.rect[0] = round(parseLen(frame.rect[0]) - x);
        frame.rect[1] = round(parseLen(frame.rect[1]) - y);
      }

      siblings.splice(Math.min(at, siblings.length), 0, {
        id: groupId,
        type: "group",
        rect: [round(x), round(y), round(right - x), round(bottom - y)],
        children: taken,
      });
    });

    return groupId;
  }

  /** Dissolve a group, lifting its children into the parent. */
  ungroup(id: string): string[] {
    const located = this.locate(id);
    if (!located || located.frame.type !== "group") return [];

    const children = located.frame.children ?? [];
    const ids = children.map((child) => child.id).filter((v): v is string => Boolean(v));
    const origin = { x: parseLen(located.frame.rect[0]), y: parseLen(located.frame.rect[1]) };

    this.commit(() => {
      const current = this.locate(id);
      if (!current || current.frame.type !== "group") return;

      const lifted = (current.frame.children ?? []).map((child) => {
        child.rect[0] = round(parseLen(child.rect[0]) + origin.x);
        child.rect[1] = round(parseLen(child.rect[1]) + origin.y);
        return child;
      });

      current.siblings.splice(current.index, 1, ...lifted);
    });

    return ids;
  }

  // ── Copying ───────────────────────────────────────────────────────────────

  /**
   * Deep-copy frames, ready to be inserted elsewhere.
   *
   * The result follows document order rather than selection order, so pasting
   * a group of shapes keeps them stacked the way they were. Coordinates are
   * made absolute: a copied child of a group must stand on its own.
   */
  cloneFrames(ids: string[]): Frame[] {
    const wanted = new Set(ids);
    const out: Frame[] = [];

    for (const page of this.doc.pages ?? []) {
      collect(page.frames ?? [], 0, 0);
    }
    return out;

    function collect(frames: Frame[], originX: number, originY: number): void {
      for (const frame of frames) {
        const x = originX + parseLen(frame.rect[0]);
        const y = originY + parseLen(frame.rect[1]);

        if (frame.id && wanted.has(frame.id)) {
          const copy = structuredClone(frame);
          copy.rect = [x, y, frame.rect[2], frame.rect[3]];
          out.push(copy);
          // Its children travel with it; no need to look inside.
          continue;
        }
        if (frame.type === "group") collect(frame.children ?? [], x, y);
      }
    }
  }

  /**
   * Insert copies onto a page, offset by `dx`/`dy`, and return their new ids.
   *
   * Ids are regenerated all the way down — two frames answering to the same
   * name would break threading, provenance and selection at once.
   */
  insertFrames(frames: Frame[], page: number, dx = 0, dy = 0): string[] {
    if (frames.length === 0) return [];
    const ids: string[] = [];

    this.commit((doc) => {
      const target = doc.pages[Math.min(page, doc.pages.length - 1)];
      if (!target) return;
      target.frames ??= [];

      for (const frame of frames) {
        const copy = structuredClone(frame);
        copy.rect = [
          round(parseLen(copy.rect[0]) + dx),
          round(parseLen(copy.rect[1]) + dy),
          copy.rect[2],
          copy.rect[3],
        ];
        renameDeep(copy);
        ids.push(copy.id!);
        target.frames.push(copy);
      }
    });

    return ids;

    function renameDeep(frame: Frame): void {
      frame.id = newFrameId(frame.type);
      // A copy must not inherit a thread target that still points at the
      // original chain, or the two would fight over the same content.
      if (frame.type === "text") frame.threadNext = undefined;
      if (frame.type === "group") (frame.children ?? []).forEach(renameDeep);
    }
  }

  /**
   * Move frames to another page, shifting them by the gap between the two
   * pages' positions on the canvas so they stay under the cursor.
   *
   * Pages are siblings: a frame belongs to exactly one, and dragging it across
   * has to say so. Leaving it on the old page would keep it painted with that
   * page — under the next page's paper.
   */
  moveToPage(ids: string[], page: number, dx: number, dy: number): void {
    const moving = ids.filter((id) => {
      const located = this.locate(id);
      return located !== null && located.page !== page;
    });
    if (moving.length === 0) return;

    this.commit((doc) => {
      const target = doc.pages[page];
      if (!target) return;
      target.frames ??= [];

      for (const id of moving) {
        const located = this.locate(id);
        if (!located) continue;

        const absolute = this.absoluteOrigin(id);
        const [frame] = located.siblings.splice(located.index, 1);
        if (!frame) continue;

        // Out of any group and onto the page, at the same place on screen.
        frame.rect[0] = round(absolute.x + dx);
        frame.rect[1] = round(absolute.y + dy);
        target.frames.push(frame);
      }
    });
  }

  /** Move a frame one step up or down among its siblings. */
  nudgeOrder(id: string, direction: 1 | -1): void {
    const located = this.locate(id);
    if (!located) return;
    const target = located.index + direction;
    if (target < 0 || target >= located.siblings.length) return;

    this.commit(() => {
      const current = this.locate(id);
      if (!current) return;
      const [frame] = current.siblings.splice(current.index, 1);
      if (frame) current.siblings.splice(current.index + direction, 0, frame);
    });
  }

  /** True when `ancestor` is somewhere above `id` in the tree. */
  isAncestor(ancestor: string, id: string): boolean {
    const frame = this.frame(ancestor);
    if (!frame || frame.type !== "group") return false;

    const search = (frames: Frame[]): boolean =>
      frames.some(
        (child) => child.id === id || (child.type === "group" && search(child.children ?? [])),
      );
    return search(frame.children ?? []);
  }

  /** Top-left of a frame in page coordinates, walking out through groups. */
  private absoluteOrigin(id: string): { x: number; y: number } {
    const box = this.absoluteBox(id);
    return box ? { x: box.x, y: box.y } : { x: 0, y: 0 };
  }

  private absoluteBox(id: string): Box | null {
    for (const page of this.doc.pages ?? []) {
      const found = walk(page.frames ?? [], 0, 0);
      if (found) return found;
    }
    return null;

    function walk(frames: Frame[], originX: number, originY: number): Box | null {
      for (const frame of frames) {
        const x = originX + parseLen(frame.rect[0]);
        const y = originY + parseLen(frame.rect[1]);
        if (frame.id === id) {
          return { x, y, w: parseLen(frame.rect[2]), h: parseLen(frame.rect[3]) };
        }
        if (frame.type === "group") {
          const nested = walk(frame.children ?? [], x, y);
          if (nested) return nested;
        }
      }
      return null;
    }
  }

  /** The paragraph a source reference points at, if it is one. */
  paragraphOf(source: SourceRef): Paragraph | null {
    const blocks = this.blocksOf(source);
    const index = source.block ?? -1;
    const block = blocks?.[index];
    return block && block.type === "paragraph" ? block : null;
  }

  /** The text run a source reference points at, if it is one. */
  runOf(source: SourceRef): { run: Extract<Inline, { type: "text" }>; paragraph: Paragraph } | null {
    const paragraph = this.paragraphOf(source);
    const inline = paragraph?.content[source.inline ?? -1];
    if (!paragraph || !inline || inline.type !== "text") return null;
    return { run: inline, paragraph };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Normalisation
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Expand every shorthand the schema allows and give each frame an explicit id.
 *
 * The schema accepts a bare string where a block or inline is expected, which
 * is lovely to author and awkward to edit — a string has nowhere to record a
 * style. Normalising once, on load, means the editing code only ever sees the
 * full form. The ids match what the engine would assign, so a document that
 * omits them keeps the same identities it had before.
 */
export function normalize(document: DocumentSpec): DocumentSpec {
  const doc: DocumentSpec = JSON.parse(JSON.stringify(document));
  doc.pages ??= [];
  doc.resources ??= {};

  doc.pages.forEach((page, index) => {
    page.frames ??= [];
    assignIds(page.frames, `p${index}.`);
    page.frames.forEach(normalizeFrame);
  });

  for (const [name, master] of Object.entries(doc.resources.masters ?? {})) {
    master.frames ??= [];
    assignIds(master.frames, `m:${name}.`);
    master.frames.forEach(normalizeFrame);
  }

  for (const blocks of Object.values(doc.resources.stories ?? {})) {
    normalizeBlocks(blocks);
  }

  return doc;
}

function assignIds(frames: Frame[], prefix: string): void {
  frames.forEach((frame, index) => {
    if (!frame.id) frame.id = `${prefix}f${index}`;
    if (frame.type === "group") assignIds(frame.children ?? [], `${frame.id}.`);
  });
}

function normalizeFrame(frame: Frame): void {
  // Geometry becomes plain points on load. The unit spellings are for authors;
  // the editor writes numbers back anyway, so resolving once keeps every
  // arithmetic path — drag, group, align, copy — working on one representation.
  frame.rect = [
    parseLen(frame.rect?.[0]),
    parseLen(frame.rect?.[1]),
    parseLen(frame.rect?.[2]),
    parseLen(frame.rect?.[3]),
  ];

  if (frame.type === "text") {
    frame.blocks ??= [];
    normalizeBlocks(frame.blocks);
  } else if (frame.type === "group") {
    (frame.children ?? []).forEach(normalizeFrame);
  }
}

function normalizeBlocks(blocks: Block[]): void {
  for (let index = 0; index < blocks.length; index += 1) {
    const block = blocks[index] as Block | string;
    if (typeof block === "string") {
      blocks[index] = { type: "paragraph", content: [{ type: "text", text: block }] };
      continue;
    }
    if (block.type === "paragraph") {
      block.content ??= [];
      normalizeInlines(block.content);
    }
    if (block.type === "table") {
      // Every cell given a place, so "insert a column before this one" has a
      // meaning. The schema's next-free-slot shorthand is for authors; the
      // editing code should only ever see the resolved form.
      block.cells ??= [];
      normalizeTable(block);
      for (const cell of block.cells) {
        cell.blocks ??= [];
        normalizeBlocks(cell.blocks);
      }
      sortCells(block);
    }
  }
}

function normalizeInlines(inlines: Inline[]): void {
  for (let index = 0; index < inlines.length; index += 1) {
    const inline = inlines[index] as Inline | string;
    if (typeof inline === "string") {
      inlines[index] = { type: "text", text: inline };
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame construction
// ─────────────────────────────────────────────────────────────────────────────

let created = 0;

/** A fresh frame id that cannot collide with the engine's `p0.f0` scheme. */
export function newFrameId(kind: string): string {
  created += 1;
  return `${kind}-${created}-${Math.random().toString(36).slice(2, 7)}`;
}

export function newTextFrame(x: number, y: number): TextFrame {
  return {
    id: newFrameId("texto"),
    type: "text",
    rect: [x, y, 200, 60],
    padding: 4,
    blocks: [{ type: "paragraph", content: [{ type: "text", text: "Texto novo" }] }],
  };
}

export function newShapeFrame(x: number, y: number): Frame {
  return {
    id: newFrameId("forma"),
    type: "shape",
    shape: "rect",
    rect: [x, y, 160, 100],
    fill: "#dbe7f3",
    border: { width: 1, color: "#1f4e79" },
  };
}

/**
 * A chart with three observations already in it.
 *
 * Not an empty one: an empty chart is a blank rectangle with nothing to say
 * what any control does, and the first thing anyone has to do with it is
 * invent data before they can see whether they wanted a chart at all.
 */
export function newChartFrame(x: number, y: number): Frame {
  return {
    id: newFrameId("grafico"),
    type: "chart",
    rect: [x, y, 260, 180],
    mark: "bar",
    data: [
      { categoria: "um", valor: 12 },
      { categoria: "dois", valor: 19 },
      { categoria: "três", valor: 8 },
    ],
    encoding: {
      x: { field: "categoria", kind: "categorical" },
      y: { field: "valor" },
    },
  };
}

export function newImageFrame(x: number, y: number, src: string): Frame {
  return {
    id: newFrameId("imagem"),
    type: "image",
    rect: [x, y, 200, 150],
    src,
    fit: "contain",
  };
}
