/**
 * Text editing: caret movement and mutations on the document JSON.
 *
 * # Offsets
 *
 * The engine reports offsets as **UTF-8 byte** positions, because that is what
 * shaping clusters are. JavaScript strings are UTF-16. Every edit therefore
 * converts at the boundary — get this wrong and a caret placed after "ç" lands
 * mid-character. The conversion lives in `utf8.ts`, which both this module and
 * the hit-testing code share.
 */

import type { Store } from "./store";
import type {
  Caret,
  DisplayList,
  DisplayPage,
  Inline,
  Paragraph,
  SourceRef,
  Style,
  TextRun,
} from "./types";
import { compareCarets, verticalNeighbour } from "./hit";
import {
  byteToIndex,
  indexToByte,
  nextBoundary,
  previousBoundary,
  utf8Length,
} from "./utf8";

type Direction = -1 | 1;

export class TextEditor {
  caret: Caret | null = null;
  /** Selection anchor; equal to the caret when nothing is selected. */
  anchor: Caret | null = null;
  /** Frame currently in text-edit mode. */
  frameId: string | null = null;
  /** Remembered column for up/down movement. */
  private desiredX = 0;

  constructor(private readonly store: Store) {}

  active(): boolean {
    return this.frameId !== null && this.caret !== null;
  }

  enter(frameId: string, caret: Caret): void {
    this.frameId = frameId;
    this.caret = caret;
    this.anchor = caret;
  }

  exit(): void {
    this.frameId = null;
    this.caret = null;
    this.anchor = null;
  }

  place(caret: Caret, extend = false): void {
    this.caret = caret;
    if (!extend || !this.anchor) this.anchor = caret;
  }

  hasSelection(): boolean {
    return this.caret !== null && this.anchor !== null && compareCarets(this.caret, this.anchor) !== 0;
  }

  /** Selection bounds in reading order. */
  range(): [Caret, Caret] | null {
    if (!this.hasSelection() || !this.caret || !this.anchor) return null;
    return compareCarets(this.anchor, this.caret) <= 0
      ? [this.anchor, this.caret]
      : [this.caret, this.anchor];
  }

  rememberColumn(x: number): void {
    this.desiredX = x;
  }

  // ── Mutations ─────────────────────────────────────────────────────────────

  /** Insert text at the caret, replacing the selection if there is one. */
  insert(text: string): void {
    if (!this.caret || text.length === 0) return;

    this.store.commit(() => {
      this.applyDeleteSelection();
      const caret = this.caret;
      if (!caret) return;

      const paragraph = this.paragraphAt(caret);
      if (!paragraph) return;

      let inline = paragraph.content[caret.inline];
      if (!inline || inline.type !== "text") {
        // An empty paragraph, or a caret next to a non-text inline: give it a
        // run to type into rather than refusing the keystroke.
        const run: TextRun = { type: "text", text: "" };
        paragraph.content.splice(caret.inline, 0, run);
        inline = run;
      }

      const run = inline as TextRun;
      const index = byteToIndex(run.text, caret.offset);
      run.text = run.text.slice(0, index) + text + run.text.slice(index);
      caret.offset += utf8Length(text);
      this.anchor = { ...caret };
    });
  }

  /** Split the paragraph at the caret — what Enter does. */
  splitParagraph(): void {
    if (!this.caret) return;
    this.store.commit(() => this.performSplit());
  }

  /**
   * Break the page at the caret.
   *
   * Splits the paragraph and drops a `pageBreak` between the halves, so the
   * rest of the story continues on a later page. With `autoFlow` on, that page
   * is created for it.
   */
  insertPageBreak(): void {
    if (!this.caret) return;

    this.store.commit(() => {
      const at = this.performSplit();
      if (at === null || !this.caret) return;

      const blocks = this.blocksAt(this.caret);
      if (!blocks) return;

      blocks.splice(at, 0, { type: "pageBreak" });
      this.caret = { ...this.caret, block: at + 1 };
      this.anchor = { ...this.caret };
    });
  }

