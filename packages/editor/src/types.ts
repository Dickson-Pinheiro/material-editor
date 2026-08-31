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

/**
 * A frame that flows.
 *
 * Fill, border, radius and inset on a block that stacks with the text and
 * breaks between pages — the callout, the activity box, the pulled quote. It
 * is what a one-cell table used to stand in for, before the engine grew a node
 * that could paint chrome *and* break.
 */
export interface PanelBlock {
  type: "panel";
  blocks: Block[];
  fill?: string | null;
  border?: Border | null;
  radius?: Radius;
  inset?: Len | Len[];
  use?: string;
  style?: Style;
  /** Where the author wrote this panel, kept across a page break. */
  origin?: number;
}

export type Block =
  | Paragraph
  | TableBlock
  | PanelBlock
  | { type: "rule"; thickness?: Len; color?: string; width?: number; style?: Style }
  | { type: "spacer"; height: Len }
  | { type: "frameBreak" }
  | { type: "columnBreak" }
  | { type: "pageBreak" };

// ─────────────────────────────────────────────────────────────────────────────
// Table
// ─────────────────────────────────────────────────────────────────────────────

/**
 * What a column or row asks for.
 *
 * `"auto"` takes what the content needs; a number or a length like `"20mm"` is
 * fixed; `"1fr"` takes a share of what is left; `"25%"` a share of the whole.
 */
export type TrackSize = number | string;

export type CellAlign = "top" | "middle" | "bottom" | "baseline";
export type GridAxis = "horizontal" | "vertical";

/** A rule drawn along a grid line, independent of any cell. */
export interface GridLine {
  axis?: GridAxis;
  /** Which boundary: 0 is before the first track, `n` after the last. */
  at?: number;
  from?: number;
  to?: number;
  width?: Len;
  color?: string;
}

/** Alternating row fills, so striping is not written into every row. */
export interface Stripe {
  every?: number;
  offset?: number;
  fill?: string;
}

/** Rows repeated when the table continues on another page. */
export interface RepeatRows {
  rows?: number;
  repeat?: boolean;
  /** Shown in place of `rows` on the pages the table is *continuing*. */
  continued?: Cell[] | null;
}

export interface Cell {
  /** Explicit column. Absent means the next free slot, filling row by row. */
  x?: number | null;
  y?: number | null;
  colspan?: number;
  rowspan?: number;
  /** A cell holds blocks, not a string — two paragraphs and a rule in one
   *  cell is ordinary in teaching material. */
  blocks?: Block[];
  verticalAlign?: CellAlign;
  fill?: string | null;
  inset?: Len | Len[];
  use?: string;
  style?: Style;
}

export interface TableBlock {
  type: "table";
  /** One entry per column. Empty means inferred from the widest row. */
  columns?: TrackSize[];
  rows?: TrackSize[];
  cells?: Cell[];
  header?: RepeatRows | null;
  footer?: RepeatRows | null;
  inset?: Len | Len[];
  columnGap?: Len;
  rowGap?: Len;
  lines?: GridLine[];
  stripe?: Stripe | null;
  fill?: string | null;
  use?: string;
  style?: Style;
}

export interface Border {
  width?: Len;
  color?: string;
  style?: "solid" | "dashed" | "dotted";
  sides?: { top?: boolean; right?: boolean; bottom?: boolean; left?: boolean };
}

/**
 * Corner radii, clockwise from the top-left.
 *
 * A single number rounds all four — the common case, and what every document
 * written before corners were separable says. The list forms follow CSS:
 * `[topLeft, topRight]` are the two diagonals, four are each corner in turn.
 */
export type Radius = Len | Len[];

