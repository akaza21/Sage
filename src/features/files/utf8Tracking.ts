import { StateField, type Text, type Transaction } from "@codemirror/state";

/**
 * Counts exactly as TextEncoder does: a lone surrogate becomes U+FFFD (3
 * bytes).
 */
export function utf8Length(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        bytes += 4;
        index += 1;
      } else bytes += 3;
    } else bytes += 3;
  }
  return bytes;
}

/** Lines never split a surrogate pair. */
export function documentUtf8Length(document: Text): number {
  let bytes = 0;
  for (const iterator = document.iter(); !iterator.next().done;) {
    bytes += iterator.lineBreak ? 1 : utf8Length(iterator.value);
  }
  return bytes;
}

/**
 * Each changed range is widened by one code unit on both sides, so an edit that
 * splits or joins a surrogate pair is counted with its neighbour.
 */
export function utf8Delta(transaction: Transaction): number {
  const before = transaction.startState.doc;
  const after = transaction.newDoc;
  let delta = 0;
  transaction.changes.iterChangedRanges((fromA, toA, fromB, toB) => {
    delta -= utf8Length(
      before.sliceString(
        Math.max(0, fromA - 1),
        Math.min(before.length, toA + 1),
      ),
    );
    delta += utf8Length(
      after.sliceString(
        Math.max(0, fromB - 1),
        Math.min(after.length, toB + 1),
      ),
    );
  });
  return delta;
}

export const utf8LengthField = StateField.define<number>({
  create: (state) => documentUtf8Length(state.doc),
  update: (bytes, transaction) =>
    transaction.docChanged ? bytes + utf8Delta(transaction) : bytes,
});

export function withinUtf8Limit(
  transaction: Transaction,
  limit: number,
): boolean {
  if (!transaction.docChanged) return true;
  // UTF-16 length bounds UTF-8 length from below and, times three, above.
  if (transaction.newDoc.length > limit) return false;
  if (transaction.newDoc.length <= Math.floor(limit / 3)) return true;
  const before =
    transaction.startState.field(utf8LengthField, false) ??
    documentUtf8Length(transaction.startState.doc);
  return before + utf8Delta(transaction) <= limit;
}
