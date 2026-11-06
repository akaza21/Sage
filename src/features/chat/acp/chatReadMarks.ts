import { useEffect, useSyncExternalStore } from "react";

import { invokeNative, isTauriRuntime } from "../../../ports/nativeInvoke";

/**
 * Which chat updates the user has seen, so a finished chat stays read after
 * a reload or a restart. Rust stores the marks
 * (settings_store/chat_read_marks.rs); this module holds their rendered
 * projection and writes each newer mark once.
 */
export const MAX_CHAT_READ_MARKS = 512;

const listeners = new Set<() => void>();
// Insertion order is read order, so the oldest mark is evicted first.
let marks = new Map<string, number>();
// The last state the store acknowledged for a chat: its mark, or null once
// a delete landed. A write the store refuses leaves the projection showing
// this. Bounded like the marks; a forgotten entry only means "unknown".
let acked = new Map<string, number | null>();

type Write = { kind: "read"; readThrough: number } | { kind: "unread" };
interface Request {
  write: Write;
  sequence: number;
  // True unless the store refused it and no newer request followed.
  settle: (stored: boolean) => void;
}
interface Lane {
  // The newest request's sequence; only its refusal rolls anything back.
  latest: number;
  // The request waiting behind the one in flight; a newer one replaces it.
  next: Request | null;
}
// One lane per chat with a write in flight, so the store applies a chat's
// writes in the order they were made. A lane holds one write in flight and
// one waiting, and goes away when it drains.
const lanes = new Map<string, Lane>();
let sequence = 0;
// Chats marked unread whose delete the store may not have applied when the
// stored marks were read: the projection skips their stored marks. Bounded
// like the marks: insertion order evicts the oldest.
let unreadBeforeLoad = new Set<string>();
let restoring: Promise<void> | null = null;
let restored = false;

function emit() {
  marks = new Map(marks);
  for (const listener of listeners) listener();
}

function bound() {
  while (marks.size > MAX_CHAT_READ_MARKS)
    marks.delete(marks.keys().next().value!);
}

function remember(chatId: string, readThrough: number) {
  marks.delete(chatId);
  marks.set(chatId, readThrough);
  bound();
}

function acknowledge(chatId: string, state: number | null) {
  acked.delete(chatId);
  acked.set(chatId, state);
  while (acked.size > MAX_CHAT_READ_MARKS)
    acked.delete(acked.keys().next().value!);
}

function admitMarks(input: unknown): Array<[string, number]> {
  if (!Array.isArray(input)) return [];
  return input
    .slice(0, MAX_CHAT_READ_MARKS)
    .flatMap((mark: unknown): Array<[string, number]> => {
      if (typeof mark !== "object" || mark === null) return [];
      const { chatId, readThrough } = mark as Record<string, unknown>;
      return typeof chatId === "string" &&
        chatId.length > 0 &&
        typeof readThrough === "number" &&
        Number.isSafeInteger(readThrough) &&
        readThrough >= 0
        ? [[chatId, readThrough]]
        : [];
    });
}

/**
 * Load the stored marks once. They are older than any mark made meanwhile,
 * so they are laid down first and the newer local marks replayed on top;
 * only then is the bound applied, so the oldest stored marks give way.
 */
export function restoreChatReadMarks(): Promise<void> {
  if (restored || !isTauriRuntime()) return Promise.resolve();
  restoring ??= invokeNative("chat_read_marks")
    .then((stored) => {
      restored = true;
      const local = marks;
      const merged = new Map<string, number>();
      // The store lists the most recent first; replay oldest first.
      for (const [chatId, readThrough] of admitMarks(stored).reverse()) {
        // An acknowledgement that already arrived is at least as new.
        if (!acked.has(chatId)) acknowledge(chatId, readThrough);
        if (!unreadBeforeLoad.has(chatId)) merged.set(chatId, readThrough);
      }
      for (const [chatId, readThrough] of local) {
        const storedThrough = merged.get(chatId);
        merged.delete(chatId);
        merged.set(chatId, Math.max(storedThrough ?? 0, readThrough));
      }
      unreadBeforeLoad = new Set();
      marks = merged;
      bound();
      emit();
    })
    .catch(() => {
      // Unreadable marks only show finished chats as Unread; a later
      // render tries again.
    })
    .finally(() => {
      restoring = null;
    });
  return restoring;
}

