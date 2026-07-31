/**
 * TypeScript mirror of the engine's two JSON shapes.
 *
 * `Document` is what you author and what the editor mutates.
 * `DisplayList` is what the engine hands back — already positioned, already
 * shaped. The editor never recomputes any of it; it paints it and maps clicks
 * back through `SourceRef`.
 */

// ─────────────────────────────────────────────────────────────────────────────
// Document
// ─────────────────────────────────────────────────────────────────────────────

/** Points, or a string with a unit: `"10mm"`, `"1cm"`, `"12pt"`, `"16px"`. */
export type Len = number | string;

/** `[x, y, w, h]` in points, from the top-left of the page. */
export type RectTuple = [Len, Len, Len, Len];

export type TextAlign = "left" | "center" | "right" | "justify";
export type VerticalAlign = "top" | "middle" | "bottom" | "justify";
export type Overflow = "clip" | "visible" | "grow";
export type ImageFit = "contain" | "cover" | "stretch" | "none";
export type ShapeKind = "rect" | "ellipse" | "line";

export interface Style {
  extends?: string;
  fontFamily?: string;
  fontSize?: Len;
  fontWeight?: number | string;
  fontStyle?: "normal" | "italic" | "oblique";
  color?: string;
  background?: string;
  /** A number multiplies the font size; a string is an absolute length. */
  lineHeight?: number | string;
  letterSpacing?: Len;
  wordSpacing?: Len;
  textAlign?: TextAlign;
  textTransform?: "none" | "uppercase" | "lowercase" | "capitalize";
  underline?: boolean;
  strikethrough?: boolean;
  spaceBefore?: Len;
  spaceAfter?: Len;
  indentFirst?: Len;
  indentLeft?: Len;
  indentRight?: Len;
  baselineShift?: Len;
  fontScale?: number;
  keepWithNext?: boolean;
}

export interface TextRun {
  type: "text";
  text: string;
  use?: string;
  style?: Style;
  id?: string;
}

export type Inline =
  | TextRun
  | { type: "break" }
  | { type: "tab"; to?: Len; leader?: string }
  | { type: "space"; width: Len }
  | { type: "image"; src: string; width?: Len; height?: Len; baseline?: Len }
  | { type: "rule"; width?: Len; thickness?: Len; color?: string; offset?: Len };

export interface Marker {
  text: string;
  style?: Style;
  gap?: Len;
  width?: Len;
  hanging?: boolean;
}

export interface Paragraph {
  type: "paragraph";
  id?: string;
  use?: string;
  style?: Style;
  marker?: Marker;
  content: Inline[];
}

export type Block =
  | Paragraph
  | { type: "rule"; thickness?: Len; color?: string; width?: number; style?: Style }
  | { type: "spacer"; height: Len }
  | { type: "frameBreak" }
  | { type: "columnBreak" }
  | { type: "pageBreak" };

export interface Border {
  width?: Len;
  color?: string;
  style?: "solid" | "dashed" | "dotted";
  sides?: { top?: boolean; right?: boolean; bottom?: boolean; left?: boolean };
}

interface FrameBase {
  id?: string;
  name?: string;
  rect: RectTuple;
  rotation?: number;
  opacity?: number;
  padding?: Len | Len[];
  fill?: string;
  border?: Border;
  radius?: number;
  clip?: boolean;
  visible?: boolean;
  locked?: boolean;
}

export interface TextFrame extends FrameBase {
  type: "text";
  blocks?: Block[];
  story?: string;
  threadNext?: string;
  /** Keep generating pages, modelled on this one, until the content runs out. */
  autoFlow?: boolean;
  columns?: number;
  columnGap?: Len;
  verticalAlign?: VerticalAlign;
  overflow?: Overflow;
  /** Lay out as if nothing on the page carried a wrap. */
  ignoreWrap?: boolean;
  use?: string;
  style?: Style;
}

export interface ImageFrame extends FrameBase {
  type: "image";
  src: string;
  fit?: ImageFit;
  align?: string;
  /** How this frame pushes text aside. Absent means it does not. */
  wrap?: Wrap | null;
}

/**
 * Text wrap: the space a frame denies to the text around it.
 *
 * The silhouette lives in the document, not in the image's pixels. Tracing it
 * is authoring work and belongs here in the editor — the engine only ever
 * reads the ring back, which is what keeps a document laying out the same way
 * on every machine.
 */
export interface Wrap {
  mode: WrapMode;
  /** Clearance in points between the text and the shape. */
  padding?: Len | Len[];
}

export type WrapMode =
  | { kind: "box" }
  /** A closed ring in `0..1` coordinates relative to the frame's rect. */
  | { kind: "contour"; points: [number, number][] };

export interface ShapeFrame extends FrameBase {
  type: "shape";
  shape?: ShapeKind;
}