  /**
   * Cut the paragraph in two at the caret. Must run inside a commit.
   *
   * Returns the index of the new second half, so callers can slip a block in
   * between — which is all a page break is.
   */
  private performSplit(): number | null {
    this.applyDeleteSelection();
    const caret = this.caret;
    if (!caret) return null;

    const blocks = this.blocksAt(caret);
    const paragraph = this.paragraphAt(caret);
    if (!blocks || !paragraph) return null;

    const tail: Inline[] = [];
    const inline = paragraph.content[caret.inline];

    if (inline && inline.type === "text") {
      const index = byteToIndex(inline.text, caret.offset);
      const rest = inline.text.slice(index);
      inline.text = inline.text.slice(0, index);
      if (rest.length > 0) tail.push({ ...inline, text: rest });
    }

    tail.push(...paragraph.content.slice(caret.inline + 1));
    paragraph.content.length = caret.inline + 1;

    const next: Paragraph = {
      type: "paragraph",
      style: paragraph.style,
      use: paragraph.use,
      content: tail.length > 0 ? tail : [{ type: "text", text: "" }],
    };

    const at = caret.block + 1;
    blocks.splice(at, 0, next);

    this.caret = { ...caret, block: at, inline: 0, offset: 0 };
    this.anchor = { ...this.caret };
    return at;
  }

  /** Backspace. */
  deleteBackward(): void {
    if (!this.caret) return;

    if (this.hasSelection()) {
      this.store.commit(() => this.applyDeleteSelection());
      return;
    }

    this.store.commit(() => {
      const caret = this.caret;
      if (!caret) return;

      const paragraph = this.paragraphAt(caret);
      const inline = paragraph?.content[caret.inline];

      if (paragraph && inline?.type === "text" && caret.offset > 0) {
        const index = byteToIndex(inline.text, caret.offset);
        const previous = previousBoundary(inline.text, index);
        inline.text = inline.text.slice(0, previous) + inline.text.slice(index);
        caret.offset = indexToByte(inline.text, previous);
        this.anchor = { ...caret };
        return;
      }

      // At the start of an inline: step back and join.
      const moved = this.stepBack(caret);
      if (!moved) return;
      this.caret = moved;
      this.anchor = { ...moved };
      this.joinAt(moved);
    });
  }

  /** Forward delete. */
  deleteForward(): void {
    if (!this.caret) return;

    if (this.hasSelection()) {
      this.store.commit(() => this.applyDeleteSelection());
      return;
    }

    this.store.commit(() => {
      const caret = this.caret;
      if (!caret) return;

      const paragraph = this.paragraphAt(caret);
      const inline = paragraph?.content[caret.inline];
      if (!paragraph || inline?.type !== "text") return;

      const index = byteToIndex(inline.text, caret.offset);
      if (index < inline.text.length) {
        const next = nextBoundary(inline.text, index);
        inline.text = inline.text.slice(0, index) + inline.text.slice(next);
        return;
      }
      this.joinAt(caret);
    });
  }

  /**
   * Merge whatever follows `caret` into the block it sits in.
   *
   * Used by both delete directions once the caret has been placed at the seam.
   */
  private joinAt(caret: Caret): void {
    const blocks = this.blocksAt(caret);
    const paragraph = this.paragraphAt(caret);
    if (!blocks || !paragraph) return;

    const inline = paragraph.content[caret.inline];
    const atEndOfInline =
      !inline || inline.type !== "text" || byteToIndex(inline.text, caret.offset) >= inline.text.length;
    if (!atEndOfInline) return;

    // Pull in the next inline of the same paragraph, if there is one.
    const following = paragraph.content[caret.inline + 1];
    if (following) {
      if (following.type === "text" && inline?.type === "text") {
        inline.text += following.text;
        paragraph.content.splice(caret.inline + 1, 1);
      } else {
        paragraph.content.splice(caret.inline + 1, 1);
      }
      return;
    }

    // Otherwise pull up the next paragraph.
    const next = blocks[caret.block + 1];
    if (!next || next.type !== "paragraph") return;
    paragraph.content.push(...next.content);
    blocks.splice(caret.block + 1, 1);
  }

