import { useSyncExternalStore } from "react";

interface ArchiveUndo {
  projectId: string;
  chatId: string;
  title: string;
}
const listeners = new Set<() => void>();
let state: { undo: ArchiveUndo | null } = { undo: null };
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
export function setAgentRailArchiveUndo(
  undo: ArchiveUndo | null,
  expected?: ArchiveUndo,
) {
  if (expected && state.undo !== expected) return;
  state = { undo };
  publish();
}
export function resetAgentRailInteractions() {
  state = { undo: null };
  publish();
}
