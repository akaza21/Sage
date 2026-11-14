import { useEffect, useSyncExternalStore } from "react";

import { useWorkbenchStore, type WorkbenchState } from "../../../app/store";
import { invokeNative, isTauriRuntime } from "../../../ports/nativeInvoke";

// Rust stores the marks (settings_store/chat_read_marks.rs); this module only
// renders them.
// Bounds what the store does not: pending requests, known-absent chats, and
// every mark outside the desktop app.
export const MAX_HELD_READ_STATES = 512;

type ReadState = number | null;

interface Request {
  state: ReadState;
  settle: (stored: boolean) => void;
}

// Serializes a chat's writes so the store applies them in order.
interface Lane {
  inFlight: Request | null;
  next: Request | null;
}

const listeners = new Set<() => void>();
let marks = new Map<string, number>();
// Bounded separately from `absent`, so "no mark" entries never push out a
// stored mark.
let known = new Map<string, number>();
let absent = new Set<string>();
const lanes = new Map<string, Lane>();
// Answers for different chats can arrive out of order, so an older change is
// ignored.
let sequences = new Map<string, number>();
let capacity: number | null = null;
// Bumped on reset, so answers to an earlier session's requests are ignored.
let epoch = 0;
let restoring: Promise<void> | null = null;
let restored = false;
const RETRY_BASE_MS = 1_000;
const RETRY_MAX_MS = 30_000;
let loadFailures = 0;
// A read may evict a mark that a concurrent snapshot still lists, so a snapshot
// taken while reads are in flight is retaken.
let readsInFlight = 0;
let readsSent = 0;
let reloadWanted = false;
let retryTimer: ReturnType<typeof setTimeout> | null = null;

function emit() {
  marks = new Map(marks);
  for (const listener of listeners) listener();
}

function knownBound() {
  return capacity ?? MAX_HELD_READ_STATES;
}

// The store decides evictions; this trim only guards memory against an answer
// that breaks that contract.
function record(chatId: string, state: ReadState) {
  if (state === null) {
    known.delete(chatId);
    absent.delete(chatId);
    absent.add(chatId);
    if (absent.size > MAX_HELD_READ_STATES)
      absent.delete(absent.values().next().value!);
    return;
  }
  absent.delete(chatId);
  known.delete(chatId);
  known.set(chatId, state);
  if (known.size > knownBound()) known.delete(known.keys().next().value!);
}

function newer(chatId: string, sequence: number | undefined): boolean {
  if (sequence === undefined) return true;
  const seen = sequences.get(chatId);
  if (seen !== undefined && seen >= sequence) return false;
  sequences.delete(chatId);
  sequences.set(chatId, sequence);
  if (sequences.size > knownBound() + MAX_HELD_READ_STATES)
    sequences.delete(sequences.keys().next().value!);
  return true;
}

function acknowledged(chatId: string): ReadState | undefined {
  if (known.has(chatId)) return known.get(chatId)!;
  if (absent.has(chatId)) return null;
  return restored || !isTauriRuntime() ? null : undefined;
}

function acknowledge(chatId: string, state: ReadState) {
  record(chatId, state);
  project(chatId);
}

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

function admitCapacity(answer: unknown) {
  if (typeof answer !== "object" || answer === null) return;
  const reported = (answer as Record<string, unknown>).capacity;
  if (
    typeof reported === "number" &&
    Number.isSafeInteger(reported) &&
    reported > 0
  )
    capacity = reported;
}

function admitEvicted(answer: unknown): string[] {
  if (typeof answer !== "object" || answer === null) return [];
  const { evicted } = answer as Record<string, unknown>;
  if (!Array.isArray(evicted)) return [];
  return evicted
    .slice(0, knownBound())
    .filter(
      (chatId): chatId is string =>
        typeof chatId === "string" && chatId.length > 0,
    );
}

function admitSequence(answer: unknown): number | undefined {
  if (typeof answer !== "object" || answer === null) return undefined;
  const { sequence } = answer as Record<string, unknown>;
  return typeof sequence === "number" && Number.isSafeInteger(sequence)
    ? sequence
    : undefined;
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

// An answer that already arrived is at least as new as the snapshot.
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
      admitCapacity(stored);
      const listed =
        typeof stored === "object" && stored !== null
          ? (stored as Record<string, unknown>).marks
          : null;
      const snapshot = (Array.isArray(listed) ? listed : [])
        .slice(0, knownBound())
        .flatMap((mark: unknown) => {
          const admitted = admitMark(mark);
          return admitted ? [admitted] : [];
        });
      // The stored marks predate any answer that arrived before the query, so
      // they go down first.
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
  if (!lane && lanes.size >= MAX_HELD_READ_STATES)
    return Promise.resolve(false);
  return new Promise((settle) => {
    const created = !lane;
    lane ??= { inFlight: null, next: null };
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
        record(chatId, null);
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
        // Drop the evicted marks unless a later write already superseded them.
        admitCapacity(answer);
        const sequence = admitSequence(answer);
        evicted = admitEvicted(answer).filter(
          (pruned) => pruned !== chatId && newer(pruned, sequence),
        );
        for (const pruned of evicted) record(pruned, null);
        if (newer(chatId, sequence))
          record(chatId, admitMark(answer)?.[1] ?? current.state);
      }
    } catch {
      if (started !== epoch) return;
      stored = false;
    }
    lane.inFlight = null;
    if (!lane.next) lanes.delete(chatId);
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

export function markChatRead(chatId: string, readThrough: number) {
  if (!Number.isSafeInteger(readThrough) || readThrough < 0) return;
  const shown = marks.get(chatId);
  if (shown !== undefined && shown >= readThrough) return;
  void request(chatId, readThrough);
}

// Resolves false when the store refuses and no newer request followed; the
// caller then rolls back.
export function markChatUnread(chatId: string): Promise<boolean> {
  return request(chatId, null);
}

function archivedChatIds(state: WorkbenchState): Set<string> {
  const ids = new Set(state.archivedChats.map((chat) => chat.id));
  for (const project of state.projects)
    for (const chat of project.railArchived ?? []) ids.add(chat.id);
  return ids;
}

// Archiving also drops the mark in the store (chat_archive_commands.rs).
useWorkbenchStore.subscribe((state, previous) => {
  if (
    state.archivedChats === previous.archivedChats &&
    state.projects === previous.projects
  )
    return;
  const before = archivedChatIds(previous);
  for (const chatId of archivedChatIds(state)) {
    if (before.has(chatId)) continue;
    record(chatId, null);
    project(chatId);
  }
});

export function resetChatReadMarks() {
  epoch += 1;
  marks = new Map();
  known = new Map();
  absent = new Set();
  sequences = new Map();
  capacity = null;
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

export function useChatReadMarks(): ReadonlyMap<string, number> {
  useEffect(() => void restoreChatReadMarks(), []);
  return useSyncExternalStore(
    subscribe,
    () => marks,
    () => marks,
  );
}
