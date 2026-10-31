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

type GetState = StoreApi<WorkbenchState>["getState"];
type SetState = StoreApi<WorkbenchState>["setState"];

export const UNSAVED_FILE_REASON =
  "Save or revert this File before renaming or recycling it.";

/** The tabs released for a move, as they were, to settle them afterwards. */
export interface ReleasedFile {
  projectId: string;
  /** Where the store stood at release; a failed move that moved these
   * may still have changed the layout natively. */
  projectGeneration: number;
  transitionEpoch: number;
  tabs: WorkbenchTab[];
  /** The released tab that was active, if one was. */
  activeId: string | null;
  /** The tab closing the active one would have activated instead. */
  neighbourId: string | null;
}

/** How the move ended, for `settleReleasedFile`. */
export type ReleaseOutcome =
  | { kind: "failed" }
  | { kind: "renamed"; target: ResourceRef }
  | { kind: "gone" };

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

/** The same conditions under which rename and recycle refuse to start. */
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
 * tabs are taken out first and settled afterwards. Returns null when nothing
 * is open, or when the move couldn't start anyway: then the action itself
 * reports why.
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

/**
 * After a failed move the same tabs come back. After a rename the File tab
 * comes back under the new name, where it stood and pinned as before; Diff
 * tabs don't. When the File is gone, or the active tab didn't come back,
 * the tab closing it would have activated becomes active.
 */
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
  // A Diff tab of the File was active: the File it showed takes its place.
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
    // Only the tab is added: the active tab and the selection stay.
    return update.tabs ? { tabs: placed(update.tabs) } : {};
  });
}

/** Runs a move of `source` with its clean open tabs released around it. */
export async function moveReleasingFile(
  get: GetState,
  set: SetState,
  source: ResourceRef,
  move: () => Promise<boolean>,
  moved: () => ReleaseOutcome,
  reportUnsaved: () => void,
): Promise<boolean> {
  const released = await releaseCleanFile(get, set, source);
  if (released === "unsaved") {
    reportUnsaved();
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
