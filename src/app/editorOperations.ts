const MAX_QUEUED_EDITOR_OPERATIONS = 8;

let editorOperationPipeline: Promise<void> = Promise.resolve();

let queuedEditorOperations = 0;

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