  /** Remove the selected range. Must run inside a commit. */
  private applyDeleteSelection(): void {
    const range = this.range();
    if (!range) return;
    const [start, end] = range;

    const blocks = this.blocksAt(start);
    if (!blocks) return;

    if (start.block === end.block && start.inline === end.inline) {
      const paragraph = this.paragraphAt(start);
      const inline = paragraph?.content[start.inline];
      if (inline?.type === "text") {
        const from = byteToIndex(inline.text, start.offset);
        const to = byteToIndex(inline.text, end.offset);
        inline.text = inline.text.slice(0, from) + inline.text.slice(to);
      }
    } else {
      // Trim the head, drop everything between, trim the tail, then join.
      const head = this.paragraphAt(start);
      const tail = this.paragraphAt(end);
      if (!head || !tail) return;

      const headInline = head.content[start.inline];
      if (headInline?.type === "text") {
        headInline.text = headInline.text.slice(0, byteToIndex(headInline.text, start.offset));
      }
      head.content.length = start.inline + 1;

      const tailInline = tail.content[end.inline];
      if (tailInline?.type === "text") {
        tailInline.text = tailInline.text.slice(byteToIndex(tailInline.text, end.offset));
      }
      const remainder = tail.content.slice(end.inline);

      head.content.push(...remainder);
      blocks.splice(start.block + 1, end.block - start.block);
    }

    this.caret = { ...start };
    this.anchor = { ...start };
  }

  /**
   * Apply a character style to the selected text.
   *
   * The runs under the selection are split at its boundaries and the patch is
   * merged into the pieces in between — the standard way a mark is stored in a
   * run-based model. Boundaries are tracked as offsets into the paragraph's
   * whole text, because inline indices shift as soon as the first split lands.
   */
  applyStyle(patch: Style): void {
    const range = this.range();
    if (!range) return;
    const [start, end] = range;

    this.store.commit(() => {
      const blocks = this.blocksAt(start);
      if (!blocks) return;

      const startParagraph = this.paragraphAt(start);
      const endParagraph = this.paragraphAt(end);
      if (!startParagraph || !endParagraph) return;

      const startAbsolute = absoluteOffset(startParagraph, start.inline, start.offset);
      const endAbsolute = absoluteOffset(endParagraph, end.inline, end.offset);

      // Back to front, so untouched indices stay valid while we splice.
      for (let index = end.block; index >= start.block; index -= 1) {
        const block = blocks[index];
        if (!block || block.type !== "paragraph") continue;

        const from = index === start.block ? startAbsolute : 0;
        const to = index === end.block ? endAbsolute : paragraphLength(block);
        styleRange(block, from, to, patch);
      }

      // Re-resolve the boundaries against the freshly split content.
      const head = blocks[start.block];
      const tail = blocks[end.block];
      if (head?.type === "paragraph") {
        const at = resolveOffset(head, startAbsolute);
        this.anchor = { ...start, inline: at.inline, offset: at.offset };
      }
      if (tail?.type === "paragraph") {
        const at = resolveOffset(tail, endAbsolute);
        this.caret = { ...end, inline: at.inline, offset: at.offset };
      }
    });
  }

  /** The selected text, flattened to plain characters. */
  selectedText(): string {
    const range = this.range();
    if (!range) return "";
    const [start, end] = range;

    const blocks = this.blocksAt(start);
    if (!blocks) return "";

    const parts: string[] = [];
    for (let index = start.block; index <= end.block; index += 1) {
      const block = blocks[index];
      if (!block || block.type !== "paragraph") continue;

      const firstInline = index === start.block ? start.inline : 0;
      const lastInline = index === end.block ? end.inline : block.content.length - 1;

      for (let at = firstInline; at <= lastInline; at += 1) {
        const inline = block.content[at];
        if (!inline || inline.type !== "text") continue;

        const from =
          index === start.block && at === start.inline
            ? byteToIndex(inline.text, start.offset)
            : 0;
        const to =
          index === end.block && at === end.inline
            ? byteToIndex(inline.text, end.offset)
            : inline.text.length;

        parts.push(inline.text.slice(from, to));
      }
      // A paragraph boundary reads as a line break outside the editor.
      if (index < end.block) parts.push("\n");
    }

    return parts.join("");
  }

