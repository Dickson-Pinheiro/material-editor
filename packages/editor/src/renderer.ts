/**
 * The painter.
 *
 * Walks the engine's display list and draws it on a canvas. Glyphs are filled
 * from their real outlines, at the coordinates the engine computed — the same
 * numbers the PDF emitter writes. That is the whole reason the preview and the
 * export cannot disagree: this file makes no layout decisions.
 */

import type { Engine } from "./engine";
import type { DisplayItem, DisplayList, DisplayPage, GlyphRun, Rect } from "./types";

export interface View {
  zoom: number;
  panX: number;
  panY: number;
}

export interface PagePlacement {
  page: DisplayPage;
  /** Top-left of the page in world coordinates. */
  x: number;
  y: number;
}

export interface Overlay {
  selected: Set<string>;
  hovered: string | null;
  editing: string | null;
  caret: { page: number; x: number; top: number; height: number } | null;
  caretVisible: boolean;
  highlights: { page: number; rect: Rect }[];
  /** Alignment guides shown while dragging, in page coordinates. */
  guides: { page: number; x?: number; y?: number }[];
  /** Rubber-band rectangle in world coordinates. */
  marquee: Rect | null;
  /**
   * Wrap silhouettes to show, keyed by frame id, in `0..1` of that frame's
   * own rect.
   *
   * Authoring chrome, like the resize handles: drawn only while the frame is
   * selected, and never part of the document. The engine paints nothing for a
   * wrap — its only effect is where the glyphs land — so without this the
   * author would have to infer the shape from the text that avoided it.
   */
  contours: Map<string, [number, number][]>;
}

/**
 * A world point in one page's own coordinates.
 *
 * The page is named, not looked up by what the point falls inside. A drag has
 * to keep measuring against the page it began on: page-local coordinates
 * restart at zero on every sheet, so a pointer that wanders onto the next page
 * would otherwise report a position near the top of *that* page and the drag
 * would jump.
 */
export function pointIn(
  placements: PagePlacement[],
  page: number,
  worldX: number,
  worldY: number,
): { x: number; y: number } | null {
  const placement = placements.find((candidate) => candidate.page.index === page);
  if (!placement) return null;
  return { x: worldX - placement.x, y: worldY - placement.y };
}

/** Vertical gap between pages, in points. */
export const PAGE_GAP = 28;
/** Side of a resize handle, in screen pixels. */
export const HANDLE_SIZE = 8;

export type HandleName = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

/** Stack the pages vertically, centred on the widest one. */
export function placePages(list: DisplayList): PagePlacement[] {
  const widest = list.pages.reduce((max, page) => Math.max(max, page.width), 0);
  let y = 0;
  return list.pages.map((page) => {
    const placement: PagePlacement = { page, x: (widest - page.width) / 2, y };
    y += page.height + PAGE_GAP;
    return placement;
  });
}

/** Total world-space extent of the document, for zoom-to-fit. */
export function documentExtent(list: DisplayList): { width: number; height: number } {
  const placements = placePages(list);
  const last = placements[placements.length - 1];
  return {
    width: list.pages.reduce((max, page) => Math.max(max, page.width), 0),
    height: last ? last.y + last.page.height : 0,
  };
}

/** Handle positions for a frame rectangle, in page coordinates. */
export function handlePositions(rect: Rect): Record<HandleName, [number, number]> {
  const { x, y, w, h } = rect;
  return {
    nw: [x, y],
    n: [x + w / 2, y],
    ne: [x + w, y],
    e: [x + w, y + h / 2],
    se: [x + w, y + h],
    s: [x + w / 2, y + h],
    sw: [x, y + h],
    w: [x, y + h / 2],
  };
}

