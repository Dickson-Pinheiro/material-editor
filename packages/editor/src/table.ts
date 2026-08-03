/**
 * The table, as the editor has to hold it to change it.
 *
 * The schema lets a cell say where it goes or say nothing and take the next
 * free slot. That is lovely to author and impossible to edit: "insert a column
 * before this one" has no meaning until every cell has a place. So a table is
 * *normalised on load* — every cell given an explicit `x` and `y` — exactly as
 * `store.ts` already expands a bare string into a paragraph, and for the same
 * reason: the editing code should only ever see the full form.
 *
 * `place` is the engine's own rule, written a second time. That is a real cost
 * and it is paid deliberately: the alternative is the engine handing back a
 * resolved grid, which would mean the display list carrying a whole geometry
 * nothing paints. The rule is small and it is fixed — pinned cells first, in
 * the order they were written, then the rest into what is left — and the tests
 * in `tests.ts` check this copy against the same cases the engine's do.
 */

import type { Cell, Paragraph, TableBlock, TrackSize } from "./types";

/** Where a cell sits once the grid is resolved. */
export interface Placed {
  cell: number;
  x: number;
  y: number;
  colspan: number;
  rowspan: number;
}

export interface Grid {
  cells: Placed[];
  columns: number;
  rows: number;
}

/** Ceiling on rows scanned before giving a cell up, as the engine has. */
const MAX_ROWS = 4096;

export function columnCount(table: TableBlock): number {
  if (table.columns && table.columns.length > 0) return table.columns.length;
  const widest = (table.cells ?? []).reduce((most, cell) => {
    if (cell.x == null) return most;
    return Math.max(most, cell.x + Math.max(1, cell.colspan ?? 1));
  }, 0);
  return Math.max(1, widest);
}

/** Resolve every cell to a position. The engine's rule, mirrored. */
export function place(table: TableBlock): Grid {
  const columns = columnCount(table);
  const cells = table.cells ?? [];
  const taken: boolean[] = [];
  const placed: (Placed | null)[] = cells.map(() => null);

  const occupy = (x: number, y: number, w: number, h: number) => {
    for (let row = y; row < y + h; row += 1) {
      for (let column = x; column < x + w; column += 1) {
        taken[row * columns + column] = true;
      }
    }
  };

  const free = (x: number, y: number, w: number, h: number) => {
    if (x + w > columns) return false;
    for (let row = y; row < y + h; row += 1) {
      for (let column = x; column < x + w; column += 1) {
        if (taken[row * columns + column]) return false;
      }
    }
    return true;
  };

  // The pinned ones, in the order they were written.
  cells.forEach((cell, index) => {
    if (cell.x == null || cell.y == null) return;
    const w = Math.max(1, cell.colspan ?? 1);
    const h = Math.max(1, cell.rowspan ?? 1);
    if (cell.x + w > columns || !free(cell.x, cell.y, w, h)) return;
    occupy(cell.x, cell.y, w, h);
    placed[index] = { cell: index, x: cell.x, y: cell.y, colspan: w, rowspan: h };
  });

  // The rest, into whatever is left.
  let cursor = 0;
  cells.forEach((cell, index) => {
    if (placed[index] || (cell.x != null && cell.y != null)) return;
    const w = Math.max(1, cell.colspan ?? 1);
    const h = Math.max(1, cell.rowspan ?? 1);
    if (w > columns) return;

    // A cell that pinned only its column waits for that column to come round;
    // one that pinned only its row starts scanning there.
    let at = cell.y != null ? cell.y * columns : cursor;
    for (;;) {
      const y = Math.floor(at / columns);
      const x = at % columns;
      if (y > MAX_ROWS) break;
      if ((cell.x == null || cell.x === x) && free(x, y, w, h)) {
        occupy(x, y, w, h);
        placed[index] = { cell: index, x, y, colspan: w, rowspan: h };
        if (cell.y == null && cell.x == null) cursor = at + w;
        break;
      }
      at += 1;
    }
  });

  const resolved = placed.filter((entry): entry is Placed => entry !== null);
  const rows = resolved.reduce((most, entry) => Math.max(most, entry.y + entry.rowspan), 0);
  return { cells: resolved, columns, rows: Math.max(rows, table.rows?.length ?? 0) };
}

