import type { StoreApi } from "zustand";

import { agentDiffKey } from "../domain/agentDiffIdentity";
import { sameResource } from "../domain/resourceIdentity";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "./agentArtifactFollowStore";
import { resourceTabUpdate } from "./resourceTabs";
import type { WorkbenchState } from "./store";

type GetState = StoreApi<WorkbenchState>["getState"];
type SetState = StoreApi<WorkbenchState>["setState"];

export const UNSAVED_FILE_REASON =
  "Save or revert this File before renaming or recycling it.";

/** Where an open File's tab stood before it was released. */
export interface ReleasedFile {
  position: number;
  pinned: boolean;
  active: boolean;
}

/** File and Diff tabs that show exactly this File. */
function tabsShowing(tabs: readonly WorkbenchTab[], source: ResourceRef) {
  return tabs.filter(
    (tab) =>
      (tab.kind === "file" || tab.kind === "diff") &&
      sameResource(tab.resource, source),
  );
}

export function fileHasUnsavedChanges(
  state: Pick<WorkbenchState, "tabs">,
  source: ResourceRef,
): boolean {
  return tabsShowing(state.tabs, source).some((tab) => tab.dirty);
}

/**
 * Native rename and recycle act only on closed Files, so a clean open File's
 * tabs are closed first and its File tab is put back afterwards, as VS Code
 * keeps a renamed File open. Diff tabs are not put back. The selection stays
 * on the File, which rename and recycle require.
 */
export function releaseCleanFile(
  get: GetState,
  set: SetState,
  source: ResourceRef,
): ReleasedFile | null | "unsaved" {
  const state = get();
  const tabs = tabsShowing(state.tabs, source);
  // While busy, tabs can't close; the action itself then says why it can't run.
  const busy =
    state.projectTransitioning ||
    state.fileRenameInFlight ||
    state.resourceTrashInFlight;
  if (tabs.length === 0 || busy) return null;
  if (tabs.some((tab) => tab.dirty)) return "unsaved";
  const fileTab = tabs.find((tab) => tab.kind === "file");
  const released = fileTab
    ? {
        position: fileTab.position,
        pinned: fileTab.pinned,
        active: fileTab.id === state.activeTabId,
      }
    : null;
  // Removed directly rather than through closeTab, which would activate a
  // neighbouring tab (perhaps a Chat) only for the File to come back.
  const ids = new Set(tabs.map((tab) => tab.id));
  for (const id of ids) {
    useAgentArtifactAdmissionStore.getState().forget(id);
    useAgentArtifactFollowStore.getState().forgetTab(id);
  }
  set((current) => {
    const diffs = { ...current.diffs };
    for (const tab of tabs) {
      if (tab.diffRef) delete diffs[agentDiffKey(tab.diffRef)];
    }
    const activeReleased = ids.has(current.activeTabId ?? "");
    return {
      tabs: current.tabs.filter((tab) => !ids.has(tab.id)),
      activeTabId: activeReleased ? null : current.activeTabId,
      activeEditor: activeReleased ? null : current.activeEditor,
      selectedResource: source,
      selectedRootId: source.rootId,
      diffs,
    };
  });
  return released;
}

/**
 * Opens `resource` where the released tab stood, pinned if it was, and
 * active only if it was.
 */
export async function restoreReleasedFile(
  get: GetState,
  set: SetState,
  projectId: string,
  resource: ResourceRef,
  released: ReleasedFile,
): Promise<void> {
  if (get().project?.id !== projectId) return;
  const atPosition = (tabs: WorkbenchTab[]) =>
    tabs.map((tab) =>
      tab.kind === "file" && sameResource(tab.resource, resource)
        ? { ...tab, position: released.position, pinned: released.pinned }
        : tab,
    );
  if (released.active) {
    await get().openFile(resource);
    set((state) => ({ tabs: atPosition(state.tabs) }));
    return;
  }
  set((state) => {
    if (state.projectTransitioning || state.project?.id !== projectId) {
      return {};
    }
    const update = resourceTabUpdate(state, "file", resource);
    if (!update.tabs) return {};
    // Only the tab is added: the active tab and the selection stay.
    return { tabs: atPosition(update.tabs) };
  });
}
