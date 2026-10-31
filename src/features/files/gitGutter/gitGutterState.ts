import {
  StateEffect,
  StateField,
  type ChangeDesc,
  type ChangeSpec,
  type EditorState,
  type Text,
} from "@codemirror/state";

import {
  HUNK_ADDED,
  HUNK_DELETED,
  HUNK_FIELDS,
  HUNK_MODIFIED,
} from "./gutterWorkerCore";

export type HunkKind = "added" | "modified" | "deleted";

/**
 * One change against the staged text, in document positions so it maps
 * through edits made before the next diff. `from` is the start of the first
 * changed line and `to` the end of the last; a deletion has `from === to`, at
 * the start of the line after it, or at the end of the document (`atEnd`).
 */
export interface GutterHunk {
  kind: HunkKind;
  from: number;
  to: number;
  atEnd: boolean;
  baseFrom: number;
  baseTo: number;
}

export interface GutterState {
  /** The staged text, or null when the File has no markers. */
  base: string | null;
  hunks: readonly GutterHunk[];
  /** Past the worker's hunk cap only the first changes are listed. */
  truncated: boolean;
  /** The hunk whose peek is open. */
  peek: number | null;
}

export const setGutterBase = StateEffect.define<string | null>();
export const setGutterHunks = StateEffect.define<{
  hunks: readonly GutterHunk[];
  truncated: boolean;
}>();
export const setGutterPeek = StateEffect.define<number | null>();

const KINDS: Record<number, HunkKind> = {
  [HUNK_ADDED]: "added",
  [HUNK_MODIFIED]: "modified",
  [HUNK_DELETED]: "deleted",
};

/** True when `changes` replace all of a non-empty document of `length`. */
export function replacesDocument(changes: ChangeDesc, length: number) {
  let whole = false;
  changes.iterChangedRanges((from, to) => {
    if (length > 0 && from === 0 && to === length) whole = true;
  });
  return whole;
}

const EMPTY: GutterState = {
  base: null,
  hunks: [],
  truncated: false,
  peek: null,
};

function mapHunk(hunk: GutterHunk, changes: ChangeDesc): GutterHunk {
  const from = changes.mapPos(hunk.from, hunk.kind === "deleted" ? 1 : -1);
  const to = hunk.kind === "deleted" ? from : changes.mapPos(hunk.to, 1);
  return from === hunk.from && to === hunk.to ? hunk : { ...hunk, from, to };
}

export const gutterField = StateField.define<GutterState>({
  create: () => EMPTY,
  update(value, transaction) {
    let next = value;
    if (transaction.docChanged && value.hunks.length > 0) {
      // Any edit closes the peek: its staged text no longer lines up. A
      // whole-document replace (an outside reload) would stretch every hunk
      // over the document, so the markers wait for the next diff instead.
      next = {
        ...value,
        hunks: replacesDocument(
          transaction.changes,
          transaction.startState.doc.length,
        )
          ? []
          : value.hunks.map((hunk) => mapHunk(hunk, transaction.changes)),
        peek: null,
      };
    }
    for (const effect of transaction.effects) {
      if (effect.is(setGutterBase)) {
        // Hunk offsets point into the old staged text, so they go with it.
        next = effect.value === null ? EMPTY : { ...EMPTY, base: effect.value };
      } else if (effect.is(setGutterHunks)) {
        if (next.base === null) continue;
        // An open peek stays on its change, or closes if that change is gone.
        const open = next.peek === null ? undefined : next.hunks[next.peek];
        const peek = open
          ? effect.value.hunks.findIndex(
              (hunk) => hunk.from === open.from && hunk.kind === open.kind,
            )
          : -1;
        next = {
          ...next,
          hunks: effect.value.hunks,
          truncated: effect.value.truncated,
          peek: peek === -1 ? null : peek,
        };
      } else if (effect.is(setGutterPeek)) {
        next = { ...next, peek: effect.value };
      }
    }
    return next;
  },
});

/** Turns the worker's flat line hunks into positions in `doc`. */
export function hunksFromLines(
  flat: readonly number[],
  doc: Text,
): GutterHunk[] {
  const hunks: GutterHunk[] = [];
  for (
    let index = 0;
    index + HUNK_FIELDS <= flat.length;
    index += HUNK_FIELDS
  ) {
    const start = flat[index]!;
    const end = flat[index + 1]!;
    const kind = KINDS[flat[index + 4]!] ?? "modified";
    const baseFrom = flat[index + 2]!;
    const baseTo = flat[index + 3]!;
    if (kind === "deleted") {
      const atEnd = start >= doc.lines;
      const from = atEnd ? doc.length : doc.line(start + 1).from;
      hunks.push({ kind, from, to: from, atEnd, baseFrom, baseTo });
    } else {
      if (end > doc.lines || start >= end) continue;
      hunks.push({
        kind,
        from: doc.line(start + 1).from,
        to: doc.line(end).to,
        atEnd: false,
        baseFrom,
        baseTo,
      });
    }
  }
  return hunks;
}

/** The hunk drawn on the line `[lineFrom, lineTo]`, if any. */
export function hunkAtLine(
  hunks: readonly GutterHunk[],
  lineFrom: number,
  lineTo: number,
  docLength: number,
): number | null {
  let low = 0;
  let high = hunks.length;
  // The first hunk that ends at or after this line's start.
  while (low < high) {
    const middle = (low + high) >> 1;
    if (hunks[middle]!.to < lineFrom) low = middle + 1;
    else high = middle;
  }
  for (let index = low; index < hunks.length; index += 1) {
    const hunk = hunks[index]!;
    if (hunk.from > lineTo) break;
    if (hunk.kind !== "deleted") return index;
    if (hunk.atEnd ? lineTo === docLength : hunk.from === lineFrom)
      return index;
  }
  return null;
}

/** The staged text a hunk replaced, with the editor's `\n` breaks. */
export function stagedText(base: string, hunk: GutterHunk): string | null {
  return hunk.kind === "added"
    ? null
    : base.slice(hunk.baseFrom, hunk.baseTo).replace(/\r\n?/g, "\n");
}

/** The edit that puts a hunk's staged lines back. */
export function revertChange(state: EditorState, hunk: GutterHunk): ChangeSpec {
  const doc = state.doc;
  const staged = stagedText(state.field(gutterField).base ?? "", hunk);
  if (hunk.kind === "modified") {
    return { from: hunk.from, to: hunk.to, insert: staged ?? "" };
  }
  if (hunk.kind === "deleted") {
    return hunk.atEnd
      ? { from: doc.length, insert: `\n${staged ?? ""}` }
      : { from: hunk.from, insert: `${staged ?? ""}\n` };
  }
  // Added lines go with one adjoining break.
  if (hunk.to < doc.length) return { from: hunk.from, to: hunk.to + 1 };
  return { from: Math.max(0, hunk.from - 1), to: hunk.to };
}
