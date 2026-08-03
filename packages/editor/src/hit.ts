/**
 * Turning coordinates into meaning.
 *
 * A click is a point on a page; the editor needs a frame, or a caret position
 * in the source JSON. Both answers come out of the display list, because the
 * display list is the only thing that knows where the engine actually put
 * everything.
 */

import { utf8Length } from "./utf8";
import type {
  Caret,
  CellStep,
  DisplayFrame,
  DisplayItem,
  DisplayList,
  DisplayPage,
  GlyphRun,
  Rect,
  SourceRef,
} from "./types";

/** A run flattened out of the paint tree, paired with its provenance. */
export interface PlacedRun {
  run: GlyphRun;
  source: SourceRef;
}

/**
 * Every glyph run on a page, in paint order.
 *
 * Group transforms are ignored: rotated text is painted correctly but is not
 * editable in place, which matches what most tools do.
 */
export function collectRuns(page: DisplayPage): PlacedRun[] {
  const out: PlacedRun[] = [];

  const walk = (items: DisplayPage["items"]): void => {
    for (const item of items) {
      if (item.type === "group") walk(item.items);
      else if (item.type === "glyphs" && item.source) {
        out.push({ run: item, source: item.source });
      }
    }
  };

  walk(page.items);
  return out;
}

// ─────────────────────────────────────────────────────────────────────────────
// Frames
// ─────────────────────────────────────────────────────────────────────────────

/** Rotate a point back into a frame's own unrotated space. */
function unrotate(frame: DisplayFrame, x: number, y: number): [number, number] {
  if (!frame.rotation) return [x, y];
  const radians = (-frame.rotation * Math.PI) / 180;
  const cx = frame.rect.x + frame.rect.w / 2;
  const cy = frame.rect.y + frame.rect.h / 2;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const dx = x - cx;
  const dy = y - cy;
  return [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos];
}

export function frameContains(frame: DisplayFrame, x: number, y: number, slack = 0): boolean {
  const [px, py] = unrotate(frame, x, y);
  const r = frame.rect;
  return (
    px >= r.x - slack && px <= r.x + r.w + slack && py >= r.y - slack && py <= r.y + r.h + slack
  );
}

/**
 * The topmost frame under a point.
 *
 * Frames are recorded in paint order, so the last match wins. Locked frames and
 * groups' children are skipped — clicking a grouped child selects the group.
 */
export function frameAt(page: DisplayPage, x: number, y: number): DisplayFrame | null {
  for (let index = page.frames.length - 1; index >= 0; index -= 1) {
    const frame = page.frames[index]!;
    if (frame.locked || frame.ancestors.length > 0) continue;
    if (frameContains(frame, x, y)) return frame;
  }
  return null;
}

/** Every frame under a point, topmost first — used for alt-click drilling. */
export function framesAt(page: DisplayPage, x: number, y: number): DisplayFrame[] {
  return page.frames.filter((frame) => frameContains(frame, x, y)).reverse();
}

// ─────────────────────────────────────────────────────────────────────────────
// Carets
// ─────────────────────────────────────────────────────────────────────────────

function ownerKey(source: SourceRef | Caret): string {
  const owner =
    "story" in source && source.story ? `story:${source.story}` : `frame:${source.frame}`;
  // Two carets in different cells of the same table are in different lists of
  // blocks, and comparing their block indices would be comparing apples. The
  // trail is part of what says *whose* block index this is.
  return owner + trailKey(source.cells);
}

/** The cell trail as a string, so two of them compare in one step. */
export function trailKey(cells: CellStep[] | null | undefined): string {
  return (cells ?? []).map((step) => `/${step.block}:${step.cell}`).join("");
}

/** True when two carets sit in the same list of blocks. */
export function sameTrail(a: Caret, b: Caret): boolean {
  return trailKey(a.cells) === trailKey(b.cells);
}

function metricsOf(list: DisplayList, run: GlyphRun): { ascent: number; descent: number } {
  const font = list.fonts[run.font];
  const ascender = font?.ascender ?? 0.8;
  const descender = font?.descender ?? -0.2;
  return { ascent: run.size * ascender, descent: run.size * -descender };
}

/** Vertical band a run occupies, used for line-level hit testing. */
function bandOf(list: DisplayList, run: GlyphRun): { top: number; bottom: number } {
  const { ascent, descent } = metricsOf(list, run);
  return { top: run.y - ascent, bottom: run.y + descent };
}

