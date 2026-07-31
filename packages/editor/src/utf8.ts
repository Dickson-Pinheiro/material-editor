/**
 * UTF-8 ↔ UTF-16 offset conversion.
 *
 * The engine reports every offset as a **UTF-8 byte** position, because that is
 * what a shaping cluster is. JavaScript strings are UTF-16. Mixing the two
 * silently misplaces the caret the moment a document contains an accent, so
 * every conversion goes through here.
 */

function unitsFor(codePoint: number): number {
  if (codePoint < 0x80) return 1;
  if (codePoint < 0x800) return 2;
  if (codePoint < 0x10000) return 3;
  return 4;
}

/** UTF-8 byte length of a JavaScript string. */
export function utf8Length(text: string): number {
  let bytes = 0;
  for (const character of text) bytes += unitsFor(character.codePointAt(0)!);
  return bytes;
}

/** UTF-8 byte offset → UTF-16 index. Clamps to the string's bounds. */
export function byteToIndex(text: string, byte: number): number {
  if (byte <= 0) return 0;
  let bytes = 0;
  let index = 0;
  while (index < text.length) {
    if (bytes >= byte) return index;
    const codePoint = text.codePointAt(index)!;
    bytes += unitsFor(codePoint);
    index += codePoint >= 0x10000 ? 2 : 1;
  }
  return text.length;
}

/** UTF-16 index → UTF-8 byte offset. */
export function indexToByte(text: string, index: number): number {
  const limit = Math.min(Math.max(index, 0), text.length);
  let bytes = 0;
  let cursor = 0;
  while (cursor < limit) {
    const codePoint = text.codePointAt(cursor)!;
    bytes += unitsFor(codePoint);
    cursor += codePoint >= 0x10000 ? 2 : 1;
  }
  return bytes;
}

/** Previous code-point boundary, so a surrogate pair deletes as one character. */
export function previousBoundary(text: string, index: number): number {
  if (index <= 0) return 0;
  const before = text.codePointAt(index - 2);
  return before !== undefined && before >= 0x10000 ? index - 2 : index - 1;
}

/** Next code-point boundary. */
export function nextBoundary(text: string, index: number): number {
  const codePoint = text.codePointAt(index);
  if (codePoint === undefined) return index;
  return index + (codePoint >= 0x10000 ? 2 : 1);
}