interface FrameBase {
  id?: string;
  name?: string;
  rect: RectTuple;
  rotation?: number;
  opacity?: number;
  padding?: Len | Len[];
  fill?: string;
  border?: Border;
  radius?: Radius;
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

// ─────────────────────────────────────────────────────────────────────────────
// Chart
// ─────────────────────────────────────────────────────────────────────────────

/** One cell of data. `null` is a hole, kept rather than rejected. */
export type DataValue = number | string | null;
/** One observation: field name to value. */
export type DataRow = Record<string, DataValue>;

export type Mark = "bar" | "line" | "area" | "point";
/** What a field means, which is what decides its scale. Absent means "read it
 *  off the data", which is only asked where the answer is not in doubt. */
export type FieldKind = "quantitative" | "categorical";
export type ScaleKind = "linear" | "log" | "band" | "point";
export type LegendPosition = "right" | "bottom" | "top" | "left";

/** The author overriding what the field's kind would have chosen. */
export interface ScaleSpec {
  kind?: ScaleKind;
  domain?: [number, number];
  categories?: string[];
  /** Whether the scale must reach zero. Absent means the mark decides, and a
   *  bar decides yes — on the axis that carries the quantity. */
  zero?: boolean;
  nice?: boolean;
  base?: number;
  paddingInner?: number;
  paddingOuter?: number;
}

export interface Channel {
  field?: string;
  kind?: FieldKind;
  scale?: ScaleSpec;
  /** Shown beside the axis. Absent means the field's own name. */
  title?: string;
}

export interface Encoding {
  x?: Channel;
  y?: Channel;
  /** Splits the data into series, one colour each. */
  color?: Channel | null;
}

export interface Axis {
  visible?: boolean;
  title?: string;
  /** Roughly how many marks to aim for. */
  ticks?: number;
  grid?: boolean;
}

export interface Axes {
  x?: Axis;
  y?: Axis;
}

export interface Legend {
  visible?: boolean;
  position?: LegendPosition;
  title?: string;
}

export interface ChartFrame extends FrameBase {
  type: "chart";
  /** Inline observations. Ignored when `dataset` is set. */
  data?: DataRow[];
  /** Pull the rows from `resources.data` instead — the same pair `blocks` and
   *  `story` already are on a text frame. */
  dataset?: string;
  mark?: Mark;
  encoding?: Encoding;
  axes?: Axes;
  legend?: Legend | null;
  /** Colours for the series, in order. Absent means the built-in palette. */
  palette?: string[];
  style?: Style;
}

export type Frame = TextFrame | ImageFrame | ShapeFrame | GroupFrame | ChartFrame;

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
  /** Named tables of observations, so two charts of the same numbers are not
   *  two copies of the numbers. */
  data?: Record<string, DataRow[]>;
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
  /**
   * The table cells this content is nested inside, outermost first.
   *
   * Absent for everything that is not in a table. `block`, `inline` and
   * `offset` are read relative to the innermost cell's own blocks — without
   * this trail they would read as indices into the frame, and a click on a
   * cell would edit whatever paragraph happened to sit at that index.
   */
  cells?: CellStep[];
  block?: number | null;
  inline?: number | null;
  /** Byte offset into that inline's text where the run starts. */
  offset?: number | null;
}

/** One step down into a table. */
export interface CellStep {
  /** Index of the table block in the list that holds it. */
  block: number;
  /** Index of the cell in that table's `cells`, as the author wrote it — not
   *  as it was drawn, since a continuation renumbers its rows. */
  cell: number;
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
  /** Always points here — the engine resolves units before it emits. */
  radius: number | number[];
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

/**
 * An arbitrary filled or stroked outline.
 *
 * Structured commands rather than an SVG path string: the browser would parse
 * a string for free, but the PDF emitter would have to parse it back, and a
 * path parser is code you write once and debug for years.
 */
export interface PathItem {
  type: "path";
  commands: PathCommand[];
  fill?: string | null;
  stroke?: Stroke | null;
  fillRule?: "nonZero" | "evenOdd";
  source?: SourceRef | null;
}

export type PathCommand =
  | { op: "moveTo"; x: number; y: number }
  | { op: "lineTo"; x: number; y: number }
  /** Cubic Bézier. The only curve; every other reduces to it. */
  | { op: "curveTo"; x1: number; y1: number; x2: number; y2: number; x: number; y: number }
  | { op: "close" };

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
  clip?: { rect: Rect; radius: number | number[] } | null;
  opacity: number;
  items: DisplayItem[];
}

export type DisplayItem =
  | GroupItem
  | GlyphRun
  | RectItem
  | EllipseItem
  | LineItem
  | PathItem
  | ImageItem;

export interface DisplayFrame {
  id: string;
  name?: string | null;
  rect: Rect;
  rotation: number;
  kind: "text" | "image" | "shape" | "group" | "chart";
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
  /** The table cells the text is inside, outermost first. Empty for text that
   *  is not in a table, which is most of it. */
  cells: CellStep[];
  block: number;
  inline: number;
  /** Byte offset into that inline's text. */
  offset: number;
}

export interface TextSelection {
  anchor: Caret;
  focus: Caret;
}
