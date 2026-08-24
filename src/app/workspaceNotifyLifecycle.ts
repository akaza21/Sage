import { resourceKey } from "../domain/resourceIdentity";
import { currentProjectLayout } from "./projectProjection";
import {
  WORKSPACE_NOTIFY_EVENT,
  publishWorkspaceNotifySearch,
  syncLiveProjectWorkspaceNotify,
  type LiveProjectNotifyStamp,
  type WorkspaceNotifyHooks,
} from "../features/files/workspaceNotifyCompose";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { WorkbenchState } from "./store";

export function liveProjectNotifyStamp(
  state: WorkbenchState,
): LiveProjectNotifyStamp | null {
  const project = state.project;
  if (!project) {
    return null;
  }
  const roots = project.roots
    .filter((root) => root.availability === "available")
    .map((root) => ({ rootId: root.id }));
  if (roots.length === 0) {
    return null;
  }
  return {
    projectId: project.id,
    projectGeneration: state.projectGeneration,
    transitionEpoch: state.nativeTransitionEpoch,
    roots,
  };
}

export function createWorkspaceNotifyHooks(
  get: () => WorkbenchState,
): WorkspaceNotifyHooks {
  return {
    watch(input) {
      return getWorkbenchPorts().project.watchWorkspaceNotify(input);
    },
    unwatch(input) {
      return getWorkbenchPorts().project.unwatchWorkspaceNotify(input);
    },
    async listen(emit) {
      if (!isTauriRuntime()) {
        return () => undefined;
      }
      const { listen } = await import("@tauri-apps/api/event");
      return listen(WORKSPACE_NOTIFY_EVENT, (event) => {
        emit(event.payload);
      });
    },
    invalidate(notice, listing) {
      publishWorkspaceNotifySearch(notice);
      const key = resourceKey(listing);
      if (get().directoryListings[key] || get().expandedDirectories[key]) {
        void get().loadDirectory(listing, true);
      }
    },
  };
}

export async function bindWorkspaceNotifyFromState(
  get: () => WorkbenchState,
): Promise<void> {
  await syncLiveProjectWorkspaceNotify(
    liveProjectNotifyStamp(get()),
    createWorkspaceNotifyHooks(get),
  );
}

export async function unbindWorkspaceNotify(): Promise<void> {
  await syncLiveProjectWorkspaceNotify(null, {
    watch: async () => ({ generation: 0 }),
    unwatch(input) {
      return getWorkbenchPorts().project.unwatchWorkspaceNotify(input);
    },
    listen: async () => () => undefined,
    invalidate() {
      /* no live listing refresh while unbound */
    },
  });
}

export async function closeLiveWorkbench(
  get: () => WorkbenchState,
): Promise<void> {
  await get().flushActiveEditor();
  await unbindWorkspaceNotify();
  await getWorkbenchPorts().project.persistProjectLayoutAndClose(
    currentProjectLayout(get()),
    get().nativeTransitionEpoch,
  );
}
