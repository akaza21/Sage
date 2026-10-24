import { useSyncExternalStore } from "react";

/**
 * Chats whose mounted surface currently shows a pending approval. Entries are
 * written only by a mounted chat surface and removed when it unmounts, so the
 * set is bounded by the visible chat panes and never claims a chat it cannot
 * see.
 */
let awaiting: ReadonlySet<string> = new Set();
const listeners = new Set<() => void>();

export function setChatAwaitingApproval(chatId: string, value: boolean) {
  if (awaiting.has(chatId) === value) return;
  const next = new Set(awaiting);
  if (value) next.add(chatId);
  else next.delete(chatId);
  awaiting = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useChatsAwaitingApproval(): ReadonlySet<string> {
  return useSyncExternalStore(subscribe, () => awaiting);
}