/** Give every cell an explicit place, so editing has something to work on. */
export function normalizeTable(table: TableBlock): void {
  const grid = place(table);
  table.columns ??= [];
  if (table.columns.length === 0) {
    table.columns = Array.from({ length: grid.columns }, () => "auto" as TrackSize);
  }
  table.cells ??= [];
  for (const entry of grid.cells) {
    const cell = table.cells[entry.cell]!;
    cell.x = entry.x;
    cell.y = entry.y;
  }
  // A cell the grid could not place is a cell nothing can address. Rather than
  // leave it to be found later by whoever clicks near it, it is dropped here,
  // where the reason is at hand: it asked for a place the table does not have.
  table.cells = table.cells.filter((cell) => cell.x != null && cell.y != null);
}

/** The cell covering `(x, y)`, span included. */
export function cellAt(table: TableBlock, x: number, y: number): number | null {
  const grid = place(table);
  const hit = grid.cells.find(
    (entry) =>
      x >= entry.x && x < entry.x + entry.colspan && y >= entry.y && y < entry.y + entry.rowspan,
  );
  return hit ? hit.cell : null;
}

/** A blank cell holding one empty paragraph, ready to be typed into. */
export function emptyCell(x: number, y: number): Cell {
  const paragraph: Paragraph = { type: "paragraph", content: [{ type: "text", text: "" }] };
  return { x, y, blocks: [paragraph] };
}

/**
 * A fresh table of `rows` × `columns`, with a rule under the heading.
 *
 * Two rules and no vertical ones: the `booktabs` model, which is what makes a
 * table read. A grid of boxes is what a spreadsheet does, and a spreadsheet is
 * not a page.
 */
export function newTable(rows: number, columns: number, heading: boolean): TableBlock {
  const cells: Cell[] = [];
  for (let y = 0; y < rows; y += 1) {
    for (let x = 0; x < columns; x += 1) {
      cells.push(emptyCell(x, y));
    }
  }

  const table: TableBlock = {
    type: "table",
    columns: Array.from({ length: columns }, () => "auto" as TrackSize),
    rows: [],
    cells,
    inset: 4,
    lines: [
      { axis: "horizontal", at: 0, width: 1 },
      { axis: "horizontal", at: rows, width: 1 },
    ],
  };

  if (heading) {
    table.header = { rows: 1, repeat: true };
    table.lines!.splice(1, 0, { axis: "horizontal", at: 1, width: 0.5 });
  }
  return table;
}

/** Insert a column before `at`, or after the last when `at` is the count. */
export function insertColumn(table: TableBlock, at: number): void {
  const grid = place(table);
  table.columns ??= [];
  table.columns.splice(at, 0, "auto");

  for (const entry of grid.cells) {
    const cell = table.cells![entry.cell]!;
    // A cell the new column lands inside grows instead of moving: splitting a
    // span is a decision the author has to make, not one to make for them.
    if (entry.x >= at) cell.x = entry.x + 1;
    else if (entry.x + entry.colspan > at) cell.colspan = entry.colspan + 1;
  }

  const rows = Math.max(1, grid.rows);
  for (let y = 0; y < rows; y += 1) {
    if (cellAt(table, at, y) === null) table.cells!.push(emptyCell(at, y));
  }
  sortCells(table);
}

/** Insert a row before `at`, or after the last when `at` is the count. */
export function insertRow(table: TableBlock, at: number): void {
  const grid = place(table);
  table.rows ??= [];
  if (at < table.rows.length) table.rows.splice(at, 0, "auto");

  for (const entry of grid.cells) {
    const cell = table.cells![entry.cell]!;
    if (entry.y >= at) cell.y = entry.y + 1;
    else if (entry.y + entry.rowspan > at) cell.rowspan = entry.rowspan + 1;
  }

  for (let x = 0; x < grid.columns; x += 1) {
    if (cellAt(table, x, at) === null) table.cells!.push(emptyCell(x, at));
  }
  shiftLines(table, "horizontal", at);
  sortCells(table);
}