/**
 * The cell whose box contains a point, innermost first.
 *
 * A table emits, per cell, a rectangle with no fill and no stroke saying
 * where that cell is. It is the only way to reach an empty cell: a caret is
 * placed by finding the glyph nearest the click, and an empty cell has no
 * glyph. Without this an empty table is a thing you can see and cannot enter.
 */
export function cellBoxAt(page: DisplayPage, x: number, y: number): SourceRef | null {
  let best: { source: SourceRef; area: number } | null = null;
  const keep = (source: SourceRef, area: number) => {
    // The smallest box wins, so a cell inside a nested table beats the cell
    // of the table that holds it.
    if (!best || area < best.area) best = { source, area };
  };

  const walk = (items: DisplayItem[]) => {
    for (const item of items) {
      if (item.type === "group") {
        walk(item.items);
        continue;
      }
      if (item.type !== "rect") continue;
      if (item.fill || item.stroke) continue;
      const source = item.source;
      if (!source?.cells?.length) continue;

      const { rect } = item;
      if (x < rect.x || x > rect.x + rect.w || y < rect.y || y > rect.y + rect.h) continue;

      keep(source, rect.w * rect.h);
    }
  };

  walk(page.items);
  return best ? (best as { source: SourceRef }).source : null;
}

/** Where a caret goes when a cell is entered by clicking its empty middle. */
export function caretInCell(source: SourceRef): Caret {
  return {
    frame: source.frame,
    story: source.story ?? null,
    cells: source.cells ?? [],
    block: 0,
    inline: 0,
    offset: 0,
  };
}

/**
 * The caret position closest to a point.
 *
 * Picks the line whose band contains the point (or the nearest baseline when
 * the click lands between lines), then the closest glyph boundary on it.
 */
export function caretAt(
  list: DisplayList,
  page: DisplayPage,
  x: number,
  y: number,
  frameId?: string,
): Caret | null {
  let runs = collectRuns(page);
  if (frameId) runs = runs.filter((placed) => placed.source.frame === frameId);
  if (runs.length === 0) return null;

  // Group runs into lines by baseline; a line is what the eye picks first.
  const onLine = runs.filter((placed) => {
    const band = bandOf(list, placed.run);
    return y >= band.top && y <= band.bottom;
  });

  const candidates = onLine.length > 0 ? onLine : nearestLine(list, runs, y);
  if (candidates.length === 0) return null;

  // Closest run horizontally, then the closest boundary inside it.
  let best: PlacedRun | null = null;
  let bestDistance = Infinity;
  for (const placed of candidates) {
    const { run } = placed;
    const distance =
      x < run.x ? run.x - x : x > run.x + run.width ? x - (run.x + run.width) : 0;
    if (distance < bestDistance) {
      bestDistance = distance;
      best = placed;
    }
  }
  if (!best) return null;

  return caretInRun(best, x);
}

function nearestLine(list: DisplayList, runs: PlacedRun[], y: number): PlacedRun[] {
  let bestBaseline: number | null = null;
  let bestDistance = Infinity;

  for (const placed of runs) {
    const band = bandOf(list, placed.run);
    const distance = y < band.top ? band.top - y : y > band.bottom ? y - band.bottom : 0;
    if (distance < bestDistance) {
      bestDistance = distance;
      bestBaseline = placed.run.y;
    }
  }

  return bestBaseline === null
    ? []
    : runs.filter((placed) => Math.abs(placed.run.y - bestBaseline) < 0.01);
}

/** The boundary in a run nearest to `x`, as a source-coordinate caret. */
export function caretInRun(placed: PlacedRun, x: number): Caret {
  const { run, source } = placed;
  const base = source.offset ?? 0;

  let offset = base + utf8Length(run.text);
  let bestDistance = Math.abs(x - (run.x + run.width));

  for (const glyph of run.glyphs) {
    const distance = Math.abs(x - (run.x + glyph.x));
    if (distance < bestDistance) {
      bestDistance = distance;
      offset = base + glyph.cluster;
    }
  }

  return {
    frame: source.frame,
    story: source.story ?? null,
    cells: source.cells ?? [],
    block: source.block ?? 0,
    inline: source.inline ?? 0,
    offset,
  };
}

/** Order two carets in reading order. `0` when they are the same spot. */
export function compareCarets(a: Caret, b: Caret): number {
  if (ownerKey(a) !== ownerKey(b)) return ownerKey(a) < ownerKey(b) ? -1 : 1;
  if (a.block !== b.block) return a.block - b.block;
  if (a.inline !== b.inline) return a.inline - b.inline;
  return a.offset - b.offset;
}

export function sameCaret(a: Caret | null, b: Caret | null): boolean {
  if (!a || !b) return a === b;
  return compareCarets(a, b) === 0 && a.frame === b.frame;
}

