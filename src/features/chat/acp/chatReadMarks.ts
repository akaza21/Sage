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
const writing = new Map<string, number>();
// Chats marked unread before the stored marks loaded; their stored marks
// are stale.
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
  if ((saved.get(chatId) ?? -1) >= through) return;
  if ((writing.get(chatId) ?? -1) >= through) return;
  writing.set(chatId, through);
  void invokeNative("chat_mark_read", {
    mark: { chatId, readThrough: through },
  })
    .then(() => {
      if (marks.has(chatId))
        saved.set(chatId, Math.max(saved.get(chatId) ?? 0, through));
    })
    .catch(() => {
      // Not acknowledged: the next view of this chat writes it again.
    })
    .finally(() => {
      if (writing.get(chatId) === through) writing.delete(chatId);
    });
}

/** Forgets the mark, so the chat's latest Turn shows as Unread. */
export function markChatUnread(chatId: string) {
  saved.delete(chatId);
  if (!restored) unreadBeforeLoad.add(chatId);
  if (marks.delete(chatId)) emit();
  if (!isTauriRuntime()) return;
  void invokeNative("chat_mark_unread", { chatId }).catch(() => {});
}

export function resetChatReadMarks() {
  marks = new Map();
  saved = new Map();
  writing.clear();
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
