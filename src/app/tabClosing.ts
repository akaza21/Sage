import type { StoreApi } from "zustand";

import type { WorkbenchState } from "./store";
import { closeTab as closeTabState } from "./tabState";

export function closeWorkbenchTab(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  tabId: string,
) {
  if (get().projectTransitioning) return;
  const state = get();
  const tab = state.tabs.find((candidate) => candidate.id === tabId);
  if (tab?.kind === "chat") {
    void get().archiveChat(tabId);
    return;
  }
  if (tab?.dirty) {
    set({ pendingCloseTabId: tabId });
    return;
  }
  const result = closeTabState(state.tabs, state.activeTabId ?? "", tabId);
  const next = result.tabs.find((item) => item.id === result.activeTabId);
  if (tabId === state.activeTabId && next?.kind === "chat") {
    void get().activateChat(next.id, result.tabs);
    return;
  }
  set({
    tabs: result.tabs,
    activeTabId: result.activeTabId,
    selectedResource: next?.resource ?? state.selectedResource,
    selectedRootId: next?.resource?.rootId ?? state.selectedRootId,
    activeEditor:
      tabId === state.activeTabId && tab?.kind === "file"
        ? null
        : state.activeEditor,
  });
}