/** The finish time through which the chat has been seen, if it has been. */
export function chatReadThrough(chatId: string): number | undefined {
  return marks.get(chatId);
}

function enqueue(chatId: string, write: Write): Promise<boolean> {
  return new Promise((settle) => {
    const request = { write, sequence: (sequence += 1), settle };
    const lane = lanes.get(chatId);
    if (lane) {
      lane.next?.settle(true);
      lane.next = request;
      lane.latest = request.sequence;
      return;
    }
    const opened = { latest: request.sequence, next: request };
    lanes.set(chatId, opened);
    void drain(chatId, opened);
  });
}

async function drain(chatId: string, lane: Lane) {
  while (lane.next) {
    const { write, sequence: requested, settle } = lane.next;
    lane.next = null;
    try {
      if (write.kind === "read") {
        await invokeNative("chat_mark_read", {
          mark: { chatId, readThrough: write.readThrough },
        });
        acknowledge(chatId, write.readThrough);
      } else {
        await invokeNative("chat_mark_unread", { chatId });
        acknowledge(chatId, null);
        // A snapshot read after the delete cannot hold the mark.
        if (!restoring) unreadBeforeLoad.delete(chatId);
      }
      settle(true);
    } catch {
      if (requested !== lane.latest) {
        // A newer request decides what the chat shows.
        settle(true);
        continue;
      }
      if (write.kind === "unread") unreadBeforeLoad.delete(chatId);
      showAcknowledged(chatId);
      settle(false);
    }
  }
  if (lanes.get(chatId) === lane) lanes.delete(chatId);
}

/** The newest request was refused: show what the store last acknowledged. */
function showAcknowledged(chatId: string) {
  if (!acked.has(chatId)) return; // Unknown until the stored marks load.
  const state = acked.get(chatId)!;
  if (state === null) {
    if (marks.delete(chatId)) emit();
  } else if (marks.get(chatId) !== state) {
    remember(chatId, state);
    emit();
  }
}

/** Records that the chat was seen through `readThrough`; never moves back. */
export function markChatRead(chatId: string, readThrough: number) {
  if (!Number.isSafeInteger(readThrough) || readThrough < 0) return;
  const current = marks.get(chatId);
  if (current === undefined || current < readThrough) {
    remember(chatId, readThrough);
    unreadBeforeLoad.delete(chatId);
    emit();
  }
  const through = marks.get(chatId)!;
  if (!isTauriRuntime()) return;
  // While a write is queued, what the store will hold is not yet known.
  if (!lanes.has(chatId) && (acked.get(chatId) ?? -1) >= through) return;
  if (!lanes.has(chatId) && lanes.size >= MAX_CHAT_READ_MARKS) return;
  // A refused read shows the acknowledged state, so the next view of this
  // chat writes it again.
  void enqueue(chatId, { kind: "read", readThrough: through });
}

/**
 * Forgets the mark, so the chat's latest Turn shows as Unread. Resolves
 * false when the store refuses and no newer request for the chat followed:
 * the projection then shows what the store last acknowledged, and the
 * caller rolls back anything it showed for the request.
 */
export function markChatUnread(chatId: string): Promise<boolean> {
  // Too many writes in flight: refuse before showing anything.
  if (
    isTauriRuntime() &&
    !lanes.has(chatId) &&
    lanes.size >= MAX_CHAT_READ_MARKS
  )
    return Promise.resolve(false);
  if (!restored) {
    unreadBeforeLoad.delete(chatId);
    unreadBeforeLoad.add(chatId);
    while (unreadBeforeLoad.size > MAX_CHAT_READ_MARKS)
      unreadBeforeLoad.delete(unreadBeforeLoad.values().next().value!);
  }
  if (marks.delete(chatId)) emit();
  if (!isTauriRuntime()) return Promise.resolve(true);
  return enqueue(chatId, { kind: "unread" });
}

export function resetChatReadMarks() {
  marks = new Map();
  acked = new Map();
  lanes.clear();
  unreadBeforeLoad = new Set();
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
