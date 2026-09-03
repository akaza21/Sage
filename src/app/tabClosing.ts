import type { StoreApi } from "zustand";

import type { WorkbenchState } from "./store";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "./agentArtifactFollowStore";
import { closeTab as closeTabState } from "./tabState";

export function closeWorkbenchTab(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  tabId: string,
  preferredNextTabId: string | null = null,
) {
  if (
    get().projectTransitioning ||
    get().fileRenameInFlight ||
    get().resourceTrashInFlight
  )
    return;
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
  if (result.tabs.length < state.tabs.length) {
    useAgentArtifactAdmissionStore.getState().forget(tabId);
    useAgentArtifactFollowStore.getState().forgetTab(tabId);
  }
  const activeTabId =
    tabId === state.activeTabId &&
    preferredNextTabId !== null &&
    result.tabs.some((item) => item.id === preferredNextTabId)
      ? preferredNextTabId
      : result.activeTabId;
  const next = result.tabs.find((item) => item.id === activeTabId);
  if (tabId === state.activeTabId && next?.kind === "chat") {
    void get().activateChat(next.id, result.tabs);
    return;
  }
  set({
    tabs: result.tabs,
    activeTabId,
    selectedResource: next?.resource ?? state.selectedResource,
    selectedRootId: next?.resource?.rootId ?? state.selectedRootId,
    activeEditor:
      tabId === state.activeTabId && tab?.kind === "file"
        ? null
        : state.activeEditor,
  });
}
