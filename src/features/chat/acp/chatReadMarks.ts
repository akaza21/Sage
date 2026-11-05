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
let restoring: Promise<void> | null = null;
let restored = false;

function emit() {
  marks = new Map(marks);
  for (const listener of listeners) listener();
}

function remember(chatId: string, readThrough: number) {
  marks.delete(chatId);
  marks.set(chatId, readThrough);
  while (marks.size > MAX_CHAT_READ_MARKS)
    marks.delete(marks.keys().next().value!);
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

/** Load the stored marks once; a mark made meanwhile keeps the later time. */
export function restoreChatReadMarks(): Promise<void> {
  if (restored || !isTauriRuntime()) return Promise.resolve();
  restoring ??= invokeNative("chat_read_marks")
    .then((stored) => {
      restored = true;
      // The store lists the most recent first; replay oldest first.
      for (const [chatId, readThrough] of admitMarks(stored).reverse()) {
        const current = marks.get(chatId);
        if (current === undefined) remember(chatId, readThrough);
        else marks.set(chatId, Math.max(current, readThrough));
      }
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

/** The update time through which the chat has been seen, if it has been. */
export function chatReadThrough(chatId: string): number | undefined {
  return marks.get(chatId);
}

/** Records that the chat was seen through `readThrough`; never moves back. */
export function markChatRead(chatId: string, readThrough: number) {
  if (!Number.isSafeInteger(readThrough) || readThrough < 0) return;
  const current = marks.get(chatId);
  if (current !== undefined && current >= readThrough) return;
  remember(chatId, readThrough);
  emit();
  if (!isTauriRuntime()) return;
  void invokeNative("chat_mark_read", {
    mark: { chatId, readThrough },
  }).catch(() => {
    // The mark stays for this session; the next view writes it again.
  });
}

/** Forgets the mark, so the chat's latest update shows as Unread. */
export function markChatUnread(chatId: string) {
  if (marks.delete(chatId)) emit();
  if (!isTauriRuntime()) return;
  void invokeNative("chat_mark_unread", { chatId }).catch(() => {});
}

export function resetChatReadMarks() {
  marks = new Map();
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
