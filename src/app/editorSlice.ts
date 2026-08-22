import type { StateCreator } from "zustand";

import {
  parentResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import type {
  BufferSnapshotRequest,
  EditorDocument,
  ResourceRef,
} from "../domain/workbench";
import { bindFileEditorController } from "../features/files/fileEditorController";
import {
  editorErrorMessage as errorMessage,
  editorReady,
  enqueueEditorOperation,
} from "./editorOperations";
import {
  matchesActiveTabIntent,
  matchesProjectIntent,
  matchesRequestIntent,
  issueRequestIntent,
} from "./projectIntent";
import type { WorkbenchState } from "./store";

export type ActiveEditorState =
  | { phase: "loading"; resource: ResourceRef }
  | { phase: "error"; resource: ResourceRef; message: string }
  | {
      phase: "ready";
      resource: ResourceRef;
      document: EditorDocument;
      content: string;
      revision: number;
      persistedRevision: number;
      persisting: boolean;
      saving: boolean;
      message: string | null;
    };

export interface EditorSlice {
  activeEditor: ActiveEditorState | null;
  pendingCloseTabId: string | null;
  cancelCloseTab: () => void;
  saveAndCloseTab: () => Promise<void>;
  discardAndCloseTab: () => Promise<void>;
  loadEditor: (resource: ResourceRef) => Promise<void>;
  updateEditorContent: (content: string) => void;
  flushActiveEditor: () => Promise<void>;
  saveActiveEditor: () => Promise<boolean>;
  discardActiveEditor: () => Promise<boolean>;
}

const MAX_SNAPSHOTS_PER_FLUSH = 8;

export const createEditorSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  EditorSlice
> = (set, get) => {
  const editorController = bindFileEditorController();

  const persistLatestEditor = async (): Promise<void> => {
    for (let attempt = 0; attempt < MAX_SNAPSHOTS_PER_FLUSH; attempt += 1) {
      const editor = get().activeEditor;
      if (
        !editor ||
        editor.phase !== "ready" ||
        editor.revision <= editor.persistedRevision
      ) {
        return;
      }
      const request: BufferSnapshotRequest = {
        resource: editor.resource,
        base: editor.document.base,
        lineEnding: editor.document.lineEnding,
        content: editor.content,
        revision: editor.revision,
      };
      set({ activeEditor: { ...editor, persisting: true, message: null } });
      try {
        const ack = await editorController.snapshot(request);
        set((state) => {
          const current = state.activeEditor;
          if (
            !current ||
            current.phase !== "ready" ||
            !sameResource(current.resource, request.resource)
          ) {
            return {};
          }
          const dirty = current.revision > ack.revision || ack.dirty;
          return {
            activeEditor: {
              ...current,
              persistedRevision: Math.max(
                current.persistedRevision,
                ack.revision,
              ),
              persisting: false,
              document: { ...current.document, dirty },
            },
            tabs: state.tabs.map((tab) =>
              sameResource(tab.resource, request.resource)
                ? { ...tab, dirty }
                : tab,
            ),
          };
        });
      } catch (error) {
        const message = errorMessage(error);
        set((state) => {
          const current = state.activeEditor;
          if (
            !current ||
            current.phase !== "ready" ||
            !sameResource(current.resource, request.resource)
          ) {
            return {};
          }
          return {
            activeEditor: {
              ...current,
              persisting: false,
              message:
                current.revision === request.revision
                  ? message
                  : current.message,
            },
          };
        });
        throw error;
      }
    }
    throw new Error(
      `editor snapshot flush reached its ${MAX_SNAPSHOTS_PER_FLUSH}-revision limit`,
    );
  };

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
          activeEditor: { phase: "loading", resource },
          requestGenerations: request.intents,
          requestIntentSequence: request.sequence,
        };
      });
      try {
        const document = await editorController.open(resource);
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
        const activeTab = get().tabs.find(
          (tab) => tab.id === get().activeTabId,
        )!;
        set((state) => ({
          activeEditor: editorReady(document),
          tabs: state.tabs.map((tab) =>
            tab.id === activeTab.id
              ? {
                  ...tab,
                  dirty: document.dirty,
                  status: document.conflict ? "failed" : tab.status,
                }
              : tab,
          ),
        }));
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
        set({
          activeEditor: {
            phase: "error",
            resource,
            message: errorMessage(error),
          },
        });
      }
    },

    updateEditorContent: (content) => {
      set((state) => {
        const editor = state.activeEditor;
        if (
          state.projectTransitioning ||
          !editor ||
          editor.phase !== "ready" ||
          editor.saving
        ) {
          return {};
        }
        return {
          activeEditor: {
            ...editor,
            content,
            revision: editor.revision + 1,
            message: null,
          },
          tabs: state.tabs.map((tab) =>
            tab.id === state.activeTabId ? { ...tab, dirty: true } : tab,
          ),
        };
      });
    },

    flushActiveEditor: () => enqueueEditorOperation(persistLatestEditor),

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
          const editor = get().activeEditor;
          if (!editor || editor.phase !== "ready") {
            return null;
          }
          set({ activeEditor: { ...editor, saving: true, message: null } });
          await persistLatestEditor();
          if (
            get().projectTransitioning ||
            !matchesProjectIntent(get(), generation, projectId)
          ) {
            return null;
          }
          const result = await editorController.save(editor.resource);
          if (
            !matchesActiveTabIntent(
              get(),
              generation,
              projectId,
              "file",
              editor.resource,
            )
          ) {
            return null;
          }
          set((state) => ({
            activeEditor: editorReady(result.document),
            tabs: state.tabs.map((tab) =>
              sameResource(tab.resource, editor.resource)
                ? {
                    ...tab,
                    dirty: result.document.dirty,
                    status: result.document.conflict ? "failed" : undefined,
                  }
                : tab,
            ),
          }));
          return result.outcome === "conflict" ? null : editor.resource;
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
          return { directoryListings, diffs };
        });
        await Promise.all([
          get().refreshRepository(),
          get().loadDirectory(parent),
        ]);
        return true;
      } catch (error) {
        if (!matchesProjectIntent(get(), generation, projectId)) return false;
        const message = errorMessage(error);
        set((state) => ({
          error: message,
          activeEditor:
            state.activeEditor?.phase === "ready"
              ? { ...state.activeEditor, saving: false, message }
              : state.activeEditor,
        }));
        return false;
      }
    },

    discardActiveEditor: async () => {
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
          const editor = get().activeEditor;
          if (!editor || editor.phase !== "ready") {
            return false;
          }
          const document = await editorController.discard(editor.resource);
          if (
            !matchesActiveTabIntent(
              get(),
              generation,
              projectId,
              "file",
              editor.resource,
            )
          ) {
            return false;
          }
          set((state) => ({
            activeEditor: editorReady(document),
            tabs: state.tabs.map((tab) =>
              sameResource(tab.resource, editor.resource)
                ? { ...tab, dirty: false, status: undefined }
                : tab,
            ),
          }));
          return true;
        });
      } catch (error) {
        if (matchesProjectIntent(get(), generation, projectId)) {
          set({ error: errorMessage(error) });
        }
        return false;
      }
    },
  };
};
