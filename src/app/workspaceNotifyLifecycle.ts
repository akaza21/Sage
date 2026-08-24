import { resourceKey } from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import {
  acquireNotifyListingSlot,
  releaseNotifyListingSlot,
  WORKSPACE_NOTIFY_EVENT,
  syncLiveProjectWorkspaceNotify,
  type LiveProjectNotifyStamp,
  type WorkspaceNotifyHooks,
  type WorkspaceNotifyNotice,
} from "../features/files/workspaceNotifyCompose";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { currentProjectLayout } from "./projectProjection";
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

function resourceFromKey(key: string): ResourceRef | null {
  const colon = key.indexOf(":");
  if (colon < 1) {
    return null;
  }
  const length = Number(key.slice(0, colon));
  if (!Number.isInteger(length) || length < 1) {
    return null;
  }
  const rest = key.slice(colon + 1);
  if (rest.length < length) {
    return null;
  }
  return {
    rootId: rest.slice(0, length),
    relativePath: rest.slice(length),
  };
}

const resyncRootsInFlight = new Set<string>();
const resyncRootsQueued = new Set<string>();

async function admittedLoadDirectory(
  get: () => WorkbenchState,
  resource: ResourceRef,
): Promise<void> {
  const acquired = await acquireNotifyListingSlot();
  if (!acquired) {
    return;
  }
  try {
    await get().loadDirectory(resource, true);
  } finally {
    releaseNotifyListingSlot();
  }
}

async function refreshListingsForResync(
  get: () => WorkbenchState,
  notice: WorkspaceNotifyNotice,
): Promise<void> {
  if (resyncRootsInFlight.has(notice.rootId)) {
    resyncRootsQueued.add(notice.rootId);
    return;
  }
  resyncRootsInFlight.add(notice.rootId);
  try {
    do {
      resyncRootsQueued.delete(notice.rootId);
      const seen = new Set<string>();
      const resources: ResourceRef[] = [];
      const add = (resource: ResourceRef) => {
        const key = resourceKey(resource);
        if (resource.rootId !== notice.rootId || seen.has(key)) {
          return;
        }
        seen.add(key);
        resources.push(resource);
      };
      for (const entry of Object.values(get().directoryListings)) {
        if (entry.phase === "ready") {
          add(entry.value.resource);
        }
      }
      for (const key of Object.keys(get().expandedDirectories)) {
        const resource = resourceFromKey(key);
        if (resource) {
          add(resource);
        }
      }
      await Promise.all(
        resources.map((resource) => admittedLoadDirectory(get, resource)),
      );
    } while (resyncRootsQueued.has(notice.rootId));
  } finally {
    resyncRootsInFlight.delete(notice.rootId);
  }
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
      if (notice.kind === "resync") {
        return refreshListingsForResync(get, notice);
      }
      const key = resourceKey(listing);
      if (get().directoryListings[key] || get().expandedDirectories[key]) {
        return admittedLoadDirectory(get, listing);
      }
      return undefined;
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
  resyncRootsInFlight.clear();
  resyncRootsQueued.clear();
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
