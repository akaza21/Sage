import { useEffect, useSyncExternalStore } from "react";

import { invokeNative, isTauriRuntime } from "../../../ports/nativeInvoke";

/**
 * Which chat updates the user has seen, so a finished chat stays read after
 * a reload or a restart. Rust stores the marks
 * (settings_store/chat_read_marks.rs); this module renders them.
 *
 * Per chat it keeps two things: what the store last acknowledged (unknown,
 * a mark, or no mark) and the requests not yet answered. What the rail shows
 * is always derived from them: the newest pending request if there is one,
 * otherwise the acknowledged state. A refused request is simply dropped, so
 * the rail falls back to what the store holds.
 */
/** The store's bound: every chat the rail can show (MAX_RAIL_CHATS in Rust). */
export const MAX_CHAT_READ_MARKS = 1072;

/** A chat's read state: its mark, or `null` for no mark. */
type ReadState = number | null;

interface Request {
  state: ReadState;
  settle: (stored: boolean) => void;
}

/**
 * One lane per chat with a request outstanding, so the store applies a
 * chat's writes in the order they were made. It holds the request in flight
 * and one waiting; a newer request replaces the waiting one.
 */
interface Lane {
  inFlight: Request | null;
  next: Request | null;
}

const listeners = new Set<() => void>();
// The rendered projection: the known marks with pending requests laid
// over them, so at most twice the bound.
let marks = new Map<string, number>();
// What the store acknowledged: chats with a mark, and chats known to have
// none. Each is bounded on its own, so "no mark" entries never push out a
// stored mark. A chat in neither is unknown until the stored marks load and
// has no mark after that; a "no mark" entry that overflows simply falls
// back to that.
let known = new Map<string, number>();
let absent = new Set<string>();
const lanes = new Map<string, Lane>();
// Bumped on reset, so answers to an earlier session's requests are ignored.
let epoch = 0;
let restoring: Promise<void> | null = null;
let restored = false;
// A failed load is retried while the rail is mounted, backing off.
const RETRY_BASE_MS = 1_000;
const RETRY_MAX_MS = 30_000;
let loadFailures = 0;
// A read the store applies prunes its oldest mark, which a snapshot taken
// meanwhile still lists. Snapshots are only trusted when no read was in
// flight while they were taken; otherwise they are taken again once the
// reads land.
let readsInFlight = 0;
let readsSent = 0;
let reloadWanted = false;
let retryTimer: ReturnType<typeof setTimeout> | null = null;

function emit() {
  marks = new Map(marks);
  for (const listener of listeners) listener();
}

/** Records an acknowledged state; returns chats whose mark was evicted. */
function record(chatId: string, state: ReadState): string[] {
  if (state === null) {
    known.delete(chatId);
    absent.delete(chatId);
    absent.add(chatId);
    if (absent.size > MAX_CHAT_READ_MARKS)
      absent.delete(absent.values().next().value!);
    return [];
  }
  absent.delete(chatId);
  known.delete(chatId);
  known.set(chatId, state);
  const evicted: string[] = [];
  while (known.size > MAX_CHAT_READ_MARKS) {
    const oldest = known.keys().next().value!;
    known.delete(oldest);
    evicted.push(oldest);
  }
  return evicted;
}

function acknowledged(chatId: string): ReadState | undefined {
  if (known.has(chatId)) return known.get(chatId)!;
  if (absent.has(chatId)) return null;
  return restored || !isTauriRuntime() ? null : undefined;
}

function acknowledge(chatId: string, state: ReadState) {
  // The store prunes its oldest marks the same way.
  for (const evicted of record(chatId, state)) project(evicted);
  project(chatId);
}

/** Re-derives what the rail shows for the chat; emits when it changes. */
function project(chatId: string) {
  const lane = lanes.get(chatId);
  const pending = lane?.next ?? lane?.inFlight;
  const state = pending ? pending.state : (acknowledged(chatId) ?? null);
  if (state === null) {
    if (marks.delete(chatId)) emit();
  } else if (marks.get(chatId) !== state) {
    marks.set(chatId, state);
    emit();
  }
}

/** Re-derives every chat. */
function reproject() {
  marks = new Map(known);
  for (const [chatId, lane] of lanes) {
    const pending = lane.next ?? lane.inFlight;
    if (!pending) continue;
    if (pending.state === null) marks.delete(chatId);
    else marks.set(chatId, pending.state);
  }
  emit();
}

function admitMark(mark: unknown): [string, number] | null {
  if (typeof mark !== "object" || mark === null) return null;
  const { chatId, readThrough } = mark as Record<string, unknown>;
  return typeof chatId === "string" &&
    chatId.length > 0 &&
    typeof readThrough === "number" &&
    Number.isSafeInteger(readThrough) &&
    readThrough >= 0
    ? [chatId, readThrough]
    : null;
}

/**
 * Load the stored marks once. They fill only chats whose state is still
 * unknown: an answer that already arrived is at least as new. A chat the
 * snapshot does not list has no mark.
 */
