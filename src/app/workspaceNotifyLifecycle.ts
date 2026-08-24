import { resourceKey } from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import {
  MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS,
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

function refreshListingsForResync(
  get: () => WorkbenchState,
  notice: WorkspaceNotifyNotice,
): Promise<void> {
  const seen = new Set<string>();
  const loads: Array<Promise<void>> = [];
  const add = (resource: ResourceRef) => {
    if (
      resource.rootId !== notice.rootId ||
      seen.has(resourceKey(resource)) ||
      loads.length >= MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS
    ) {
      return;
    }
    seen.add(resourceKey(resource));
    loads.push(Promise.resolve(get().loadDirectory(resource, true)));
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
  return Promise.all(loads).then(() => undefined);
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
        return get().loadDirectory(listing, true);
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
