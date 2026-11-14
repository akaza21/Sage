import { resourceKey } from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import {
  acquireNotifyListingSlot,
  isIgnoreRuleFile,
  MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS,
  releaseNotifyListingSlot,
  WORKSPACE_NOTIFY_EVENT,
  syncLiveProjectWorkspaceNotify,
  workspaceNotifyBatchNotices,
  type LiveProjectNotifyStamp,
  type WorkspaceNotifyHooks,
  type WorkspaceNotifyNotice,
} from "../features/files/workspaceNotifyCompose";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { WorkbenchState } from "./store";
import { bindWorkspaceNotifyClose } from "./workbenchClose";

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

const resyncRoots = new Map<string, { dirty: boolean }>();

interface ListingRefreshBatch {
  resources: ResourceRef[];
  load(resource: ResourceRef): Promise<void>;
  complete(): void;
  cancel(): void;
}

// Root resyncs coalesce per root; the notice scheduler admits at most eight
// subtree refreshes. Each selection holds at most 64 cached + 64 expanded
// listings. Workers pull from those bounded selections, never fan them out
// into admission waiters. Unbind discards selections from the old session.
const listingRefreshBatches = new Set<ListingRefreshBatch>();
let listingRefreshWorkers = 0;

function discardListingRefreshes(): void {
  for (const batch of listingRefreshBatches) batch.cancel();
  listingRefreshBatches.clear();
  resyncRoots.clear();
}

function startListingRefreshWorkers(): void {
  while (
    listingRefreshWorkers < MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS &&
    listingRefreshBatches.size > 0
  ) {
    listingRefreshWorkers += 1;
    void (async () => {
      try {
        while (listingRefreshBatches.size > 0) {
          const batch = listingRefreshBatches.values().next().value!;
          const resource = batch.resources.shift()!;
          if (batch.resources.length === 0) listingRefreshBatches.delete(batch);
          try {
            await batch.load(resource);
          } catch {
            // Directory loads report native failures in listing state. An
            // unexpected rejection must not strand the remaining selections.
          } finally {
            batch.complete();
          }
        }
      } finally {
        listingRefreshWorkers -= 1;
      }
    })();
  }
}

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

async function reloadRootListings(
  get: () => WorkbenchState,
  rootId: string,
  subtree?: string,
): Promise<void> {
  const seen = new Set<string>();
  const resources: ResourceRef[] = [];
  const add = (resource: ResourceRef) => {
    const key = resourceKey(resource);
    if (
      resource.rootId !== rootId ||
      (subtree !== undefined &&
        resource.relativePath !== subtree &&
        !resource.relativePath.startsWith(`${subtree}/`)) ||
      seen.has(key)
    ) {
      return;
    }
    seen.add(key);
    resources.push(resource);
  };
  for (const [key, entry] of Object.entries(get().directoryListings)) {
    if (entry.phase === "ready") {
      add(entry.value.resource);
    } else if (entry.phase === "loading") {
      const resource = resourceFromKey(key);
      if (resource) add(resource);
    }
  }
  for (const key of Object.keys(get().expandedDirectories)) {
    const resource = resourceFromKey(key);
    if (resource) {
      add(resource);
    }
  }
  if (resources.length === 0) return;
  const { projectGeneration, nativeTransitionEpoch } = get();
  await new Promise<void>((resolve) => {
    let remaining = resources.length;
    const batch: ListingRefreshBatch = {
      resources,
      async load(resource) {
        if (
          get().projectGeneration === projectGeneration &&
          get().nativeTransitionEpoch === nativeTransitionEpoch
        ) {
          await admittedLoadDirectory(get, resource);
        }
      },
      complete() {
        if (--remaining === 0) resolve();
      },
      cancel() {
        remaining -= resources.length;
        resources.length = 0;
        if (remaining === 0) resolve();
      },
    };
    listingRefreshBatches.add(batch);
    startListingRefreshWorkers();
  });
}

async function refreshListingsForResync(
  get: () => WorkbenchState,
  notice: WorkspaceNotifyNotice,
): Promise<void> {
  const current = resyncRoots.get(notice.rootId);
  if (current) {
    current.dirty = true;
    return;
  }
  const entry = { dirty: false };
  resyncRoots.set(notice.rootId, entry);
  try {
    do {
      entry.dirty = false;
      await reloadRootListings(get, notice.rootId);
    } while (entry.dirty && resyncRoots.get(notice.rootId) === entry);
  } finally {
    if (resyncRoots.get(notice.rootId) === entry) {
      resyncRoots.delete(notice.rootId);
    }
  }
}

export function createWorkspaceNotifyHooks(
  get: () => WorkbenchState,
): WorkspaceNotifyHooks {
  return {
    dispose: discardListingRefreshes,
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
        for (const notice of workspaceNotifyBatchNotices(event.payload)) {
          emit(notice);
        }
      });
    },
    invalidate(notice, listing) {
      // A changed ignore file can change what Explorer dims in any listing
      // below it, so it refreshes the root's listings as a resync does.
      if (notice.kind === "resync" || isIgnoreRuleFile(notice.relativePath)) {
        return refreshListingsForResync(get, notice);
      }
      if (notice.kind === "ignored") {
        return reloadRootListings(get, notice.rootId, notice.relativePath);
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
  bindWorkspaceNotifyClose(unbindWorkspaceNotify);
  await syncLiveProjectWorkspaceNotify(
    liveProjectNotifyStamp(get()),
    createWorkspaceNotifyHooks(get),
  );
}

export async function unbindWorkspaceNotify(): Promise<void> {
  discardListingRefreshes();
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
