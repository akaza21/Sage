// Loaded on first use: resolving a change on disk and Save All are rare, so
// they stay out of the initial bundle.
import { errorMessage } from "../domain/errors";
import {
  parentResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import { ABSENT_FINGERPRINT, type ResourceRef } from "../domain/workbench";
import { contentText } from "../features/files/editorContent";
import {
  actionFailed,
  EXTERNAL_CHANGE,
} from "../features/files/editorNoticeState";
import { getFileEditorController } from "../features/files/fileEditorController";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { SaveAllFailure } from "./editorConflictSlice";
import { enqueueEditorOperation } from "./editorOperations";
import { storeEditorBridge, tabsWithEditorUpdate } from "./editorSlice";
import { matchesActiveTabIntent, matchesProjectIntent } from "./projectIntent";
import { useWorkbenchStore } from "./store";

const set = useWorkbenchStore.setState;
const get = useWorkbenchStore.getState;

export const CHANGED_ON_DISK = "It changed on disk since it was opened.";
export const DELETED_ON_DISK =
  "It was deleted on disk. Keep your version to create it again, or close it.";

/**
 * An action that waited for this module to load, or in the queue, must never
 * land on a File the user has switched to since.
 */
function whileActive(resource: ResourceRef) {
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  return () =>
    !!projectId &&
    !get().projectTransitioning &&
    sameResource(get().activeEditor?.resource, resource) &&
    matchesActiveTabIntent(get(), generation, projectId, "file", resource);
}

export async function keepActiveEditorBuffer(
  resource: ResourceRef,
): Promise<boolean> {
  const controller = getFileEditorController();
  const bridge = storeEditorBridge(set, get);
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  const isActive = whileActive(resource);
  if (!isActive()) return false;
  const canCommit = (other: ResourceRef) =>
    sameResource(other, resource) && isActive();
  try {
    return await enqueueEditorOperation(async () => {
      const active = get().activeEditor;
      if (
        active?.phase !== "ready" ||
        !active.document.conflict ||
        active.saving ||
        !canCommit(active.resource)
      ) {
        return false;
      }
      bridge.setActiveEditor({ ...active, saving: true, message: null });
      const result = await getWorkbenchPorts().editor.rebaseEditorBuffer({
        resource: active.resource,
        expectedDisk:
          active.document.conflict.kind === "deleted"
            ? { fingerprint: ABSENT_FINGERPRINT, byteLen: 0, modifiedAt: null }
            : active.document.disk,
        lineEnding: active.document.lineEnding,
        content: contentText(active.content),
        revision: Math.max(1, active.revision),
      });
      if (!canCommit(active.resource)) return false;
      const current = get().activeEditor;
      if (current?.phase !== "ready") return false;
      // "matched" means disk holds the very text sent, which the editor still
      // holds: it cannot be edited while `saving`.
      if (result.outcome === "conflict") {
        const { disk, conflict } = result.document;
        bridge.setActiveEditor(
          {
            ...current,
            saving: false,
            document: {
              ...current.document,
              disk,
              conflict: conflict ?? EXTERNAL_CHANGE,
            },
          },
          { tabStatus: "failed" },
        );
        return false;
      }
      bridge.setActiveEditor(controller.ready(result.document), {
        dirty: result.document.dirty,
        tabStatus: null,
      });
      return true;
    });
  } catch (error) {
    if (projectId && matchesProjectIntent(get(), generation, projectId)) {
      set({ error: actionFailed(bridge, error, "keep") });
    }
    return false;
  }
}

export async function reloadActiveEditor(
  resource: ResourceRef,
): Promise<boolean> {
  const controller = getFileEditorController();
  const bridge = storeEditorBridge(set, get);
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  const isActive = whileActive(resource);
  if (!isActive()) return false;
  const canCommit = (other: ResourceRef) =>
    sameResource(other, resource) && isActive();
  try {
    return await enqueueEditorOperation(async () => {
      const document = await controller.discard(bridge, canCommit, true);
      const current = get().activeEditor;
      if (
        !document ||
        current?.phase !== "ready" ||
        !canCommit(document.resource)
      ) {
        return false;
      }
      if (document.disk.fingerprint === ABSENT_FINGERPRINT) {
        bridge.setActiveEditor(
          {
            ...current,
            document: {
              ...current.document,
              disk: document.disk,
              conflict: document.conflict,
            },
          },
          { tabStatus: document.conflict ? "failed" : null },
        );
        return false;
      }
      bridge.setActiveEditor(controller.ready(document), {
        dirty: document.dirty,
        tabStatus: null,
      });
      return true;
    });
  } catch (error) {
    if (projectId && matchesProjectIntent(get(), generation, projectId)) {
      controller.saveError(bridge, error);
      set({ error: errorMessage(error) });
    }
    return false;
  }
}

export async function closeEditorWithoutSaving(tabId: string) {
  if (get().activeTabId !== tabId) return;
  if (await get().discardActiveEditor()) get().closeTab(tabId);
}

async function saveBackgroundFile(resource: ResourceRef) {
  const markFailed = () =>
    set((state) => ({
      tabs: tabsWithEditorUpdate(state.tabs, resource, {
        tabStatus: "failed",
      }),
    }));
  let result;
  try {
    result = await enqueueEditorOperation(() =>
      getWorkbenchPorts().editor.saveEditorDocument(resource),
    );
  } catch (error) {
    markFailed();
    throw error;
  }
  const conflict = result.document.conflict;
  set((state) => ({
    tabs: tabsWithEditorUpdate(state.tabs, resource, {
      dirty: result.document.dirty,
      tabStatus: result.outcome === "conflict" ? "failed" : null,
    }),
  }));
  if (result.outcome === "conflict") {
    throw new Error(
      conflict?.kind === "deleted" ? DELETED_ON_DISK : CHANGED_ON_DISK,
    );
  }
}

function activeSaveFailure() {
  const editor = get().activeEditor;
  if (editor?.phase !== "ready") return "It could not be saved.";
  const conflict = editor.document.conflict;
  if (conflict?.kind === "deleted") return DELETED_ON_DISK;
  if (conflict) return CHANGED_ON_DISK;
  return editor.message ?? "It could not be saved.";
}

export async function saveAllEditors(): Promise<SaveAllFailure[]> {
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  if (!projectId || get().projectTransitioning) return [];
  const seen = new Set<string>();
  const dirty = get()
    .tabs.filter((tab) => tab.kind === "file" && tab.dirty && tab.resource)
    .sort((left, right) => left.position - right.position)
    .flatMap((tab) => {
      const key = resourceKey(tab.resource!);
      if (seen.has(key)) return [];
      seen.add(key);
      return [tab.resource!];
    });
  const failures: SaveAllFailure[] = [];
  const savedInBackground: ResourceRef[] = [];
  for (const resource of dirty) {
    if (
      get().projectTransitioning ||
      !matchesProjectIntent(get(), generation, projectId)
    ) {
      return failures;
    }
    const active = get().activeEditor;
    if (active?.phase === "ready" && sameResource(active.resource, resource)) {
      if (!(await get().saveActiveEditor())) {
        failures.push({ resource, message: activeSaveFailure() });
      }
      continue;
    }
    try {
      await saveBackgroundFile(resource);
      savedInBackground.push(resource);
    } catch (error) {
      failures.push({ resource, message: errorMessage(error) });
    }
  }
  if (!matchesProjectIntent(get(), generation, projectId)) return failures;
  set({ saveAllReport: failures.length ? { projectId, failures } : null });
  if (savedInBackground.length) {
    const parents = new Map(
      savedInBackground.map((resource) => {
        const parent = parentResource(resource);
        return [resourceKey(parent), parent] as const;
      }),
    );
    set((state) => {
      const directoryListings = { ...state.directoryListings };
      const diffs = { ...state.diffs };
      for (const key of parents.keys()) delete directoryListings[key];
      for (const resource of savedInBackground) {
        delete diffs[resourceKey(resource)];
      }
      return { directoryListings, diffs };
    });
    await Promise.all([
      get().refreshRepository(),
      ...[...parents.values()].map((parent) => get().loadDirectory(parent)),
    ]);
  }
  return failures;
}
