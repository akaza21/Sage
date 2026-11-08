// Loaded on first use: resolving a change on disk and Save All are rare, so
// they stay out of the initial bundle.
import type { StoreApi } from "zustand";

import { errorMessage } from "../domain/errors";
import {
  parentResource,
  resourceKey,
  resourceTitle,
  sameResource,
} from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import { rebaseBufferOnDisk } from "../features/files/editorConflictResolution";
import {
  getFileEditorController,
  type ActiveEditorState,
  type EditorStateBridge,
} from "../features/files/fileEditorController";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { SaveAllFailure } from "./editorConflictSlice";
import { enqueueEditorOperation } from "./editorOperations";
import { storeEditorBridge, tabsWithEditorUpdate } from "./editorSlice";
import { matchesActiveTabIntent, matchesProjectIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

type ReadyEditor = Extract<ActiveEditorState, { phase: "ready" }>;
type Set = StoreApi<WorkbenchState>["setState"];
type Get = StoreApi<WorkbenchState>["getState"];

export const CHANGED_ON_DISK = "It changed on disk since it was opened.";

/**
 * Lets go of the buffer of a File that no longer exists. The native discard
 * reads the File first, so it fails here and the recovery buffer stays until
 * the path exists again; the tab is clean either way.
 */
export async function dropDeletedBuffer(
  bridge: EditorStateBridge,
  editor: ReadyEditor,
): Promise<void> {
  try {
    await getWorkbenchPorts().editor.discardEditorBuffer(editor.resource);
  } catch {
    // Expected while the File is missing; see above.
  }
  bridge.setActiveEditor(
    { ...editor, saving: false },
    { dirty: false, tabStatus: null },
  );
}

/**
 * Creates the deleted File again, empty, and puts the kept buffer on it, so
 * the save that follows writes the buffer through the ordinary path.
 */
export async function recreateDeletedFile(
  state: Pick<
    WorkbenchState,
    "project" | "projectGeneration" | "nativeTransitionEpoch"
  >,
  bridge: EditorStateBridge,
  editor: ReadyEditor,
): Promise<void> {
  if (!state.project) throw new Error("No project is open.");
  bridge.setActiveEditor({ ...editor, saving: true, message: null });
  const ports = getWorkbenchPorts();
  await ports.project.createProjectFile({
    projectId: state.project.id,
    projectGeneration: state.projectGeneration,
    expectedTransitionEpoch: state.nativeTransitionEpoch,
    parent: parentResource(editor.resource),
    basename: resourceTitle(editor.resource),
  });
  const document = await rebaseBufferOnDisk(ports.editor, {
    resource: editor.resource,
    content: editor.content,
    lineEnding: editor.document.lineEnding,
    revision: editor.revision,
  });
  bridge.setActiveEditor(getFileEditorController().ready(document), {
    dirty: document.dirty,
    tabStatus: null,
  });
}

/** "Keep mine", as `keepActiveEditorBuffer` describes it. */
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
      if (active.document.conflict.kind === "deleted") {
        bridge.setActiveEditor(
          {
            ...active,
            document: { ...active.document, conflict: null },
            message: null,
            recreateOnSave: true,
          },
          { dirty: true, tabStatus: null },
        );
        return true;
      }
      bridge.setActiveEditor({ ...active, saving: true, message: null });
      const document = await rebaseBufferOnDisk(getWorkbenchPorts().editor, {
        resource: active.resource,
        content: active.content,
        lineEnding: active.document.lineEnding,
        revision: active.revision,
      });
      if (!canCommit(active.resource)) return false;
      bridge.setActiveEditor(controller.ready(document), {
        dirty: document.dirty,
        tabStatus: document.conflict ? "failed" : null,
      });
      return true;
    });
  } catch (error) {
    if (matchesProjectIntent(get(), generation, projectId)) {
      controller.saveError(bridge, error);
    }
    return false;
  }
}

/** Saves a File that is not in the editor from its native buffer. */
async function saveBackgroundFile(set: Set, resource: ResourceRef) {
  const result = await enqueueEditorOperation(() =>
    getWorkbenchPorts().editor.saveEditorDocument(resource),
  );
  const conflict = result.outcome === "conflict";
  set((state) => ({
    tabs: tabsWithEditorUpdate(state.tabs, resource, {
      dirty: result.document.dirty,
      tabStatus: conflict ? "failed" : null,
    }),
  }));
  if (conflict) throw new Error(CHANGED_ON_DISK);
}

function activeSaveFailure(get: Get) {
  const editor = get().activeEditor;
  if (editor?.phase !== "ready") return "It could not be saved.";
  if (editor.document.conflict) return CHANGED_ON_DISK;
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