export class Renderer {
  private readonly ctx: CanvasRenderingContext2D;
  private readonly images = new Map<string, CanvasImageSource>();

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly engine: Engine,
  ) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas 2d indisponível");
    this.ctx = ctx;
  }

  /** Supply the pixels for an image key the document references. */
  setImage(key: string, source: CanvasImageSource): void {
    this.images.set(key, source);
  }

  /** Resize the backing store to the element's size and pixel ratio. */
  resize(): { width: number; height: number } {
    const ratio = window.devicePixelRatio || 1;
    const width = this.canvas.clientWidth;
    const height = this.canvas.clientHeight;
    this.canvas.width = Math.max(1, Math.round(width * ratio));
    this.canvas.height = Math.max(1, Math.round(height * ratio));
    return { width, height };
  }

  render(list: DisplayList, view: View, overlay: Overlay): void {
    const { ctx } = this;
    const ratio = window.devicePixelRatio || 1;

    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, this.canvas.clientWidth, this.canvas.clientHeight);
    ctx.fillStyle = "#eceff3";
    ctx.fillRect(0, 0, this.canvas.clientWidth, this.canvas.clientHeight);

    ctx.save();
    ctx.translate(view.panX, view.panY);
    ctx.scale(view.zoom, view.zoom);

    // Two passes. Every sheet of paper is laid down before any content, so a
    // frame that has been dragged over a neighbouring page is never buried by
    // that page's background. Pages are siblings; content sits above all of
    // them.
    const placements = placePages(list);

    for (const placement of placements) {
      ctx.save();
      ctx.translate(placement.x, placement.y);
      this.drawPaper(placement.page, view);
      ctx.restore();
    }

    for (const placement of placements) {
      ctx.save();
      ctx.translate(placement.x, placement.y);
      this.drawPage(placement.page, view, overlay);
      ctx.restore();
    }

    if (overlay.marquee) {
      const { x, y, w, h } = overlay.marquee;
      ctx.strokeStyle = "#2f6fd0";
      ctx.fillStyle = "rgba(47, 111, 208, 0.12)";
      ctx.lineWidth = 1 / view.zoom;
      ctx.fillRect(x, y, w, h);
      ctx.strokeRect(x, y, w, h);
    }

    ctx.restore();
  }

  /** The sheet itself, plus its margin guides. */
  private drawPaper(page: DisplayPage, view: View): void {
    const { ctx } = this;

    ctx.save();
    ctx.shadowColor = "rgba(15, 23, 42, 0.18)";
    ctx.shadowBlur = 12 / view.zoom;
    ctx.shadowOffsetY = 3 / view.zoom;
    ctx.fillStyle = page.background ?? "#ffffff";
    ctx.fillRect(0, 0, page.width, page.height);
    ctx.restore();

    const margin = page.marginBox;
    ctx.save();
    ctx.strokeStyle = "rgba(13, 153, 255, 0.3)";
    ctx.lineWidth = 1 / view.zoom;
    ctx.setLineDash([4 / view.zoom, 4 / view.zoom]);
    ctx.strokeRect(margin.x, margin.y, margin.w, margin.h);
    ctx.restore();
  }

  private drawPage(page: DisplayPage, view: View, overlay: Overlay): void {
    const { ctx } = this;

    this.drawItems(page.items, view);

    // Text selection wash, over the glyphs so it survives frame fills.
    const highlights = overlay.highlights.filter((h) => h.page === page.index);
    if (highlights.length > 0) {
      ctx.save();
      ctx.fillStyle = "rgba(47, 111, 208, 0.28)";
      for (const { rect } of highlights) ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
      ctx.restore();
    }

    this.drawFrameChrome(page, view, overlay);

    // Alignment guides while dragging.
    const guides = overlay.guides.filter((guide) => guide.page === page.index);
    if (guides.length > 0) {
      ctx.save();
      ctx.strokeStyle = "#e0457b";
      ctx.lineWidth = 1 / view.zoom;
      for (const guide of guides) {
        ctx.beginPath();
        if (guide.x !== undefined) {
          ctx.moveTo(guide.x, 0);
          ctx.lineTo(guide.x, page.height);
        } else if (guide.y !== undefined) {
          ctx.moveTo(0, guide.y);
          ctx.lineTo(page.width, guide.y);
        }
        ctx.stroke();
      }
      ctx.restore();
    }

    // Caret.
    if (overlay.caret && overlay.caret.page === page.index && overlay.caretVisible) {
      const caret = overlay.caret;
      ctx.save();
      ctx.strokeStyle = "#111827";
      ctx.lineWidth = Math.max(1 / view.zoom, 1);
      ctx.beginPath();
      ctx.moveTo(caret.x, caret.top);
      ctx.lineTo(caret.x, caret.top + caret.height);
      ctx.stroke();
      ctx.restore();
    }
  }

  // ── Content ───────────────────────────────────────────────────────────────

  private drawItems(items: DisplayItem[], view: View): void {
    for (const item of items) {
      switch (item.type) {
        case "group":
          this.drawGroup(item, view);
          break;
        case "glyphs":
          this.drawGlyphs(item);
          break;
        case "rect":
          this.drawRect(item);
          break;
        case "ellipse":
          this.drawEllipse(item);
          break;
        case "line":
          this.drawLine(item);
          break;
        case "image":
          this.drawImage(item);
          break;
      }
    }
  }

  private drawGroup(group: Extract<DisplayItem, { type: "group" }>, view: View): void {
    const { ctx } = this;
    ctx.save();

    if (group.transform) {
      const [a, b, c, d, e, f] = group.transform;
      ctx.transform(a, b, c, d, e, f);
    }
    if (group.clip) {
      const path = new Path2D();
      const { rect, radius } = group.clip;
      if (radius > 0) path.roundRect(rect.x, rect.y, rect.w, rect.h, radius);
      else path.rect(rect.x, rect.y, rect.w, rect.h);
      ctx.clip(path);
    }
    if (group.opacity < 1) ctx.globalAlpha *= group.opacity;

    this.drawItems(group.items, view);
    ctx.restore();
  }

  /**
   * Compose a run into a single path, then fill once.
   *
   * Each glyph outline arrives in em units on the baseline; the matrix moves it
   * to its position and scales it to the font size — exactly the transform the
   * PDF emitter encodes in its text matrix.
   */
  private drawGlyphs(run: GlyphRun): void {
    const composed = new Path2D();
    let painted = false;

    for (const glyph of run.glyphs) {
      const outline = this.engine.glyph(run.font, glyph.id);
      if (!outline) continue;
      const matrix = new DOMMatrix()
        .translate(run.x + glyph.x, run.y + glyph.y)
        .scale(run.size, run.size);
      composed.addPath(outline, matrix);
      painted = true;
    }

    if (!painted) return;
    this.ctx.fillStyle = run.fill;
    this.ctx.fill(composed);
  }

  private drawRect(item: Extract<DisplayItem, { type: "rect" }>): void {
    const { ctx } = this;
    const { rect, radius } = item;
    const path = new Path2D();
    if (radius > 0) path.roundRect(rect.x, rect.y, rect.w, rect.h, radius);
    else path.rect(rect.x, rect.y, rect.w, rect.h);

    if (item.fill) {
      ctx.fillStyle = item.fill;
      ctx.fill(path);
    }
    if (item.stroke) {
      this.applyStroke(item.stroke);
      ctx.stroke(path);
      ctx.setLineDash([]);
    }
  }

  private drawEllipse(item: Extract<DisplayItem, { type: "ellipse" }>): void {
    const { ctx } = this;
    const { rect } = item;
    const path = new Path2D();
    path.ellipse(rect.x + rect.w / 2, rect.y + rect.h / 2, rect.w / 2, rect.h / 2, 0, 0, Math.PI * 2);

    if (item.fill) {
      ctx.fillStyle = item.fill;
      ctx.fill(path);
    }
    if (item.stroke) {
      this.applyStroke(item.stroke);
      ctx.stroke(path);
      ctx.setLineDash([]);
    }
  }

  private drawLine(item: Extract<DisplayItem, { type: "line" }>): void {
    const { ctx } = this;
    this.applyStroke(item.stroke);
    ctx.beginPath();
    ctx.moveTo(item.x1, item.y1);
    ctx.lineTo(item.x2, item.y2);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  private drawImage(item: Extract<DisplayItem, { type: "image" }>): void {
    const source = this.images.get(item.src);
    const { rect } = item;

    if (!source) {
      // Placeholder, so a missing asset is visible instead of invisible.
      const { ctx } = this;
      ctx.save();
      ctx.fillStyle = "#f1f3f5";
      ctx.strokeStyle = "#adb5bd";
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
      ctx.strokeRect(rect.x, rect.y, rect.w, rect.h);
      ctx.restore();
      return;
    }

    this.ctx.drawImage(source, rect.x, rect.y, rect.w, rect.h);
  }

  private applyStroke(stroke: { color: string; width: number; dash?: [number, number] | null }): void {
    this.ctx.strokeStyle = stroke.color;
    this.ctx.lineWidth = stroke.width;
    this.ctx.setLineDash(stroke.dash ?? []);
  }

  // ── Chrome ────────────────────────────────────────────────────────────────

  private drawFrameChrome(page: DisplayPage, view: View, overlay: Overlay): void {
    const { ctx } = this;
    const hairline = 1 / view.zoom;

    for (const frame of page.frames) {
      const selected = overlay.selected.has(frame.id);
      const hovered = overlay.hovered === frame.id;
      const editing = overlay.editing === frame.id;

      if (frame.overset) {
        // InDesign's overset marker: content that had nowhere to go.
        ctx.save();
        ctx.strokeStyle = "#e0457b";
        ctx.lineWidth = hairline * 2;
        ctx.strokeRect(frame.rect.x, frame.rect.y, frame.rect.w, frame.rect.h);
        ctx.fillStyle = "#e0457b";
        const size = 10 / view.zoom;
        ctx.fillRect(frame.rect.x + frame.rect.w - size, frame.rect.y + frame.rect.h, size, size);
        ctx.restore();
      }

      if (!selected && !hovered && !editing) continue;

      ctx.save();
      if (frame.rotation) {
        const cx = frame.rect.x + frame.rect.w / 2;
        const cy = frame.rect.y + frame.rect.h / 2;
        ctx.translate(cx, cy);
        ctx.rotate((frame.rotation * Math.PI) / 180);
        ctx.translate(-cx, -cy);
      }

      ctx.strokeStyle = editing ? "#0f9d58" : selected ? "#2f6fd0" : "rgba(47, 111, 208, 0.5)";
      ctx.lineWidth = (selected || editing ? 1.5 : 1) * hairline;
      ctx.setLineDash(editing ? [5 * hairline, 3 * hairline] : []);
      ctx.strokeRect(frame.rect.x, frame.rect.y, frame.rect.w, frame.rect.h);
      ctx.setLineDash([]);

      const ring = selected ? overlay.contours.get(frame.id) : undefined;
      if (ring && ring.length >= 3) {
        ctx.strokeStyle = "#a24cd6";
        ctx.lineWidth = 1.5 * hairline;
        ctx.setLineDash([4 * hairline, 3 * hairline]);
        ctx.beginPath();
        ring.forEach(([nx, ny], index) => {
          const x = frame.rect.x + nx * frame.rect.w;
          const y = frame.rect.y + ny * frame.rect.h;
          if (index === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        });
        ctx.closePath();
        ctx.stroke();
        ctx.setLineDash([]);
      }

      if (selected && !editing && !frame.locked) {
        const size = HANDLE_SIZE / view.zoom;
        ctx.fillStyle = "#ffffff";
        ctx.strokeStyle = "#2f6fd0";
        ctx.lineWidth = hairline;
        for (const [x, y] of Object.values(handlePositions(frame.rect))) {
          ctx.fillRect(x - size / 2, y - size / 2, size, size);
          ctx.strokeRect(x - size / 2, y - size / 2, size, size);
        }
      }

      ctx.restore();
    }
  }
}
