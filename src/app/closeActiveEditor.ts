import type { TabKind } from "../domain/chat";
import { useWorkbenchStore } from "./store";

export const isEditorTabKind = (kind: TabKind) =>
  kind === "file" || kind === "diff" || kind === "settings";

export function closeActiveEditor(): void {
  const state = useWorkbenchStore.getState();
  const tab = state.tabs.find((item) => item.id === state.activeTabId);
  if (tab && isEditorTabKind(tab.kind)) state.closeTab(tab.id);
}
