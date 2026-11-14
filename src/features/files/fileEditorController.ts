import { useEffect } from "react";

import { errorMessage } from "../../domain/errors";
import { sameResource } from "../../domain/resourceIdentity";
import type {
  BufferSnapshotAck,
  BufferSnapshotRequest,
  EditorDocument,
  EditorSaveResult,
  ResourceRef,
} from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import type { EditorContent, EditorContentBase } from "./editorContent";

export const EDITOR_SNAPSHOT_DELAY_MS = 750;

const MAX_SNAPSHOTS_PER_FLUSH = 8;

export type ActiveEditorState =
  | { phase: "loading"; resource: ResourceRef }
  | {
      phase: "error";
      resource: ResourceRef;
      message: string;
      kind?: "mixedLineEndings";
    }
  | {
      phase: "ready";
      resource: ResourceRef;
      document: EditorDocument;
      content: EditorContent;
      revision: number;
      persistedRevision: number;
      persisting: boolean;
      saving: boolean;
      message: string | null;
      /** Persistence failure retained through edits until save, reload, or Keep mine. */
      unstored?: string;
      unwatched?: string;
    };

export type ReadyEditorState = Extract<ActiveEditorState, { phase: "ready" }>;

export interface EditorStateBridge {
  getActiveEditor(): ActiveEditorState | null;
  setActiveEditor(
    editor: ActiveEditorState,
    update?: { dirty?: boolean; tabStatus?: "failed" | null },
  ): void;
}

interface EditorLifecycleState {
  phase: "loading" | "error" | "ready";
  resource: ResourceRef;
  revision?: number;
  persistedRevision?: number;
}

interface FileEditorLifecycleOptions {
  resource: ResourceRef | undefined;
  editor: EditorLifecycleState | null;
  loadEditor(resource: ResourceRef): Promise<void>;
  flushActiveEditor(): Promise<void>;
}

export interface FileEditorController {
  loading(resource: ResourceRef): ActiveEditorState;
  failLoad(
    resource: ResourceRef,
    error: unknown,
    bridge: EditorStateBridge,
  ): void;
  load(
    resource: ResourceRef,
    bridge?: EditorStateBridge,
    canCommit?: (resource: ResourceRef) => boolean,
  ): Promise<EditorDocument>;
  ready(document: EditorDocument): ActiveEditorState;
  updateContent(
    bridge: EditorStateBridge,
    content: EditorContent,
    base?: EditorContentBase | null,
  ): void;
  snapshot(request: BufferSnapshotRequest): Promise<BufferSnapshotAck>;
  flush(bridge: EditorStateBridge): Promise<void>;
  scheduleSnapshot(flush: () => Promise<unknown>): () => void;
  save(
    bridge: EditorStateBridge,
    canCommit: (resource: ResourceRef) => boolean,
  ): Promise<EditorSaveResult | null>;
  /** Null if nothing was loaded. With `reload`, a File that is gone keeps its
   * edits, and the document is returned for the caller to show rather than shown. */
  discard(
    bridge: EditorStateBridge,
    canCommit: (resource: ResourceRef) => boolean,
    reload?: boolean,
  ): Promise<EditorDocument | null>;
  saveError(bridge: EditorStateBridge, error: unknown): void;
}

function ready(document: EditorDocument): ReadyEditorState {
  return {
    phase: "ready",
    resource: document.resource,
    document,
    content: document.buffer.content,
    revision: document.buffer.revision,
    persistedRevision: document.buffer.revision,
    persisting: false,
    saving: false,
    message: null,
  };
}

function currentReady(bridge: EditorStateBridge): ReadyEditorState | null {
  const editor = bridge.getActiveEditor();
  return editor?.phase === "ready" ? editor : null;
}

