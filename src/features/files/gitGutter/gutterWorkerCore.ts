import { diffLines } from "./lineDiff";

/**
 * The gutter worker's state machine, apart from `self` so tests drive it
 * directly. Each editor sends its document once, then only the edits
 * made since its last message, so the main thread never builds the text again.
 */
export type GutterRequest =
  | { type: "open"; id: number; version: number; base: string; doc: string }
  | {
      type: "update";
      id: number;
      version: number;
      /** Edits since the last message, as `ChangeSet.toJSON()`. */
      changes: unknown;
      /** A new staged text, applied after the edits. */
      base: string | null;
    }
  | { type: "close"; id: number };

/**
 * Flat hunks, five numbers each: `start, end` (current lines, half open),
 * `baseFrom, baseTo` (offsets of the staged text the hunk replaced, without
 * the final line break) and `kind` (`HUNK_*`).
 */
export interface GutterResult {
  id: number;
  version: number;
  hunks: number[];
  /** More hunks than `MAX_GUTTER_HUNKS`; only the first ones are listed. */
  truncated?: boolean;
  /** The worker has no document for `id`; the editor must open it again. */
  lost?: boolean;
}

export const HUNK_ADDED = 0;
export const HUNK_MODIFIED = 1;
export const HUNK_DELETED = 2;
export const HUNK_FIELDS = 5;
export const MAX_GUTTER_HUNKS = 2_000;

interface Tracked {
  base: readonly string[];
  /** Offset of each staged line's start, plus one past the end. */
  baseStarts: Int32Array;
  baseText: string;
  doc: string;
}

export function splitLines(text: string): string[] {
  return text.split(/\r\n|\r|\n/);
}

function lineStarts(text: string, lines: readonly string[]): Int32Array {
  const starts = new Int32Array(lines.length + 1);
  let offset = 0;
  for (let index = 0; index < lines.length; index += 1) {
    starts[index] = offset;
    offset += lines[index]!.length;
    // The break after this line is one or two characters.
    if (text.charCodeAt(offset) === 13 && text.charCodeAt(offset + 1) === 10)
      offset += 2;
    else offset += 1;
  }
  starts[lines.length] = text.length + 1;
  return starts;
}

/**
 * Applies a CodeMirror `ChangeSet.toJSON()` to the text it was made against:
 * a number keeps that many characters, `[deleted, ...lines]` replaces some.
 * Done by hand so the worker stays free of the CodeMirror runtime.
 */
export function applyChanges(text: string, changes: unknown): string {
  if (!Array.isArray(changes)) throw new Error("malformed changes");
  const parts: string[] = [];
  let position = 0;
  for (const part of changes) {
    if (typeof part === "number") {
      parts.push(text.slice(position, position + part));
      position += part;
    } else if (Array.isArray(part) && typeof part[0] === "number") {
      position += part[0];
      parts.push(part.slice(1).join("\n"));
    } else {
      throw new Error("malformed changes");
    }
  }
  if (position !== text.length) throw new Error("changes do not fit");
  return parts.join("");
}

function tracked(baseText: string, doc: string): Tracked {
  const base = splitLines(baseText);
  return { base, baseStarts: lineStarts(baseText, base), baseText, doc };
}

export function createGutterWorkerCore() {
  const documents = new Map<number, Tracked>();

  function result(id: number, version: number, entry: Tracked): GutterResult {
    const changes = diffLines(entry.base, splitLines(entry.doc));
    const truncated = changes.length > MAX_GUTTER_HUNKS;
    const hunks: number[] = [];
    for (const change of changes.slice(0, MAX_GUTTER_HUNKS)) {
      const removed = change.baseEnd > change.baseStart;
      const added = change.end > change.start;
      const baseFrom = entry.baseStarts[change.baseStart]!;
      // Up to the break that ends the last replaced line.
      const baseTo = removed
        ? entry.baseStarts[change.baseEnd - 1]! +
          entry.base[change.baseEnd - 1]!.length
        : baseFrom;
      hunks.push(
        change.start,
        change.end,
        baseFrom,
        baseTo,
        added && removed ? HUNK_MODIFIED : added ? HUNK_ADDED : HUNK_DELETED,
      );
    }
    return truncated
      ? { id, version, hunks, truncated }
      : { id, version, hunks };
  }

  return {
    handle(request: GutterRequest): GutterResult | null {
      if (request.type === "close") {
        documents.delete(request.id);
        return null;
      }
      let entry = documents.get(request.id);
      if (request.type === "open") {
        entry = tracked(request.base, request.doc);
      } else if (!entry) {
        return {
          id: request.id,
          version: request.version,
          hunks: [],
          lost: true,
        };
      } else {
        try {
          const doc =
            request.changes === null
              ? entry.doc
              : applyChanges(entry.doc, request.changes);
          entry =
            request.base === null
              ? { ...entry, doc }
              : tracked(request.base, doc);
        } catch {
          // Out of step with the editor: drop the File so it is sent again.
          documents.delete(request.id);
          return {
            id: request.id,
            version: request.version,
            hunks: [],
            lost: true,
          };
        }
      }
      documents.set(request.id, entry);
      return result(request.id, request.version, entry);
    },
    size: () => documents.size,
  };
}
