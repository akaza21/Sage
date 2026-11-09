import type { AcpChatPage, TurnProjection } from "../domain/acp";
import { utf8ByteLength } from "../domain/text";

/**
 * Serialized sizes of Items and activities, keyed by object. The store never
 * mutates them, and a merge keeps the objects it does not replace, so a
 * refresh serializes only what changed instead of the whole retained page.
 */
const entryBytes = new WeakMap<object, number>();

function jsonBytes(value: unknown): number {
  return utf8ByteLength(JSON.stringify(value));
}

function entryByteLength(entry: object): number {
  let bytes = entryBytes.get(entry);
  if (bytes === undefined) {
    bytes = jsonBytes(entry);
    entryBytes.set(entry, bytes);
  }
  return bytes;
}

/** Bytes the entries add inside an array literal: each entry and its comma. */
function entriesByteLength(entries: readonly object[]): number {
  let total = Math.max(0, entries.length - 1);
  for (const entry of entries) total += entryByteLength(entry);
  return total;
}

function turnByteLength(turn: TurnProjection): number {
  return (
    jsonBytes({ ...turn, items: [], activities: [] }) +
    entriesByteLength(turn.items) +
    entriesByteLength(turn.activities)
  );
}

/** Exactly `utf8ByteLength(JSON.stringify(page))`, computed incrementally. */
export function pageByteLength(page: AcpChatPage): number {
  let total = jsonBytes({ ...page, turns: [] });
  total += Math.max(0, page.turns.length - 1);
  for (const turn of page.turns) total += turnByteLength(turn);
  return total;
}
