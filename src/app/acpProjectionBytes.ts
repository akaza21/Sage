import type { AcpChatPage, TurnProjection } from "../domain/acp";
import { utf8ByteLength } from "../domain/text";

/**
 * Serialized sizes of Items and activities, keyed by object. The store never
 * mutates them, and a merge keeps the objects it does not replace, so a
 * refresh serializes only what changed instead of the whole retained page.
 */
const entryBytes = new WeakMap<object, number>();

/**
 * The same sizes keyed by entity id and revision. Each refresh decodes the
 * newest window into fresh objects, so the WeakMap misses them; the store
 * bumps an entity's revision on every content change, so an unchanged
 * revision means unchanged bytes. Oldest entries are evicted past the cap.
 */
const revisionBytes = new Map<string, { revision: string; bytes: number }>();
const MAX_REVISION_ENTRIES = 8_192;

interface RevisionedEntry {
  id: string;
  revision: string;
}

function jsonBytes(value: unknown): number {
  return utf8ByteLength(JSON.stringify(value));
}

function revisionedBytes(kind: string, entry: RevisionedEntry): number {
  if (typeof entry.revision !== "string") return jsonBytes(entry);
  const key = `${kind}:${entry.id}`;
  const cached = revisionBytes.get(key);
  if (cached?.revision === entry.revision) return cached.bytes;
  const bytes = jsonBytes(entry);
  revisionBytes.delete(key);
  revisionBytes.set(key, { revision: entry.revision, bytes });
  if (revisionBytes.size > MAX_REVISION_ENTRIES) {
    const oldest = revisionBytes.keys().next();
    if (!oldest.done) revisionBytes.delete(oldest.value);
  }
  return bytes;
}

function entryByteLength(kind: string, entry: RevisionedEntry): number {
  let bytes = entryBytes.get(entry);
  if (bytes === undefined) {
    bytes = revisionedBytes(kind, entry);
    entryBytes.set(entry, bytes);
  }
  return bytes;
}

/** Bytes the entries add inside an array literal: each entry and its comma. */
function entriesByteLength(
  kind: string,
  entries: readonly RevisionedEntry[],
): number {
  let total = Math.max(0, entries.length - 1);
  for (const entry of entries) total += entryByteLength(kind, entry);
  return total;
}

function turnByteLength(turn: TurnProjection): number {
  return (
    jsonBytes({ ...turn, items: [], activities: [] }) +
    entriesByteLength("item", turn.items) +
    entriesByteLength("activity", turn.activities)
  );
}

/** Exactly `utf8ByteLength(JSON.stringify(page))`, computed incrementally. */
export function pageByteLength(page: AcpChatPage): number {
  let total = jsonBytes({ ...page, turns: [] });
  total += Math.max(0, page.turns.length - 1);
  for (const turn of page.turns) total += turnByteLength(turn);
  return total;
}
