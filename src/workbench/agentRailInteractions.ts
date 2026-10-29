import { useSyncExternalStore } from "react";

interface ArchiveUndo {
  projectId: string;
  chatId: string;
  title: string;
}
const listeners = new Set<() => void>();
let state: {
  unread: ReadonlySet<string>;
  approval: ReadonlySet<string>;
  undo: ArchiveUndo | null;
} = { unread: new Set(), approval: new Set(), undo: null };
function publish() {
  for (const listener of listeners) listener();
}
export function useAgentRailInteractions() {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => state,
  );
}
export function markAgentRailUnread(chatId: string) {
  const unread = new Set(state.unread);
  if (unread.size >= 64) unread.delete(unread.values().next().value!);
  unread.add(chatId);
  state = { ...state, unread };
  publish();
}
export function clearAgentRailUnread(chatId?: string) {
  if (chatId && !state.unread.has(chatId)) return;
  const unread = new Set(state.unread);
  if (chatId) unread.delete(chatId);
  else unread.clear();
  state = { ...state, unread };
  publish();
}
export function setAgentRailArchiveUndo(
  undo: ArchiveUndo | null,
  expected?: ArchiveUndo,
) {
  if (expected && state.undo !== expected) return;
  state = { ...state, undo };
  publish();
}
export function setAgentRailApproval(chatId: string, pending: boolean) {
  if (state.approval.has(chatId) === pending) return;
  const approval = new Set(state.approval);
  if (!pending) approval.delete(chatId);
  else if (approval.size < 64) approval.add(chatId);
  state = { ...state, approval };
  publish();
}
export function resetAgentRailInteractions() {
  state = { unread: new Set(), approval: new Set(), undo: null };
  publish();
}
