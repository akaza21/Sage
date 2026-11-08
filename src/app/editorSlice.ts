import { type StateCreator, type StoreApi } from "zustand";
import { errorMessage } from "../domain/errors";
import {
  parentResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import { type ResourceRef } from "../domain/workbench";
import {
  type EditorContent,
  type EditorContentBase,
} from "../features/files/editorContent";
import {
  getFileEditorController,
  type ActiveEditorState,
  type EditorStateBridge,
} from "../features/files/fileEditorController";
import type { SaveAllReport } from "./editorConflictSlice";
import { enqueueEditorOperation } from "./editorOperations";
import {
  issueRequestIntent,
  matchesActiveTabIntent,
  matchesProjectIntent,
  matchesRequestIntent,
} from "./projectIntent";
import { type WorkbenchState } from "./store";

export type { ActiveEditorState } from "../features/files/fileEditorController";

export interface EditorSlice {
  activeEditor: ActiveEditorState | null;
  pendingCloseTabId: string | null;
  cancelCloseTab: () => void;
  saveAndCloseTab: () => Promise<void>;
  discardAndCloseTab: () => Promise<void>;
  loadEditor: (resource: ResourceRef) => Promise<void>;
  updateEditorContent: (
    content: EditorContent,
    base?: EditorContentBase | null,
  ) => void;
  flushActiveEditor: () => Promise<void>;
  saveActiveEditor: () => Promise<boolean>;
  /** With `reload`, a File that is gone keeps its edits (Reload). */
  discardActiveEditor: (reload?: boolean) => Promise<boolean>;
}

export const createEditorSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  EditorSlice
> = (set, get) => {
  const editorController = getFileEditorController();
  const editorState = storeEditorBridge(set, get);

  const closePending = async (discard: boolean) => {
    const tabId = get().pendingCloseTabId;
    const tab = get().tabs.find((candidate) => candidate.id === tabId);
    if (!tab?.resource) {
      set({ pendingCloseTabId: null });
      return;
    }
    await get().activateTab(tab.id);
    const completed = discard
      ? await get().discardActiveEditor()
      : await get().saveActiveEditor();
    if (completed) {
      set({ pendingCloseTabId: null });
      get().closeTab(tab.id);
    }
  };

  return {
    activeEditor: null,
    pendingCloseTabId: null,
    cancelCloseTab: () => set({ pendingCloseTabId: null }),
    saveAndCloseTab: () => closePending(false),
    discardAndCloseTab: () => closePending(true),

    loadEditor: async (resource) => {
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (
        !projectId ||
        !matchesActiveTabIntent(get(), generation, projectId, "file", resource)
      ) {
        return;
      }
      const active = get().activeEditor;
      if (
        sameResource(active?.resource, resource) &&
        (active?.phase === "loading" || active?.phase === "ready")
      ) {
        return;
      }
      const requestKey = `editor:${resourceKey(resource)}`;
      let requestGeneration = 0;
      set((state) => {
        const request = issueRequestIntent(
          state.requestGenerations,
          requestKey,
          state.requestIntentSequence,
        );
        requestGeneration = request.token;
        return {
          activeEditor: editorController.loading(resource),
          requestGenerations: request.intents,
          requestIntentSequence: request.sequence,
        };
      });
      try {
        await editorController.load(
          resource,
          editorState,
          (candidate) =>
            matchesActiveTabIntent(
              get(),
              generation,
              projectId,
              "file",
              candidate,
            ) && matchesRequestIntent(get(), requestKey, requestGeneration),
        );
      } catch (error) {
        if (
          !matchesActiveTabIntent(
            get(),
            generation,
            projectId,
            "file",
            resource,
          ) ||
          !matchesRequestIntent(get(), requestKey, requestGeneration)
        ) {
          return;
        }
        editorController.failLoad(resource, error, editorState);
      }
    },

    updateEditorContent: (content, base) => {
      const state = get();
      const editor = state.activeEditor;
      if (
        state.projectTransitioning ||
        !editor ||
        editor.phase !== "ready" ||
        editor.saving
      ) {
        return;
      }
      editorController.updateContent(editorState, content, base);
    },

    flushActiveEditor: () =>
      enqueueEditorOperation(() => editorController.flush(editorState)),

    saveActiveEditor: async () => {
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId || get().projectTransitioning) return false;
      try {
        const savedResource = await enqueueEditorOperation(async () => {
          if (
            get().projectTransitioning ||
            !matchesProjectIntent(get(), generation, projectId)
          ) {
            return null;
          }
          const result = await editorController.save(
            editorState,
            (resource) =>
              !get().projectTransitioning &&
              matchesProjectIntent(get(), generation, projectId) &&
              matchesActiveTabIntent(
                get(),
                generation,
                projectId,
                "file",
                resource,
              ),
          );
          if (!result) {
            return null;
          }
          return result.outcome === "conflict"
            ? null
            : result.document.resource;
        });
        if (!savedResource) {
          return false;
        }
        const parent = parentResource(savedResource);
        set((state) => {
          const directoryListings = { ...state.directoryListings };
          const diffs = { ...state.diffs };
          delete directoryListings[resourceKey(parent)];
          delete diffs[resourceKey(savedResource)];
          return {
            directoryListings,
            diffs,
            saveAllReport: reportWithout(state.saveAllReport, savedResource),
          };
        });
        await Promise.all([
          get().refreshRepository(),
          get().loadDirectory(parent),
        ]);
        return true;
      } catch (error) {
        if (!matchesProjectIntent(get(), generation, projectId)) return false;
        const { actionFailed } =
          await import("../features/files/editorNoticeState");
        set({ error: actionFailed(editorState, error) });
        return false;
      }
    },

    discardActiveEditor: async (reload) => {
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId || get().projectTransitioning) return false;
      try {
        return await enqueueEditorOperation(async () => {
          if (
            get().projectTransitioning ||
            !matchesProjectIntent(get(), generation, projectId)
          ) {
            return false;
          }
          const document = await editorController.discard(
            editorState,
            (resource) =>
              !get().projectTransitioning &&
              matchesProjectIntent(get(), generation, projectId) &&
              matchesActiveTabIntent(
                get(),
                generation,
                projectId,
                "file",
                resource,
              ),
            reload,
          );
          return document !== null;
        });
      } catch (error) {
        if (matchesProjectIntent(get(), generation, projectId)) {
          editorController.saveError(editorState, error);
          set({ error: errorMessage(error) });
        }
        return false;
      }
    },
  };
};

