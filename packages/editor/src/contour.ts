/**
 * Tracing a picture's silhouette.
 *
 * The engine never looks at a pixel. It reads a ring out of the document and
 * lays text around it, which is what keeps a document laying out the same way
 * on every machine and in both wasm targets. Turning pixels into that ring is
 * authoring work, and authoring work belongs here, where a person can see the
 * result and correct it.
 *
 * The method is the one Cheng Lou's `pretext` uses in its wrap demo: read the
 * alpha channel row by row, take the first and last opaque pixel of each row,
 * smooth the two edges, and sample them into a closed ring.
 */

import type { ImageFit, ImageFrame, Len } from "./types";
import { parseLen } from "./store";

/** Points per CSS pixel, matching the engine's `PT_PER_PX`. */
const PT_PER_PX = 72 / 96;

/** Longest side the silhouette is read at. Finer than a point is noise. */
const SAMPLE_SIZE = 320;

/** Below this the pixel counts as see-through. */
const ALPHA_FLOOR = 12;

/** Vertices per edge. Two edges make the ring, so roughly twice this. */
const ROWS = 52;

export type Point = [number, number];

export interface TraceOptions {
  /** Rows averaged either side of each sample. 0 traces every wobble. */
  smooth?: number;
  /**
   * `mean` follows the silhouette; `envelope` takes the widest pixel in the
   * window, so the ring never cuts into the picture.
   */
  mode?: "mean" | "envelope";
}

export interface Trace {
  /** Closed ring in `0..1` of the image's own box. */
  points: Point[];
  /**
   * Every row reached both edges of the image: there is no silhouette to
   * speak of, only a rectangle. Worth telling the author rather than handing
   * back a ring that pretends to be a shape.
   */
  opaque: boolean;
}

/**
 * Read a silhouette out of an image's alpha channel.
 *
 * Returns `null` when the image is entirely see-through — there is nothing to
 * wrap around, and an empty ring would be a lie.
 */
export function trace(source: ImageBitmap, options: TraceOptions = {}): Trace | null {
  const smooth = Math.max(0, Math.floor(options.smooth ?? 2));
  const mode = options.mode ?? "envelope";

  const aspect = source.width / source.height;
  const width = Math.max(8, Math.round(aspect >= 1 ? SAMPLE_SIZE : SAMPLE_SIZE * aspect));
  const height = Math.max(8, Math.round(aspect >= 1 ? SAMPLE_SIZE / aspect : SAMPLE_SIZE));

  const canvas = new OffscreenCanvas(width, height);
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (context === null) return null;
  context.clearRect(0, 0, width, height);
  context.drawImage(source, 0, 0, width, height);
  const { data } = context.getImageData(0, 0, width, height);

  // ── Edges, row by row ─────────────────────────────────────────────────────
  const lefts: (number | null)[] = new Array(height).fill(null);
  const rights: (number | null)[] = new Array(height).fill(null);
  let opaque = true;

  for (let y = 0; y < height; y += 1) {
    let left = -1;
    let right = -1;
    for (let x = 0; x < width; x += 1) {
      if (data[(y * width + x) * 4 + 3]! < ALPHA_FLOOR) continue;
      if (left === -1) left = x;
      right = x;
    }
    if (left === -1) {
      opaque = false;
      continue;
    }
    lefts[y] = left;
    rights[y] = right + 1;
    if (left > 0 || right + 1 < width) opaque = false;
  }

  const rows: number[] = [];
  for (let y = 0; y < height; y += 1) {
    if (lefts[y] !== null) rows.push(y);
  }
  if (rows.length === 0) return null;

  // ── Smooth ────────────────────────────────────────────────────────────────
  const smoothLeft = new Array(height).fill(0);
  const smoothRight = new Array(height).fill(0);

  for (const y of rows) {
    let sumLeft = 0;
    let sumRight = 0;
    let count = 0;
    let edgeLeft = Infinity;
    let edgeRight = -Infinity;

    for (let offset = -smooth; offset <= smooth; offset += 1) {
      const left = lefts[y + offset];
      const right = rights[y + offset];
      if (left == null || right == null) continue;
      sumLeft += left;
      sumRight += right;
      edgeLeft = Math.min(edgeLeft, left);
      edgeRight = Math.max(edgeRight, right);
      count += 1;
    }

    if (count === 0) {
      smoothLeft[y] = lefts[y]!;
      smoothRight[y] = rights[y]!;
    } else if (mode === "envelope") {
      smoothLeft[y] = edgeLeft;
      smoothRight[y] = edgeRight;
    } else {
      smoothLeft[y] = sumLeft / count;
      smoothRight[y] = sumRight / count;
    }
  }

  // ── Sample into a ring ────────────────────────────────────────────────────
  const step = Math.max(1, Math.floor(rows.length / ROWS));
  const sampled: number[] = [];
  for (let index = 0; index < rows.length; index += step) sampled.push(rows[index]!);
  if (sampled[sampled.length - 1] !== rows[rows.length - 1]) {
    sampled.push(rows[rows.length - 1]!);
  }

  // Normalised against the whole image, not against the opaque part: the ring
  // has to land where the picture's content actually sits inside its own box.
  const points: Point[] = [];
  for (const y of sampled) {
    points.push([smoothLeft[y] / width, (y + 0.5) / height]);
  }
  for (let index = sampled.length - 1; index >= 0; index -= 1) {
    const y = sampled[index]!;
    points.push([smoothRight[y] / width, (y + 0.5) / height]);
  }

  return { points, opaque };
}