/** Take a column out, and everything in it. */
export function removeColumn(table: TableBlock, at: number): void {
  const grid = place(table);
  if (grid.columns <= 1) return;

  table.columns?.splice(at, 1);
  const survivors: Cell[] = [];
  for (const entry of grid.cells) {
    const cell = table.cells![entry.cell]!;
    if (entry.x === at && entry.colspan === 1) continue;
    if (entry.x > at) cell.x = entry.x - 1;
    else if (entry.x + entry.colspan > at) cell.colspan = entry.colspan - 1;
    survivors.push(cell);
  }
  table.cells = survivors;
  sortCells(table);
}

/** Take a row out, and everything in it. */
export function removeRow(table: TableBlock, at: number): void {
  const grid = place(table);
  if (grid.rows <= 1) return;

  if (at < (table.rows?.length ?? 0)) table.rows!.splice(at, 1);
  const survivors: Cell[] = [];
  for (const entry of grid.cells) {
    const cell = table.cells![entry.cell]!;
    if (entry.y === at && entry.rowspan === 1) continue;
    if (entry.y > at) cell.y = entry.y - 1;
    else if (entry.y + entry.rowspan > at) cell.rowspan = entry.rowspan - 1;
    survivors.push(cell);
  }
  table.cells = survivors;
  shiftLines(table, "horizontal", at, -1);
  sortCells(table);
}

/**
 * Rules at or past `at` move with the boundary they were declared for.
 *
 * A rule under the heading has to stay under the heading when a row is pushed
 * in above it — that is the whole point of declaring it as a rule instead of
 * as a border on eight cells.
 */
function shiftLines(table: TableBlock, axis: "horizontal" | "vertical", at: number, by = 1): void {
  if (!table.lines) return;
  table.lines = table.lines.filter((line) => {
    if ((line.axis ?? "horizontal") !== axis) return true;
    const where = line.at ?? 0;
    if (by < 0 && where === at) return false;
    if (where >= at) line.at = where + by;
    return true;
  });
}

/** Reading order, so the JSON stays legible and Tab has an order to follow. */
export function sortCells(table: TableBlock): void {
  table.cells?.sort((a, b) => (a.y ?? 0) - (b.y ?? 0) || (a.x ?? 0) - (b.x ?? 0));
}

/**
 * The cell that Tab should go to next, in reading order.
 *
 * `null` at the end of the table, which is what tells the caller to leave it
 * rather than wrap around to the top — wrapping loses your place silently.
 */
export function nextCell(table: TableBlock, from: number, direction: 1 | -1): number | null {
  const grid = place(table);
  const order = [...grid.cells].sort((a, b) => a.y - b.y || a.x - b.x);
  const index = order.findIndex((entry) => entry.cell === from);
  if (index < 0) return null;
  const next = order[index + direction];
  return next ? next.cell : null;
}

/** How a track's size reads in the inspector's selector. */
export function trackKind(size: TrackSize | undefined): "auto" | "fixed" | "fraction" | "percent" {
  if (size == null || size === "auto") return "auto";
  if (typeof size === "number") return "fixed";
  if (size.endsWith("fr")) return "fraction";
  if (size.endsWith("%")) return "percent";
  return "fixed";
}

/** The number inside a track size, for the field beside the selector. */
export function trackAmount(size: TrackSize | undefined): number {
  if (size == null || size === "auto") return 0;
  if (typeof size === "number") return size;
  const number = Number.parseFloat(size);
  return Number.isFinite(number) ? number : 0;
}

/** Build a track size back from the selector and the number. */
export function trackOf(kind: string, amount: number): TrackSize {
  switch (kind) {
    case "fixed":
      return amount;
    case "fraction":
      return `${amount || 1}fr`;
    case "percent":
      return `${amount || 25}%`;
    default:
      return "auto";
  }
}