export function restoreChatReadMarks(): Promise<void> {
  if (restored || !isTauriRuntime()) return Promise.resolve();
  if (restoring) return restoring;
  if (readsInFlight > 0) {
    reloadWanted = true;
    return Promise.resolve();
  }
  const started = epoch;
  const sentAtStart = readsSent;
  const load = invokeNative("chat_read_marks").then(
    (stored) => {
      if (started !== epoch) return;
      if (readsSent !== sentAtStart) {
        // A read may have pruned a mark this snapshot lists.
        reloadWanted = true;
        return;
      }
      const snapshot = (Array.isArray(stored) ? stored : [])
        .slice(0, MAX_CHAT_READ_MARKS)
        .flatMap((mark: unknown) => {
          const admitted = admitMark(mark);
          return admitted ? [admitted] : [];
        });
      // The stored marks are older than any answer that arrived before the
      // query: lay them down first (oldest first), then those answers, in
      // the order the store applied them.
      const answered = known;
      known = new Map();
      for (const [chatId, readThrough] of snapshot.reverse())
        if (!answered.has(chatId) && !absent.has(chatId))
          record(chatId, readThrough);
      for (const [chatId, readThrough] of answered) record(chatId, readThrough);
      restored = true;
      loadFailures = 0;
      reproject();
    },
    () => {
      if (started !== epoch) return;
      // Unknown states only show finished chats as Unread. Try again while
      // the rail is mounted, backing off.
      loadFailures += 1;
      if (listeners.size === 0 || retryTimer) return;
      retryTimer = setTimeout(
        () => {
          retryTimer = null;
          void restoreChatReadMarks();
        },
        Math.min(RETRY_BASE_MS * 2 ** (loadFailures - 1), RETRY_MAX_MS),
      );
    },
  );
  restoring = load.then(() => {
    if (started !== epoch) return;
    restoring = null;
    if (reloadWanted && readsInFlight === 0) {
      reloadWanted = false;
      return restoreChatReadMarks();
    }
  });
  return restoring;
}

/** The finish time through which the chat has been seen, if it has been. */
export function chatReadThrough(chatId: string): number | undefined {
  return marks.get(chatId);
}

function request(chatId: string, state: ReadState): Promise<boolean> {
  if (!isTauriRuntime()) {
    acknowledge(chatId, state);
    return Promise.resolve(true);
  }
  let lane = lanes.get(chatId);
  // Bounded like the marks: refuse rather than hold another chat's request.
  if (!lane && lanes.size >= MAX_CHAT_READ_MARKS) return Promise.resolve(false);
  return new Promise((settle) => {
    const created = !lane;
    lane ??= { inFlight: null, next: null };
    // The waiting request is replaced; the newer one decides.
    lane.next?.settle(true);
    lane.next = { state, settle };
    if (created) {
      lanes.set(chatId, lane);
      void drain(chatId, lane, epoch);
    }
    project(chatId);
  });
}

async function drain(chatId: string, lane: Lane, started: number) {
  while (lane.next) {
    const current = (lane.inFlight = lane.next);
    lane.next = null;
    let stored = true;
    let evicted: string[] = [];
    try {
      if (current.state === null) {
        await invokeNative("chat_mark_unread", { chatId });
        if (started !== epoch) return;
        evicted = record(chatId, null);
      } else {
        readsInFlight += 1;
        readsSent += 1;
        let answer: unknown;
        try {
          answer = await invokeNative("chat_mark_read", {
            mark: { chatId, readThrough: current.state },
          });
        } finally {
          if (started === epoch) readsInFlight -= 1;
        }
        if (started !== epoch) return;
        // The store keeps the later mark; take the one it reports.
        evicted = record(chatId, admitMark(answer)?.[1] ?? current.state);
      }
    } catch {
      if (started !== epoch) return;
      stored = false;
    }
    lane.inFlight = null;
    if (!lane.next) lanes.delete(chatId);
    // The store prunes its oldest marks the same way.
    for (const pruned of evicted) project(pruned);
    project(chatId);
    if (reloadWanted && readsInFlight === 0 && !restoring) {
      reloadWanted = false;
      void restoreChatReadMarks();
    }
    // A refusal matters only when no newer request followed.
    current.settle(stored || lane.next !== null);
  }
}

/** Records that the chat was seen through `readThrough`; never moves back. */
export function markChatRead(chatId: string, readThrough: number) {
  if (!Number.isSafeInteger(readThrough) || readThrough < 0) return;
  const shown = marks.get(chatId);
  if (shown !== undefined && shown >= readThrough) return;
  void request(chatId, readThrough);
}

/**
 * Forgets the mark, so the chat's latest Turn shows as Unread. Resolves
 * false when the store refuses and no newer request followed; the rail then
 * shows what the store holds, and the caller rolls back what it showed.
 */
export function markChatUnread(chatId: string): Promise<boolean> {
  return request(chatId, null);
}

export function resetChatReadMarks() {
  epoch += 1;
  marks = new Map();
  known = new Map();
  absent = new Set();
  lanes.clear();
  restoring = null;
  restored = false;
  loadFailures = 0;
  readsInFlight = 0;
  readsSent = 0;
  reloadWanted = false;
  if (retryTimer) clearTimeout(retryTimer);
  retryTimer = null;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Re-renders the caller whenever a mark changes. */
export function useChatReadMarks(): ReadonlyMap<string, number> {
  useEffect(() => void restoreChatReadMarks(), []);
  return useSyncExternalStore(
    subscribe,
    () => marks,
    () => marks,
  );
}
