import type { TabKind } from "../domain/chat";
import { useWorkbenchStore } from "./store";

/** The tabs the Editor strip holds, which Close Editor may close. */
export const isEditorTabKind = (kind: TabKind) =>
  kind === "file" || kind === "diff" || kind === "settings";

/**
 * Closes the Editor tab that is active when it runs, as its close button
 * does: a tab with unsaved changes asks first.
 */
export function closeActiveEditor(): void {
  const state = useWorkbenchStore.getState();
  const tab = state.tabs.find((item) => item.id === state.activeTabId);
  if (tab && isEditorTabKind(tab.kind)) state.closeTab(tab.id);
}