/** The report without `resource`, which has since been saved. */
function reportWithout(
  report: SaveAllReport | null,
  resource: ResourceRef,
): SaveAllReport | null {
  if (!report) return null;
  const failures = report.failures.filter(
    (failure) => !sameResource(failure.resource, resource),
  );
  if (failures.length === report.failures.length) return report;
  return failures.length ? { ...report, failures } : null;
}

/** The active editor as the store holds it, with its tabs kept in step. */
export function storeEditorBridge(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
): EditorStateBridge {
  return {
    getActiveEditor: () => get().activeEditor,
    setActiveEditor: (editor, update) => {
      set((state) => ({
        activeEditor: editor,
        tabs: tabsWithEditorUpdate(state.tabs, editor.resource, update),
      }));
    },
  };
}

/**
 * Applies a dirty or status change to the editor's tab. The tabs array keeps
 * its identity when nothing changes, so a keystroke that leaves the tab dirty
 * does not re-render every tabs subscriber (EP-01).
 */
export function tabsWithEditorUpdate(
  tabs: WorkbenchState["tabs"],
  resource: ResourceRef,
  update?: { dirty?: boolean; tabStatus?: "failed" | null },
): WorkbenchState["tabs"] {
  if (!update) return tabs;
  // Every tab on the resource, since a File and its Diff can both be open.
  const next = tabs.map((tab) => {
    if (!sameResource(tab.resource, resource)) return tab;
    const dirty = update.dirty ?? tab.dirty;
    const status =
      update.tabStatus === undefined
        ? tab.status
        : (update.tabStatus ?? undefined);
    return dirty === tab.dirty && status === tab.status
      ? tab
      : { ...tab, dirty, status };
  });
  return next.some((tab, index) => tab !== tabs[index]) ? next : tabs;
}
