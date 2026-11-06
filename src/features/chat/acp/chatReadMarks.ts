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
export const MAX_CHAT_READ_MARKS = 512;

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
// The rendered projection. Insertion order is read order.
let marks = new Map<string, number>();
// Known states. A chat missing here is unknown until the stored marks load,
// and has no mark after that.
let acked = new Map<string, ReadState>();
const lanes = new Map<string, Lane>();
// Bumped on reset, so answers to an earlier session's requests are ignored.
let epoch = 0;
let restoring: Promise<void> | null = null;
let restored = false;

function emit() {
  marks = new Map(marks);
  for (const listener of listeners) listener();
}

function touch<V>(map: Map<string, V>, chatId: string, value: V) {
  map.delete(chatId);
  map.set(chatId, value);
  while (map.size > MAX_CHAT_READ_MARKS) map.delete(map.keys().next().value!);
}

function acknowledged(chatId: string): ReadState | undefined {
  if (acked.has(chatId)) return acked.get(chatId)!;
  return restored || !isTauriRuntime() ? null : undefined;
}

/** Re-derives what the rail shows for the chat; emits when it changes. */
function project(chatId: string) {
  const lane = lanes.get(chatId);
  const pending = lane?.next ?? lane?.inFlight;
  const state = pending ? pending.state : (acknowledged(chatId) ?? null);
  if (state === null) {
    if (marks.delete(chatId)) emit();
  } else if (marks.get(chatId) !== state) {
    touch(marks, chatId, state);
    emit();
  }
}

/** Re-derives every chat, keeping the newest requests within the bound. */
function reproject() {
  const next = new Map<string, number>();
  for (const [chatId, state] of acked)
    if (state !== null) next.set(chatId, state);
  for (const [chatId, lane] of lanes) {
    const pending = lane.next ?? lane.inFlight;
    if (!pending) continue;
    next.delete(chatId);
    if (pending.state !== null) next.set(chatId, pending.state);
  }
  marks = new Map();
  for (const [chatId, state] of next) touch(marks, chatId, state);
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
  const started = epoch;
  restoring ??= invokeNative("chat_read_marks")
    .then((stored) => {
      if (started !== epoch) return;
      const snapshot = (Array.isArray(stored) ? stored : [])
        .slice(0, MAX_CHAT_READ_MARKS)
        .flatMap((mark: unknown) => {
          const admitted = admitMark(mark);
          return admitted ? [admitted] : [];
        });
      // The stored marks are older than any answer that arrived meanwhile:
      // lay them down first (oldest first), then those answers, and only
      // then apply the bound.
      const merged = new Map<string, ReadState>();
      for (const [chatId, readThrough] of snapshot.reverse())
        if (!acked.has(chatId)) merged.set(chatId, readThrough);
      for (const [chatId, state] of acked) merged.set(chatId, state);
      acked = new Map();
      for (const [chatId, state] of merged) touch(acked, chatId, state);
      restored = true;
      reproject();
    })
    .catch(() => {
      // Unknown states only show finished chats as Unread; a later render
      // tries again.
    })
    .finally(() => {
      if (started === epoch) restoring = null;
    });
  return restoring;
}

/** The finish time through which the chat has been seen, if it has been. */
export function chatReadThrough(chatId: string): number | undefined {
  return marks.get(chatId);
}

function request(chatId: string, state: ReadState): Promise<boolean> {
  if (!isTauriRuntime()) {
    touch(acked, chatId, state);
    project(chatId);
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
    try {
      if (current.state === null) {
        await invokeNative("chat_mark_unread", { chatId });
        if (started !== epoch) return;
        touch(acked, chatId, null);
      } else {
        const answer = await invokeNative("chat_mark_read", {
          mark: { chatId, readThrough: current.state },
        });
        if (started !== epoch) return;
        // The store keeps the later mark; take the one it reports.
        touch(acked, chatId, admitMark(answer)?.[1] ?? current.state);
      }
    } catch {
      if (started !== epoch) return;
      stored = false;
    }
    lane.inFlight = null;
    if (!lane.next) lanes.delete(chatId);
    project(chatId);
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
  acked = new Map();
  lanes.clear();
  restoring = null;
  restored = false;
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
