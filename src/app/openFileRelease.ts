import type { StoreApi } from "zustand";

import { agentDiffKey } from "../domain/agentDiffIdentity";
import {
  parentResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "./agentArtifactFollowStore";
import { resourceTabUpdate } from "./resourceTabs";
import type { WorkbenchState } from "./store";
import { orderedTabs } from "./tabState";
import {
  fileHasUnsavedChanges,
  tabsShowing,
  UNSAVED_FILE_REASON,
} from "./unsavedFile";

type GetState = StoreApi<WorkbenchState>["getState"];
type SetState = StoreApi<WorkbenchState>["setState"];

export interface ReleasedFile {
  projectId: string;
  /** A failed move may still have changed the layout natively. */
  projectGeneration: number;
  transitionEpoch: number;
  tabs: WorkbenchTab[];
  activeId: string | null;
  neighbourId: string | null;
}

export type ReleaseOutcome =
  | { kind: "failed" }
  | { kind: "renamed"; target: ResourceRef }
  | { kind: "gone" };

function moveCannotStart(state: WorkbenchState, source: ResourceRef) {
  const root = state.project?.roots.find((item) => item.id === source.rootId);
  const parent = state.directoryListings[resourceKey(parentResource(source))];
  return (
    root?.availability !== "available" ||
    state.projectTransitioning ||
    state.fileCreateInFlight ||
    state.folderCreateInFlight ||
    state.fileRenameInFlight ||
    state.resourceTrashInFlight ||
    state.repositoryRefreshing ||
    state.repositoryMutation !== null ||
    state.repositoryCommit !== null ||
    !sameResource(state.selectedResource, source) ||
    parent?.phase !== "ready" ||
    parent.value.truncated
  );
}

function neighbourOf(
  tabs: WorkbenchTab[],
  activeId: string,
  gone: Set<string>,
) {
  const ordered = orderedTabs(tabs);
  const at = ordered.findIndex((tab) => tab.id === activeId);
  const after = ordered.slice(at + 1).find((tab) => !gone.has(tab.id));
  const before = ordered
    .slice(0, at)
    .reverse()
    .find((tab) => !gone.has(tab.id));
  return (after ?? before)?.id ?? null;
}

/**
 * Native rename and recycle act only on closed Files, so a clean open File's
 * tabs are taken out first and settled afterwards.
 */
export async function releaseCleanFile(
  get: GetState,
  set: SetState,
  source: ResourceRef,
): Promise<ReleasedFile | null | "unsaved"> {
  const projectId = get().project?.id;
  if (!projectId || tabsShowing(get().tabs, source).length === 0) return null;
  if (moveCannotStart(get(), source)) return null;
  if (fileHasUnsavedChanges(get(), source)) return "unsaved";
  // Settles the editor's saved buffer, which may still hold edits that were
  // undone back to the text on disk.
  if (sameResource(get().activeEditor?.resource, source)) {
    try {
      await get().flushActiveEditor();
    } catch {
      return null;
    }
  }
  const state = get();
  const tabs = tabsShowing(state.tabs, source);
  if (
    state.project?.id !== projectId ||
    tabs.length === 0 ||
    moveCannotStart(state, source)
  ) {
    return null;
  }
  if (tabs.some((tab) => tab.dirty)) return "unsaved";
  const ids = new Set(tabs.map((tab) => tab.id));
  const activeId = ids.has(state.activeTabId ?? "") ? state.activeTabId : null;
  // Taken out directly rather than through closeTab, which would activate a
  // neighbouring tab (perhaps a Chat) only for the File to come back.
  set({
    tabs: state.tabs.filter((tab) => !ids.has(tab.id)),
    activeTabId: activeId ? null : state.activeTabId,
    activeEditor: activeId ? null : state.activeEditor,
    selectedResource: source,
    selectedRootId: source.rootId,
  });
  return {
    projectId,
    projectGeneration: state.projectGeneration,
    transitionEpoch: state.nativeTransitionEpoch,
    tabs,
    activeId,
    neighbourId: activeId ? neighbourOf(state.tabs, activeId, ids) : null,
  };
}

export async function settleReleasedFile(
  get: GetState,
  set: SetState,
  released: ReleasedFile,
  outcome: ReleaseOutcome,
): Promise<void> {
  if (get().project?.id !== released.projectId) return;
  // Put back only if nothing else moved; otherwise native may have committed
  // a layout without these tabs, or even the move itself.
  const unchanged =
    get().projectGeneration === released.projectGeneration &&
    get().nativeTransitionEpoch === released.transitionEpoch;
  if (outcome.kind === "failed" && unchanged) {
    set((state) => ({
      tabs: [
        ...state.tabs,
        ...released.tabs.filter(
          (tab) => !state.tabs.some((item) => item.id === tab.id),
        ),
      ],
    }));
    if (released.activeId) await get().activateTab(released.activeId);
    return;
  }
  for (const tab of released.tabs) {
    useAgentArtifactAdmissionStore.getState().forget(tab.id);
    useAgentArtifactFollowStore.getState().forgetTab(tab.id);
  }
  set((state) => {
    const diffs = { ...state.diffs };
    for (const tab of released.tabs) {
      if (tab.diffRef) delete diffs[agentDiffKey(tab.diffRef)];
    }
    return { diffs };
  });
  const fileTab = released.tabs.find((tab) => tab.kind === "file");
  if (outcome.kind === "renamed" && fileTab) {
    await reopenAt(
      get,
      set,
      outcome.target,
      fileTab,
      released.activeId === fileTab.id,
    );
  }
  const reopened = get().tabs.find(
    (tab) =>
      outcome.kind === "renamed" &&
      tab.kind === "file" &&
      sameResource(tab.resource, outcome.target),
  );
  if (!released.activeId || (reopened && get().activeTabId === reopened.id)) {
    return;
  }
  if (reopened) {
    await get().activateTab(reopened.id);
    return;
  }
  const neighbour = get().tabs.find((tab) => tab.id === released.neighbourId);
  // The native layout may have made another tab active meanwhile, so a Chat
  // is activated directly, which also brings its session along.
  if (neighbour?.kind === "chat") await get().activateChat(neighbour.id);
  else if (neighbour) await get().activateTab(neighbour.id);
}

async function reopenAt(
  get: GetState,
  set: SetState,
  resource: ResourceRef,
  was: WorkbenchTab,
  active: boolean,
) {
  const placed = (tabs: WorkbenchTab[]) =>
    tabs.map((tab) =>
      tab.kind === "file" && sameResource(tab.resource, resource)
        ? { ...tab, position: was.position, pinned: was.pinned }
        : tab,
    );
  if (active) {
    await get().openFile(resource);
    set((state) => ({ tabs: placed(state.tabs) }));
    return;
  }
  set((state) => {
    if (state.projectTransitioning) return {};
    const update = resourceTabUpdate(state, "file", resource);
    return update.tabs ? { tabs: placed(update.tabs) } : {};
  });
}

/**
 * The stores load this module only when a move releases tabs, which keeps it
 * off the startup path.
 */
export async function moveReleasingFile(
  get: GetState,
  set: SetState,
  source: ResourceRef,
  move: () => Promise<boolean>,
  moved: () => ReleaseOutcome,
  reportUnsaved: (reason: string) => void,
): Promise<boolean> {
  const released = await releaseCleanFile(get, set, source);
  if (released === "unsaved") {
    reportUnsaved(UNSAVED_FILE_REASON);
    return false;
  }
  const ok = await move();
  if (released) {
    await settleReleasedFile(
      get,
      set,
      released,
      ok ? moved() : { kind: "failed" },
    );
  }
  return ok;
}
