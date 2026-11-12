import { resourceKey } from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import {
  acquireNotifyListingSlot,
  isIgnoreRuleFile,
  releaseNotifyListingSlot,
  WORKSPACE_NOTIFY_EVENT,
  syncLiveProjectWorkspaceNotify,
  workspaceNotifyBatchNotices,
  type LiveProjectNotifyStamp,
  type WorkspaceNotifyHooks,
  type WorkspaceNotifyNotice,
} from "../features/files/workspaceNotifyCompose";
import { invokeNative, isTauriRuntime } from "../ports/nativeInvoke";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";
import { errorMessage } from "../domain/errors";

type CloseStage =
  | "closeRequested"
  | "editorFlush"
  | "appearanceFlush"
  | "workspaceUnwatch"
  | "persistAndDestroy";
type CloseStageStatus = "start" | "end" | "error";
let closeTraceQueue = Promise.resolve();

export function traceCloseStage(
  stage: CloseStage,
  status: CloseStageStatus,
): void {
  if (!isTauriRuntime()) return;
  closeTraceQueue = closeTraceQueue
    .then(() => invokeNative<void>("trace_close_stage", { stage, status }))
    .catch(() => undefined);
}

async function runCloseStage(
  stage: CloseStage,
  work: () => Promise<void>,
): Promise<void> {
  traceCloseStage(stage, "start");
  try {
    await work();
    traceCloseStage(stage, "end");
  } catch (error) {
    traceCloseStage(stage, "error");
    const detail = errorMessage(error);
    throw new Error(`Close ${stage} failed: ${detail}`, { cause: error });
  }
}

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

/** Reloads the root's ready or expanded listings that `within` admits. */
async function reloadRootListings(
  get: () => WorkbenchState,
  rootId: string,
  within: (relativePath: string) => boolean,
): Promise<void> {
  const seen = new Set<string>();
  const resources: ResourceRef[] = [];
  const add = (resource: ResourceRef) => {
    const key = resourceKey(resource);
    if (
      resource.rootId !== rootId ||
      !within(resource.relativePath) ||
      seen.has(key)
    ) {
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
      await reloadRootListings(get, notice.rootId, () => true);
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
      // Changes inside an ignored folder: its listings and those below.
      if (notice.kind === "ignored") {
        const folder = notice.relativePath;
        return reloadRootListings(
          get,
          notice.rootId,
          (path) => path === folder || path.startsWith(`${folder}/`),
        );
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

/**
 * The close path holds and flushes Editor settings through this hook, so the
 * settings store and its catalogue load with the Workbench instead of in the
 * startup bundle. The store binds itself when its module loads; until then
 * nothing can be pending, so holding and flushing do nothing.
 */
interface EditorSettingsClose {
  hold(): () => void;
  flush(): Promise<void>;
}

let editorSettingsClose: EditorSettingsClose | null = null;

export function bindEditorSettingsClose(close: EditorSettingsClose) {
  editorSettingsClose = close;
}

export async function closeLiveWorkbench(
  get: () => WorkbenchState,
): Promise<void> {
  const releaseSettings = editorSettingsClose?.hold() ?? (() => undefined);
  try {
    await runCloseStage("editorFlush", () => get().flushActiveEditor());
    await runCloseStage(
      "appearanceFlush",
      () => editorSettingsClose?.flush() ?? Promise.resolve(),
    );
    await runCloseStage("workspaceUnwatch", unbindWorkspaceNotify);
    await runCloseStage("persistAndDestroy", () =>
      getWorkbenchPorts().project.persistProjectLayoutAndClose(
        currentProjectLayout(get()),
        get().nativeTransitionEpoch,
      ),
    );
  } finally {
    releaseSettings();
  }
}
