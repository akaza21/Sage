import type {
  BufferSnapshotAck,
  BufferSnapshotRequest,
  EditorDocument,
  EditorSaveResult,
  ResourceRef,
} from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import type { EditorPort } from "../../ports/workbenchPorts";

export interface FileEditorController {
  open(resource: ResourceRef): Promise<EditorDocument>;
  snapshot(request: BufferSnapshotRequest): Promise<BufferSnapshotAck>;
  save(resource: ResourceRef): Promise<EditorSaveResult>;
  discard(resource: ResourceRef): Promise<EditorDocument>;
}

export function bindFileEditorController(
  editor: EditorPort = getWorkbenchPorts().editor,
): FileEditorController {
  return {
    open: (resource) => editor.openEditorDocument(resource),
    snapshot: (request) => editor.snapshotEditorBuffer(request),
    save: (resource) => editor.saveEditorDocument(resource),
    discard: (resource) => editor.discardEditorBuffer(resource),
  };
}