  /** Copy the selection and remove it. */
  cut(): string {
    const text = this.selectedText();
    if (text) this.store.commit(() => this.applyDeleteSelection());
    return text;
  }

  /** The style of the run under the caret, for showing the inspector's state. */
  styleAtCaret(): Style | null {
    if (!this.caret) return null;
    const paragraph = this.paragraphAt(this.caret);
    const inline = paragraph?.content[this.caret.inline];
    return inline?.type === "text" ? (inline.style ?? {}) : null;
  }

  // ── Movement ──────────────────────────────────────────────────────────────

  moveHorizontal(direction: Direction, extend: boolean): void {
    if (!this.caret) return;

    // Collapsing a selection with a plain arrow key jumps to its edge.
    if (!extend && this.hasSelection()) {
      const range = this.range();
      if (range) {
        this.caret = { ...(direction < 0 ? range[0] : range[1]) };
        this.anchor = { ...this.caret };
        return;
      }
    }

    const moved = direction < 0 ? this.stepBack(this.caret) : this.stepForward(this.caret);
    if (!moved) return;
    this.caret = moved;
    if (!extend) this.anchor = { ...moved };
  }

  moveVertical(
    direction: Direction,
    extend: boolean,
    list: DisplayList,
    page: DisplayPage,
  ): void {
    if (!this.caret) return;
    const moved = verticalNeighbour(list, page, this.caret, direction, this.desiredX);
    if (!moved) return;
    this.caret = moved;
    if (!extend) this.anchor = { ...moved };
  }

  moveToLineEdge(direction: Direction, extend: boolean): void {
    if (!this.caret) return;
    const paragraph = this.paragraphAt(this.caret);
    if (!paragraph) return;

    let moved: Caret;
    if (direction < 0) {
      moved = { ...this.caret, inline: 0, offset: 0 };
    } else {
      const last = paragraph.content.length - 1;
      const inline = paragraph.content[last];
      moved = {
        ...this.caret,
        inline: Math.max(0, last),
        offset: inline?.type === "text" ? utf8Length(inline.text) : 0,
      };
    }

    this.caret = moved;
    if (!extend) this.anchor = { ...moved };
  }

  selectAll(): void {
    if (!this.caret) return;
    const blocks = this.blocksAt(this.caret);
    if (!blocks || blocks.length === 0) return;

    const lastIndex = blocks.length - 1;
    const last = blocks[lastIndex];
    const lastInline = last?.type === "paragraph" ? last.content.length - 1 : 0;
    const inline = last?.type === "paragraph" ? last.content[lastInline] : undefined;

    this.anchor = { ...this.caret, block: 0, inline: 0, offset: 0 };
    this.caret = {
      ...this.caret,
      block: lastIndex,
      inline: Math.max(0, lastInline),
      offset: inline?.type === "text" ? utf8Length(inline.text) : 0,
    };
  }

  /** One code point back, crossing inline and block boundaries. */
  private stepBack(caret: Caret): Caret | null {
    const paragraph = this.paragraphAt(caret);
    const inline = paragraph?.content[caret.inline];

    if (inline?.type === "text" && caret.offset > 0) {
      const index = byteToIndex(inline.text, caret.offset);
      return { ...caret, offset: indexToByte(inline.text, previousBoundary(inline.text, index)) };
    }

    if (paragraph && caret.inline > 0) {
      const previous = paragraph.content[caret.inline - 1];
      return {
        ...caret,
        inline: caret.inline - 1,
        offset: previous?.type === "text" ? utf8Length(previous.text) : 0,
      };
    }

    if (caret.block > 0) {
      const blocks = this.blocksAt(caret);
      const previous = blocks?.[caret.block - 1];
      if (previous?.type === "paragraph") {
        const last = Math.max(0, previous.content.length - 1);
        const inline = previous.content[last];
        return {
          ...caret,
          block: caret.block - 1,
          inline: last,
          offset: inline?.type === "text" ? utf8Length(inline.text) : 0,
        };
      }
    }

    return null;
  }

