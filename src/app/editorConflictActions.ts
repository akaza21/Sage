// Loaded on first use: resolving a change on disk and Save All are rare, so
// they stay out of the initial bundle.
import type { StoreApi } from "zustand";

import { errorMessage } from "../domain/errors";
import {
  parentResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import { ABSENT_FINGERPRINT, type ResourceRef } from "../domain/workbench";
import { contentText } from "../features/files/editorContent";
import { actionFailed } from "../features/files/editorNoticeState";
import { getFileEditorController } from "../features/files/fileEditorController";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { SaveAllFailure } from "./editorConflictSlice";
import { enqueueEditorOperation } from "./editorOperations";
import { storeEditorBridge, tabsWithEditorUpdate } from "./editorSlice";
import { matchesActiveTabIntent, matchesProjectIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

type Set = StoreApi<WorkbenchState>["setState"];
type Get = StoreApi<WorkbenchState>["getState"];

export const CHANGED_ON_DISK = "It changed on disk since it was opened.";
export const DELETED_ON_DISK =
  "It was deleted on disk. Keep your version to create it again, or close it.";

/**
 * "Keep mine": puts the active buffer on top of the File as the banner showed
 * it on disk (`document.disk`). If disk has moved on since, the buffer stays
 * as it is and the banner shows the File as it is now.
 */
export async function keepActiveEditorBuffer(
  set: Set,
  get: Get,
): Promise<boolean> {
  const controller = getFileEditorController();
  const bridge = storeEditorBridge(set, get);
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  if (!projectId || get().projectTransitioning) return false;
  const canCommit = (resource: ResourceRef) =>
    !get().projectTransitioning &&
    matchesActiveTabIntent(get(), generation, projectId, "file", resource);
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
        // The banner showed this version; a deletion has none.
        expectedDisk:
          active.document.conflict.kind === "deleted"
            ? { fingerprint: ABSENT_FINGERPRINT, byteLen: 0, modifiedAt: null }
            : active.document.disk,
        lineEnding: active.document.lineEnding,
        content: contentText(active.content),
        revision: Math.max(1, active.revision),
      });
      if (!canCommit(active.resource)) return false;
      // A File that came back holding the editor's own text: nothing to keep.
      if (result.outcome === "conflict" && result.document.conflict) {
        const { disk, conflict } = result.document;
        bridge.setActiveEditor(
          {
            ...active,
            saving: false,
            document: {
              ...active.document,
              disk,
              conflict,
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
    if (matchesProjectIntent(get(), generation, projectId)) {
      set({ error: actionFailed(bridge, error, "keep") });
    }
    return false;
  }
}

/** Saves a File that is not in the editor from its native buffer. */
async function saveBackgroundFile(set: Set, resource: ResourceRef) {
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

function activeSaveFailure(get: Get) {
  const editor = get().activeEditor;
  if (editor?.phase !== "ready") return "It could not be saved.";
  const conflict = editor.document.conflict;
  if (conflict?.kind === "deleted") return DELETED_ON_DISK;
  if (conflict) return CHANGED_ON_DISK;
  return editor.message ?? "It could not be saved.";
}

/**
 * Saves every dirty File tab in tab order: the active one through the
 * editor, the others from their native buffers. A failure is recorded and the
 * rest still save.
 */
export async function saveAllEditors(
  set: Set,
  get: Get,
): Promise<SaveAllFailure[]> {
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
        failures.push({ resource, message: activeSaveFailure(get) });
      }
      continue;
    }
    try {
      await saveBackgroundFile(set, resource);
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
