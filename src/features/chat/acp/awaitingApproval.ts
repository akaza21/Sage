import { useSyncExternalStore } from "react";

// Written only by mounted chat surfaces and cleared on unmount, so it stays
// bounded by the visible panes.
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