  /** One code point forward, crossing inline and block boundaries. */
  private stepForward(caret: Caret): Caret | null {
    const paragraph = this.paragraphAt(caret);
    const inline = paragraph?.content[caret.inline];

    if (inline?.type === "text") {
      const index = byteToIndex(inline.text, caret.offset);
      if (index < inline.text.length) {
        return { ...caret, offset: indexToByte(inline.text, nextBoundary(inline.text, index)) };
      }
    }

    if (paragraph && caret.inline + 1 < paragraph.content.length) {
      return { ...caret, inline: caret.inline + 1, offset: 0 };
    }

    const blocks = this.blocksAt(caret);
    if (blocks && caret.block + 1 < blocks.length) {
      return { ...caret, block: caret.block + 1, inline: 0, offset: 0 };
    }

    return null;
  }

  // ── Document access ───────────────────────────────────────────────────────

  private asSource(caret: Caret): SourceRef {
    return {
      page: 0,
      frame: caret.frame,
      story: caret.story,
      block: caret.block,
      inline: caret.inline,
      offset: caret.offset,
    };
  }

  private blocksAt(caret: Caret) {
    return this.store.blocksOf(this.asSource(caret));
  }

  private paragraphAt(caret: Caret): Paragraph | null {
    return this.store.paragraphOf(this.asSource(caret));
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Paragraph offsets
//
// Inline indices are unstable the moment a run is split, so any operation that
// restructures a paragraph tracks positions as an offset into its whole text
// and re-resolves afterwards.
// ─────────────────────────────────────────────────────────────────────────────

function inlineLength(inline: Inline): number {
  return inline.type === "text" ? utf8Length(inline.text) : 0;
}

function paragraphLength(paragraph: Paragraph): number {
  return paragraph.content.reduce((total, inline) => total + inlineLength(inline), 0);
}

/** (inline, offset) → offset into the paragraph's whole text. */
function absoluteOffset(paragraph: Paragraph, inline: number, offset: number): number {
  let total = 0;
  for (let index = 0; index < inline && index < paragraph.content.length; index += 1) {
    total += inlineLength(paragraph.content[index]!);
  }
  return total + offset;
}

/** offset into the paragraph's whole text → (inline, offset). */
function resolveOffset(paragraph: Paragraph, absolute: number): { inline: number; offset: number } {
  let remaining = absolute;
  for (let index = 0; index < paragraph.content.length; index += 1) {
    const length = inlineLength(paragraph.content[index]!);
    if (remaining <= length) return { inline: index, offset: remaining };
    remaining -= length;
  }
  const last = Math.max(0, paragraph.content.length - 1);
  const inline = paragraph.content[last];
  return { inline: last, offset: inline ? inlineLength(inline) : 0 };
}

/** Split the runs covering `[from, to)` and merge `patch` into them. */
function styleRange(paragraph: Paragraph, from: number, to: number, patch: Style): void {
  if (to <= from) return;

  let cursor = 0;
  for (let index = paragraph.content.length - 1; index >= 0; index -= 1) {
    // Recompute the start of this inline; splices only affect later indices.
    cursor = 0;
    for (let before = 0; before < index; before += 1) {
      cursor += inlineLength(paragraph.content[before]!);
    }

    const inline = paragraph.content[index];
    if (!inline || inline.type !== "text") continue;

    const start = cursor;
    const end = start + utf8Length(inline.text);
    const overlapFrom = Math.max(from, start);
    const overlapTo = Math.min(to, end);
    if (overlapTo <= overlapFrom) continue;

    const head = inline.text.slice(0, byteToIndex(inline.text, overlapFrom - start));
    const middle = inline.text.slice(
      byteToIndex(inline.text, overlapFrom - start),
      byteToIndex(inline.text, overlapTo - start),
    );
    const tail = inline.text.slice(byteToIndex(inline.text, overlapTo - start));

    const pieces: TextRun[] = [];
    if (head) pieces.push({ ...inline, text: head });
    pieces.push({ ...inline, text: middle, style: { ...(inline.style ?? {}), ...patch } });
    if (tail) pieces.push({ ...inline, text: tail });

    paragraph.content.splice(index, 1, ...pieces);
  }
}

export { byteToIndex, indexToByte, utf8Length } from "./utf8";
