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
// What the store has acknowledged, so a failed write is retried on the next
// view instead of being hidden by the rendered mark.
let saved = new Map<string, number>();

type Write = { kind: "read"; readThrough: number } | { kind: "unread" };
type Outcome = "saved" | "refused" | "superseded";
interface Lane {
  // The write waiting behind the one in flight; a newer one replaces it.
  next: { write: Write; settle: (outcome: Outcome) => void } | null;
}
// One lane per chat with a write in flight, so the store applies a chat's
// writes in the order they were made and a stale acknowledgement never
// lands after a newer write. A lane holds one write in flight and one
// waiting, and goes away when it drains.
const lanes = new Map<string, Lane>();
// Chats marked unread whose delete the store may not have applied when the
// stored marks were read; their stored marks are stale. Bounded like the
// marks: insertion order evicts the oldest.
let unreadBeforeLoad = new Set<string>();
let restoring: Promise<void> | null = null;
let restored = false;

function emit() {
  marks = new Map(marks);
  for (const listener of listeners) listener();
}

function bound() {
  while (marks.size > MAX_CHAT_READ_MARKS) {
    const oldest = marks.keys().next().value!;
    marks.delete(oldest);
    saved.delete(oldest);
  }
}

function remember(chatId: string, readThrough: number) {
  marks.delete(chatId);
  marks.set(chatId, readThrough);
  bound();
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
        if (unreadBeforeLoad.has(chatId)) continue;
        merged.set(chatId, readThrough);
        saved.set(chatId, Math.max(saved.get(chatId) ?? 0, readThrough));
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

function enqueue(chatId: string, write: Write): Promise<Outcome> {
  return new Promise((settle) => {
    let lane = lanes.get(chatId);
    if (!lane) {
      // Bounded like the marks; a refused read is written on the next view.
      if (lanes.size >= MAX_CHAT_READ_MARKS) return settle("refused");
      lane = { next: null };
      lanes.set(chatId, lane);
      lane.next = { write, settle };
      void drain(chatId, lane);
      return;
    }
    lane.next?.settle("superseded");
    lane.next = { write, settle };
  });
}

async function drain(chatId: string, lane: Lane) {
  while (lane.next) {
    const { write, settle } = lane.next;
    lane.next = null;
    try {
      if (write.kind === "read") {
        await invokeNative("chat_mark_read", {
          mark: { chatId, readThrough: write.readThrough },
        });
        // A write queued behind this one decides what the store holds.
        if (!lane.next && marks.has(chatId))
          saved.set(
            chatId,
            Math.max(saved.get(chatId) ?? 0, write.readThrough),
          );
      } else {
        await invokeNative("chat_mark_unread", { chatId });
        saved.delete(chatId);
      }
      settle("saved");
    } catch {
      settle("refused");
    }
  }
  if (lanes.get(chatId) === lane) lanes.delete(chatId);
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
  if (!lanes.has(chatId) && (saved.get(chatId) ?? -1) >= through) return;
  // Not acknowledged: the next view of this chat writes it again.
  void enqueue(chatId, { kind: "read", readThrough: through });
}

/**
 * Forgets the mark, so the chat's latest Turn shows as Unread. Resolves
 * false when the store refuses: the projection then shows the mark the store
 * still holds, and the caller rolls back anything it showed for the request.
 */
export function markChatUnread(chatId: string): Promise<boolean> {
  // Too many writes in flight: refuse before showing anything.
  if (
    isTauriRuntime() &&
    !lanes.has(chatId) &&
    lanes.size >= MAX_CHAT_READ_MARKS
  )
    return Promise.resolve(false);
  const previous = marks.get(chatId);
  saved.delete(chatId);
  if (!restored) {
    unreadBeforeLoad.delete(chatId);
    unreadBeforeLoad.add(chatId);
    while (unreadBeforeLoad.size > MAX_CHAT_READ_MARKS)
      unreadBeforeLoad.delete(unreadBeforeLoad.values().next().value!);
  }
  if (marks.delete(chatId)) emit();
  if (!isTauriRuntime()) return Promise.resolve(true);
  return enqueue(chatId, { kind: "unread" }).then((outcome) => {
    if (outcome === "saved") {
      // A snapshot read after the delete cannot hold the mark.
      if (!restoring) unreadBeforeLoad.delete(chatId);
      return true;
    }
    // A newer write for this chat replaced the request.
    if (outcome === "superseded") return true;
    unreadBeforeLoad.delete(chatId);
    // Nothing newer was recorded meanwhile: show what the store holds.
    if (previous !== undefined && !marks.has(chatId)) {
      remember(chatId, previous);
      emit();
    }
    return false;
  });
}

export function resetChatReadMarks() {
  marks = new Map();
  saved = new Map();
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
