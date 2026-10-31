import type { StoreApi } from "zustand";

import type { WorkbenchState } from "./store";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "./agentArtifactFollowStore";
import { closeTab as closeTabState } from "./tabState";
import { agentDiffKey } from "../domain/agentDiffIdentity";
import { closeBrowser } from "../ports/nativeBrowserPort";
import { pinnedAgentChatOwner } from "./agentChatOwner";
import { useAcpProjectionStore } from "./acpProjectionStore";
import { isAcpProjectionPinned } from "./acpProjectionRegistry";

export const BROWSER_CLOSE_ERROR =
  "Could not close the browser tab. Try again.";

const browserClosesInFlight = new Set<string>();
const browsersAlreadyClosed = new Set<string>();

export function consumeBrowserAlreadyClosed(tabId: string): boolean {
  return browsersAlreadyClosed.delete(tabId);
}

export function isBrowserCloseInFlight(tabId: string): boolean {
  return browserClosesInFlight.has(tabId);
}

export function reconcileNativeClosedBrowserTabs(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  closedIds: string[],
) {
  const state = get();
  let tabs = state.tabs;
  let activeTabId: string | null = state.activeTabId;
  for (const id of closedIds) {
    const tab = tabs.find((candidate) => candidate.id === id);
    if (tab?.kind !== "browser") continue;
    browsersAlreadyClosed.add(id);
    useAgentArtifactAdmissionStore.getState().forget(id);
    useAgentArtifactFollowStore.getState().forgetTab(id);
    const result = closeTabState(tabs, activeTabId ?? "", id);
    tabs = result.tabs;
    activeTabId = result.activeTabId;
  }
  if (tabs === state.tabs) return;
  const next = tabs.find((tab) => tab.id === activeTabId);
  set({
    tabs,
    activeTabId,
    selectedResource: next?.resource ?? state.selectedResource,
    selectedRootId: next?.resource?.rootId ?? state.selectedRootId,
    activeEditor: state.activeTabId === activeTabId ? state.activeEditor : null,
  });
  if (
    next?.kind === "chat" &&
    state.activeTabId !== activeTabId &&
    !isAcpProjectionPinned(next.id) &&
    pinnedAgentChatOwner() !== next.id &&
    useAcpProjectionStore.getState().scope?.chatId !== next.id
  ) {
    void get().activateChat(next.id, tabs);
  }
}

export function closeWorkbenchTab(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  tabId: string,
  preferredNextTabId: string | null = null,
  nativeBrowserClosed = false,
) {
  if (
    !nativeBrowserClosed &&
    (get().projectTransitioning ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight)
  )
    return;
  const state = get();
  const tab = state.tabs.find((candidate) => candidate.id === tabId);
  if (nativeBrowserClosed && tab?.kind === "browser") {
    browsersAlreadyClosed.add(tabId);
  }
  if (
    tab?.kind === "browser" &&
    tab.status !== "closed" &&
    !nativeBrowserClosed
  ) {
    if (browserClosesInFlight.has(tabId)) return;
    browserClosesInFlight.add(tabId);
    void closeBrowser(tabId)
      .then(() => {
        browserClosesInFlight.delete(tabId);
        browsersAlreadyClosed.add(tabId);
        closeWorkbenchTab(set, get, tabId, preferredNextTabId, true);
        browsersAlreadyClosed.delete(tabId);
        set((current) => ({
          error: current.error === BROWSER_CLOSE_ERROR ? null : current.error,
        }));
      })
      .catch(() => {
        browserClosesInFlight.delete(tabId);
        set({
          error: BROWSER_CLOSE_ERROR,
        });
      });
    return;
  }
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
    if (tab?.diffRef) {
      const diffs = { ...get().diffs };
      delete diffs[agentDiffKey(tab.diffRef)];
      set({ diffs });
    }
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
