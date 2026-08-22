import { useEffect } from "react";

import { sameResource } from "../../domain/resourceIdentity";
import type {
  BufferSnapshotAck,
  BufferSnapshotRequest,
  EditorDocument,
  EditorSaveResult,
  ResourceRef,
} from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import type { EditorPort } from "../../ports/workbenchPorts";

export const EDITOR_SNAPSHOT_DELAY_MS = 750;

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
  load(resource: ResourceRef): Promise<EditorDocument>;
  snapshot(request: BufferSnapshotRequest): Promise<BufferSnapshotAck>;
  scheduleSnapshot(flush: () => Promise<unknown>): () => void;
  save(
    resource: ResourceRef,
    flush?: () => Promise<unknown>,
    canSave?: () => boolean,
  ): Promise<EditorSaveResult | null>;
  discard(resource: ResourceRef): Promise<EditorDocument>;
}

export function bindFileEditorController(
  editor: EditorPort = getWorkbenchPorts().editor,
): FileEditorController {
  let snapshotTimer: number | undefined;

  const cancelScheduledSnapshot = () => {
    if (snapshotTimer !== undefined) {
      window.clearTimeout(snapshotTimer);
      snapshotTimer = undefined;
    }
  };

  return {
    load: (resource) => editor.openEditorDocument(resource),
    snapshot: (request) => editor.snapshotEditorBuffer(request),
    scheduleSnapshot: (flush) => {
      cancelScheduledSnapshot();
      snapshotTimer = window.setTimeout(() => {
        snapshotTimer = undefined;
        void flush().catch(() => undefined);
      }, EDITOR_SNAPSHOT_DELAY_MS);
      return cancelScheduledSnapshot;
    },
    save: async (resource, flush, canSave) => {
      cancelScheduledSnapshot();
      await flush?.();
      if (canSave && !canSave()) {
        return null;
      }
      return editor.saveEditorDocument(resource);
    },
    discard: async (resource) => {
      cancelScheduledSnapshot();
      return editor.discardEditorBuffer(resource);
    },
  };
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