/** True when `caret` falls inside the half-open range `[from, to)`. */
function withinRun(placed: PlacedRun, caret: Caret): boolean {
  const { run, source } = placed;
  if (ownerKey(source) !== ownerKey(caret)) return false;
  if (trailKey(source.cells) !== trailKey(caret.cells)) return false;
  if ((source.block ?? 0) !== caret.block || (source.inline ?? 0) !== caret.inline) return false;
  const base = source.offset ?? 0;
  return caret.offset >= base && caret.offset <= base + utf8Length(run.text);
}

/** Screen geometry for a caret, in page coordinates. */
export function caretGeometry(
  list: DisplayList,
  page: DisplayPage,
  caret: Caret,
): { x: number; top: number; height: number } | null {
  const runs = collectRuns(page).filter((placed) => withinRun(placed, caret));
  if (runs.length === 0) return null;

  // Prefer the run painted by the frame the caret belongs to; a threaded story
  // paints the same offsets in more than one frame.
  const preferred = runs.find((placed) => placed.source.frame === caret.frame) ?? runs[0]!;
  const { run, source } = preferred;
  const local = caret.offset - (source.offset ?? 0);

  const glyph = run.glyphs.find((candidate) => candidate.cluster === local);
  const x = glyph ? run.x + glyph.x : run.x + run.width;
  const { ascent, descent } = metricsOf(list, run);

  return { x, top: run.y - ascent, height: ascent + descent };
}

/** Rectangles covering everything between two carets, for the selection wash. */
export function rangeRects(
  list: DisplayList,
  page: DisplayPage,
  from: Caret,
  to: Caret,
): Rect[] {
  const [start, end] = compareCarets(from, to) <= 0 ? [from, to] : [to, from];
  const rects: Rect[] = [];

  for (const placed of collectRuns(page)) {
    const { run, source } = placed;
    if (ownerKey(source) !== ownerKey(start)) continue;
    if (source.frame !== start.frame && source.frame !== end.frame && !start.story) continue;

    const base = source.offset ?? 0;
    const runStart: Caret = {
      frame: source.frame,
      story: source.story ?? null,
      cells: source.cells ?? [],
      block: source.block ?? 0,
      inline: source.inline ?? 0,
      offset: base,
    };
    const runEnd: Caret = { ...runStart, offset: base + utf8Length(run.text) };

    if (compareCarets(runEnd, start) <= 0 || compareCarets(runStart, end) >= 0) continue;

    const from = Math.max(start.offset, base) - base;
    const to = Math.min(end.offset, base + utf8Length(run.text)) - base;

    const left = offsetToX(run, from);
    const right = offsetToX(run, to);
    const { ascent, descent } = metricsOf(list, run);

    if (right > left) {
      rects.push({ x: run.x + left, y: run.y - ascent, w: right - left, h: ascent + descent });
    }
  }

  return rects;
}

function offsetToX(run: GlyphRun, local: number): number {
  const glyph = run.glyphs.find((candidate) => candidate.cluster === local);
  if (glyph) return glyph.x;
  return local <= 0 ? 0 : run.width;
}

/**
 * The caret one line up or down from `caret`, keeping the horizontal position.
 *
 * Works off painted baselines rather than the source, which is the only way to
 * get "up" right when a paragraph wraps.
 */
export function verticalNeighbour(
  list: DisplayList,
  page: DisplayPage,
  caret: Caret,
  direction: -1 | 1,
  desiredX: number,
): Caret | null {
  const geometry = caretGeometry(list, page, caret);
  if (!geometry) return null;

  const runs = collectRuns(page).filter((placed) => placed.source.frame === caret.frame);
  const baselines = [...new Set(runs.map((placed) => placed.run.y))].sort((a, b) => a - b);
  const current = geometry.top + geometry.height / 2;

  const target =
    direction < 0
      ? [...baselines].reverse().find((y) => y < current)
      : baselines.find((y) => y > current + 0.01);

  if (target === undefined) return null;

  const onTarget = runs.filter((placed) => Math.abs(placed.run.y - target) < 0.01);
  if (onTarget.length === 0) return null;

  let best = onTarget[0]!;
  let bestDistance = Infinity;
  for (const placed of onTarget) {
    const { run } = placed;
    const distance =
      desiredX < run.x
        ? run.x - desiredX
        : desiredX > run.x + run.width
          ? desiredX - (run.x + run.width)
          : 0;
    if (distance < bestDistance) {
      bestDistance = distance;
      best = placed;
    }
  }

  return caretInRun(best, desiredX);
}