function createFileEditorController(editor: EditorPort): FileEditorController {
  let snapshotTimer: number | undefined;

  const cancelScheduledSnapshot = () => {
    if (snapshotTimer !== undefined) {
      window.clearTimeout(snapshotTimer);
      snapshotTimer = undefined;
    }
  };

  const persistLatestEditor = async (bridge: EditorStateBridge) => {
    for (let attempt = 0; attempt < MAX_SNAPSHOTS_PER_FLUSH; attempt += 1) {
      const active = currentReady(bridge);
      if (!active || active.revision <= active.persistedRevision) {
        return;
      }
      const request: BufferSnapshotRequest = {
        resource: active.resource,
        base: active.document.base,
        lineEnding: active.document.lineEnding,
        content: String(active.content),
        revision: active.revision,
      };
      bridge.setActiveEditor({ ...active, persisting: true, message: null });
      try {
        const ack = await editor.snapshotEditorBuffer(request);
        const current = currentReady(bridge);
        if (!current || !sameResource(current.resource, request.resource)) {
          return;
        }
        const dirty = current.revision > ack.revision || ack.dirty;
        bridge.setActiveEditor(
          {
            ...current,
            persistedRevision: Math.max(
              current.persistedRevision,
              ack.revision,
            ),
            persisting: false,
          },
          { dirty },
        );
      } catch (error) {
        const current = currentReady(bridge);
        if (!current || !sameResource(current.resource, request.resource)) {
          return;
        }
        bridge.setActiveEditor({
          ...current,
          persisting: false,
          message:
            current.revision === request.revision
              ? errorMessage(error)
              : current.message,
        });
        throw error;
      }
    }
    throw new Error(
      `editor snapshot flush reached its ${MAX_SNAPSHOTS_PER_FLUSH}-revision limit`,
    );
  };

  return {
    loading: (resource) => ({ phase: "loading", resource }),
    failLoad: (resource, error, bridge) => {
      bridge.setActiveEditor({
        phase: "error",
        resource,
        message: errorMessage(error),
        kind:
          typeof error === "object" &&
          error !== null &&
          "kind" in error &&
          error.kind === "mixedLineEndings"
            ? "mixedLineEndings"
            : undefined,
      });
    },
    load: async (resource, bridge, canCommit = () => true) => {
      const document = await editor.openEditorDocument(resource);
      if (bridge && canCommit(resource)) {
        bridge.setActiveEditor(ready(document), {
          dirty: document.dirty,
          tabStatus: document.conflict ? "failed" : undefined,
        });
      }
      return document;
    },
    ready,
    updateContent: (bridge, content, base) => {
      const active = currentReady(bridge);
      if (!active) {
        return;
      }
      const bufferContent = active.document.buffer.content;
      // Comparing text is the fallback; the length check spares the string for
      // most edits.
      const unchanged =
        base?.content === bufferContent
          ? base.unchanged
          : content.length === bufferContent.length &&
            String(content) === bufferContent;
      const dirty = unchanged ? active.document.dirty : true;
      bridge.setActiveEditor(
        {
          ...active,
          content,
          revision: active.revision + 1,
          message: null,
        },
        { dirty },
      );
    },
    snapshot: (request) => editor.snapshotEditorBuffer(request),
    flush: persistLatestEditor,
    scheduleSnapshot: (flush) => {
      cancelScheduledSnapshot();
      snapshotTimer = window.setTimeout(() => {
        snapshotTimer = undefined;
        void flush().catch(() => undefined);
      }, EDITOR_SNAPSHOT_DELAY_MS);
      return cancelScheduledSnapshot;
    },
    save: async (bridge, canCommit) => {
      cancelScheduledSnapshot();
      const active = currentReady(bridge);
      if (!active || !canCommit(active.resource)) {
        return null;
      }
      bridge.setActiveEditor({ ...active, saving: true, message: null });
      await persistLatestEditor(bridge);
      const current = currentReady(bridge);
      if (!current || !canCommit(current.resource)) {
        return null;
      }
      const result = await editor.saveEditorDocument(current.resource);
      if (!canCommit(current.resource)) {
        return null;
      }
      bridge.setActiveEditor(ready(result.document), {
        dirty: result.document.dirty,
        tabStatus: result.document.conflict ? "failed" : null,
      });
      return result;
    },
    discard: async (bridge, canCommit, reload) => {
      cancelScheduledSnapshot();
      const active = currentReady(bridge);
      if (!active || !canCommit(active.resource)) {
        return null;
      }
      const document = await editor.discardEditorBuffer(
        active.resource,
        reload,
      );
      if (!canCommit(active.resource)) {
        return null;
      }
      if (!reload) {
        bridge.setActiveEditor(ready(document), {
          dirty: document.dirty,
          tabStatus: null,
        });
      }
      return document;
    },
    saveError: (bridge, error) => {
      const active = currentReady(bridge);
      if (active) {
        bridge.setActiveEditor({
          ...active,
          saving: false,
          message: errorMessage(error),
        });
      }
    },
  };
}

let sharedFileEditorController: FileEditorController | undefined;

export function bindFileEditorController(
  editor?: EditorPort,
): FileEditorController {
  if (editor) {
    sharedFileEditorController = createFileEditorController(editor);
  }
  return getFileEditorController();
}

export function getFileEditorController(): FileEditorController {
  sharedFileEditorController ??= createFileEditorController(
    getWorkbenchPorts().editor,
  );
  return sharedFileEditorController;
}

export function useFileEditorLifecycle(
  controller: FileEditorController,
  {
    resource,
    editor,
    loadEditor,
    flushActiveEditor,
  }: FileEditorLifecycleOptions,
) {
  const editorPhase = editor?.phase;
  const editorResource = editor?.resource;
  const revision = editor?.revision ?? 0;
  const persistedRevision = editor?.persistedRevision ?? 0;

  useEffect(() => {
    if (resource) {
      void loadEditor(resource);
    }
  }, [loadEditor, resource]);

  useEffect(() => {
    if (
      !resource ||
      editorPhase !== "ready" ||
      !sameResource(editorResource, resource) ||
      revision <= persistedRevision
    ) {
      return;
    }
    return controller.scheduleSnapshot(flushActiveEditor);
  }, [
    controller,
    editorPhase,
    editorResource,
    flushActiveEditor,
    persistedRevision,
    resource,
    revision,
  ]);
}
