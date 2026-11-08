// Quick Open's request and response shapes, matching the native
// `quick_open_query` command, and the parsing around them.

import { isRecord } from "../../domain/guards";
import { truncateUtf8 } from "../../domain/text";
import type { ResourceRef } from "../../domain/workbench";

export const QUICK_OPEN_LIMIT = 50;
/** The native command takes at most this many query bytes. */
const MAX_QUERY_BYTES = 1024;

export interface QuickOpenRequest {
  projectId: string;
  expectedTransitionEpoch: number;
  query: string;
  limit: number;
  generation: number;
  session: number;
  recent: readonly ResourceRef[];
}

export interface QuickOpenMatch extends ResourceRef {
  /** Matched characters, as UTF-16 offsets into `relativePath`. */
  positions: number[];
}

export interface QuickOpenResponse {
  state: "ready" | "superseded";
  results: QuickOpenMatch[];
  indexedFiles: number;
  truncated: boolean;
  /** A fresher index is being built; asking again shortly sees it. */
  refreshing: boolean;
}

export type QuickOpenInvoke = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;

let lastStamp = 0;

/**
 * Increasing numbers for generations and sessions. Native keeps the highest
 * generation it has seen across webview reloads, so these start from the
 * clock rather than from zero.
 */
export function nextQuickOpenStamp(now = Date.now()): number {
  lastStamp = Math.max(now * 1000, lastStamp + 1);
  return lastStamp;
}

/** Splits a trailing `:line` or `:line:column` from what was typed. */
export function parseQuickOpenInput(text: string): {
  query: string;
  line: number | null;
  column: number | null;
} {
  // A ":" being typed is already a suffix, so `name:` asks for `name`.
  const suffix = /:(\d{0,9})(?::(\d{0,9}))?\s*$/u.exec(text);
  const query = truncateUtf8(
    (suffix ? text.slice(0, suffix.index) : text).trim(),
    MAX_QUERY_BYTES,
  );
  const line = suffix?.[1] ? Number(suffix[1]) : null;
  const column = suffix?.[2] ? Number(suffix[2]) : null;
  return {
    query,
    line: line && line > 0 ? line : null,
    column: column && column > 0 ? column : null,
  };
}

function parseMatch(value: unknown): QuickOpenMatch | null {
  if (
    !isRecord(value) ||
    typeof value.rootId !== "string" ||
    typeof value.relativePath !== "string" ||
    !Array.isArray(value.positions) ||
    !value.positions.every(
      (position) =>
        Number.isInteger(position) &&
        position >= 0 &&
        position < (value.relativePath as string).length,
    )
  ) {
    return null;
  }
  return {
    rootId: value.rootId,
    relativePath: value.relativePath,
    positions: value.positions as number[],
  };
}

export function parseQuickOpenResponse(value: unknown): QuickOpenResponse {
  const results =
    isRecord(value) && Array.isArray(value.results)
      ? value.results.map(parseMatch)
      : null;
  if (
    !isRecord(value) ||
    (value.state !== "ready" && value.state !== "superseded") ||
    !results ||
    results.length > QUICK_OPEN_LIMIT ||
    results.some((result) => result === null) ||
    !Number.isInteger(value.indexedFiles) ||
    typeof value.truncated !== "boolean" ||
    typeof value.refreshing !== "boolean"
  ) {
    throw new Error("invalid Quick Open response");
  }
  return {
    state: value.state,
    results: results as QuickOpenMatch[],
    indexedFiles: value.indexedFiles as number,
    truncated: value.truncated,
    refreshing: value.refreshing,
  };
}

/** `text` cut into runs, each marked whether it holds matched characters. */
export function highlightRuns(
  text: string,
  offset: number,
  positions: readonly number[],
): Array<{ text: string; matched: boolean }> {
  const matched = new Set(positions);
  const runs: Array<{ text: string; matched: boolean }> = [];
  let index = offset;
  // By code point, so an emoji is never split across two runs.
  for (const character of text) {
    let isMatch = false;
    for (let unit = 0; unit < character.length; unit += 1) {
      isMatch ||= matched.has(index + unit);
    }
    index += character.length;
    const last = runs.at(-1);
    if (last && last.matched === isMatch) last.text += character;
    else runs.push({ text: character, matched: isMatch });
  }
  return runs;
}

/**
 * The keys of a shortcut label such as "Ctrl+Shift+P", one per keycap. A
 * literal "+" key ("Ctrl++") stays a key of its own.
 */
export function shortcutKeys(label: string): string[] {
  return label.split(/\+(?=.)/).filter(Boolean);
}