export interface GroupFrame extends FrameBase {
  type: "group";
  children: Frame[];
}

export type Frame = TextFrame | ImageFrame | ShapeFrame | GroupFrame;

export interface Page {
  id?: string;
  name?: string;
  size?: string | [Len, Len];
  margins?: Len | Len[];
  master?: string;
  background?: string;
  style?: Style;
  frames: Frame[];
}

export interface Master {
  size?: string | [Len, Len];
  margins?: Len | Len[];
  background?: string;
  frames?: Frame[];
}

export interface Resources {
  styles?: Record<string, Style>;
  masters?: Record<string, Master>;
  stories?: Record<string, Block[]>;
  colors?: Record<string, string>;
}

export interface Meta {
  title?: string;
  author?: string;
  subject?: string;
  keywords?: string[];
  language?: string;
}

export interface DocumentSpec {
  version?: number;
  meta?: Meta;
  page?: { size?: string | [Len, Len]; margins?: Len | Len[]; bleed?: number; facing?: boolean };
  style?: Style;
  resources?: Resources;
  pages: Page[];
}

// ─────────────────────────────────────────────────────────────────────────────
// Display list
// ─────────────────────────────────────────────────────────────────────────────

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Where a painted thing came from.
 *
 * The indices address the content's *origin*, so text that overflowed into
 * another frame still points at the block and inline that own it.
 */
export interface SourceRef {
  page: number;
  frame: string;
  /** Set when the content lives in `resources.stories`. */
  story?: string | null;
  block?: number | null;
  inline?: number | null;
  /** Byte offset into that inline's text where the run starts. */
  offset?: number | null;
}

export interface Glyph {
  id: number;
  /** Offset from the run origin along the baseline, in points. */
  x: number;
  y: number;
  advance: number;
  /** Byte offset into the run's `text`. */
  cluster: number;
}

export interface GlyphRun {
  type: "glyphs";
  font: number;
  size: number;
  fill: string;
  /** Origin of the baseline. */
  x: number;
  y: number;
  width: number;
  glyphs: Glyph[];
  text: string;
  source?: SourceRef | null;
}

export interface Stroke {
  color: string;
  width: number;
  dash?: [number, number] | null;
}

export interface RectItem {
  type: "rect";
  rect: Rect;
  radius: number;
  fill?: string | null;
  stroke?: Stroke | null;
  source?: SourceRef | null;
}

export interface EllipseItem {
  type: "ellipse";
  rect: Rect;
  fill?: string | null;
  stroke?: Stroke | null;
  source?: SourceRef | null;
}

export interface LineItem {
  type: "line";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  stroke: Stroke;
  source?: SourceRef | null;
}

export interface ImageItem {
  type: "image";
  src: string;
  rect: Rect;
  crop?: Rect | null;
  source?: SourceRef | null;
}

export interface GroupItem {
  type: "group";
  source?: SourceRef | null;
  /** Affine `[a, b, c, d, e, f]`, applied before painting the children. */
  transform?: [number, number, number, number, number, number] | null;
  clip?: { rect: Rect; radius: number } | null;
  opacity: number;
  items: DisplayItem[];
}

export type DisplayItem =
  | GroupItem
  | GlyphRun
  | RectItem
  | EllipseItem
  | LineItem
  | ImageItem;

export interface DisplayFrame {
  id: string;
  name?: string | null;
  rect: Rect;
  rotation: number;
  kind: "text" | "image" | "shape" | "group";
  locked: boolean;
  /** Content that did not fit and had nowhere to flow. */
  overset: boolean;
  ancestors: string[];
}

export interface DisplayFont {
  id: number;
  family: string;
  weight: number;
  italic: boolean;
  postScriptName: string;
  unitsPerEm: number;
  /** Em-relative, positive. */
  ascender: number;
  /** Em-relative, negative. */
  descender: number;
}

export interface DisplayPage {
  index: number;
  id?: string | null;
  width: number;
  height: number;
  background?: string | null;
  marginBox: Rect;
  frames: DisplayFrame[];
  items: DisplayItem[];
}

export interface Diagnostic {
  severity: "info" | "warning" | "error";
  code: string;
  message: string;
  page?: number | null;
  frame?: string | null;
}

export interface DisplayList {
  version: number;
  fonts: DisplayFont[];
  pages: DisplayPage[];
  diagnostics: Diagnostic[];
}

// ─────────────────────────────────────────────────────────────────────────────
// Editor-side selection
// ─────────────────────────────────────────────────────────────────────────────

/** A position inside the text of a document, in source coordinates. */
export interface Caret {
  /** Frame the caret was placed through — used to scope the search. */
  frame: string;
  story: string | null;
  block: number;
  inline: number;
  /** Byte offset into that inline's text. */
  offset: number;
}

export interface TextSelection {
  anchor: Caret;
  focus: Caret;
}