/**
 * Where the picture actually sits inside its frame, in `0..1` of the frame.
 *
 * Mirrors `layout_image_frame`: the fit decides the size against the content
 * box — the frame less its padding — and the alignment places what is left
 * over. Without this the ring would be stretched to the frame and drift off
 * the picture on every fit but `stretch`.
 */
export function placement(
  frame: ImageFrame,
  natural: { width: number; height: number },
): { x: number; y: number; w: number; h: number } {
  const [, , frameW, frameH] = frame.rect.map((value) => parseLen(value as Len));
  const inset = insets(frame.padding);

  const boxW = Math.max(1, frameW! - inset.left - inset.right);
  const boxH = Math.max(1, frameH! - inset.top - inset.bottom);
  const naturalW = Math.max(1, natural.width * PT_PER_PX);
  const naturalH = Math.max(1, natural.height * PT_PER_PX);

  let w = boxW;
  let h = boxH;
  const fit: ImageFit = frame.fit ?? "contain";
  if (fit === "none") {
    w = naturalW;
    h = naturalH;
  } else if (fit === "contain" || fit === "cover") {
    const ratios = [boxW / naturalW, boxH / naturalH];
    const scale = fit === "contain" ? Math.min(...ratios) : Math.max(...ratios);
    w = naturalW * scale;
    h = naturalH * scale;
  }

  const [fx, fy] = alignFactors(frame.align);
  return {
    x: (inset.left + (boxW - w) * fx) / Math.max(1, frameW!),
    y: (inset.top + (boxH - h) * fy) / Math.max(1, frameH!),
    w: w / Math.max(1, frameW!),
    h: h / Math.max(1, frameH!),
  };
}

/** Move a ring from the image's own box into the frame's. */
export function toFrame(points: Point[], box: ReturnType<typeof placement>): Point[] {
  return points.map(([x, y]) => [box.x + x * box.w, box.y + y * box.h] as Point);
}

function insets(padding: Len | Len[] | undefined): {
  top: number;
  right: number;
  bottom: number;
  left: number;
} {
  const list = (Array.isArray(padding) ? padding : [padding ?? 0]).map((value) =>
    parseLen(value as Len),
  );
  const [a = 0, b = a, c = a, d = b] = list;
  return { top: a, right: b, bottom: c, left: d };
}

/** The engine's `ImageAlign::factors`, spelled out rather than inferred. */
const ALIGN: Record<string, [number, number]> = {
  topLeft: [0, 0],
  top: [0.5, 0],
  topRight: [1, 0],
  left: [0, 0.5],
  center: [0.5, 0.5],
  right: [1, 0.5],
  bottomLeft: [0, 1],
  bottom: [0.5, 1],
  bottomRight: [1, 1],
};

function alignFactors(align: string | undefined): [number, number] {
  return ALIGN[align ?? "center"] ?? ALIGN.center!;
}
