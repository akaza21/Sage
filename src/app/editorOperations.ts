import type { EditorDocument } from "../domain/workbench";
import type { ActiveEditorState } from "./editorSlice";

const MAX_QUEUED_EDITOR_OPERATIONS = 8;
let editorOperationPipeline: Promise<void> = Promise.resolve();
let queuedEditorOperations = 0;

export function editorErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function editorReady(document: EditorDocument): ActiveEditorState {
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

export function enqueueEditorOperation<T>(
  operation: () => Promise<T>,
): Promise<T> {
  if (queuedEditorOperations >= MAX_QUEUED_EDITOR_OPERATIONS) {
    return Promise.reject(
      new Error(
        `editor operation queue reached its ${MAX_QUEUED_EDITOR_OPERATIONS}-operation limit`,
      ),
    );
  }
  queuedEditorOperations += 1;
  const result = editorOperationPipeline.then(operation, operation);
  editorOperationPipeline = result.then(
    () => undefined,
    () => undefined,
  );
  void result.then(
    () => {
      queuedEditorOperations -= 1;
    },
    () => {
      queuedEditorOperations -= 1;
    },
  );
  return result;
}
