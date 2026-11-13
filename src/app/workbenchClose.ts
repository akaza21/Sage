// The Workbench close sequence and its trace. Kept apart from the workspace
// watcher so the startup bundle that traces close stages does not carry the
// watcher; the close path loads it when it unwatches.

import { errorMessage } from "../domain/errors";
import { invokeNative, isTauriRuntime } from "../ports/nativeInvoke";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";

type CloseStage =
  | "closeRequested"
  | "editorFlush"
  | "appearanceFlush"
  | "workspaceUnwatch"
  | "persistAndDestroy";
type CloseStageStatus = "start" | "end" | "error";
let closeTraceQueue = Promise.resolve();

export function traceCloseStage(
  stage: CloseStage,
  status: CloseStageStatus,
): void {
  if (!isTauriRuntime()) return;
  closeTraceQueue = closeTraceQueue
    .then(() => invokeNative<void>("trace_close_stage", { stage, status }))
    .catch(() => undefined);
}

async function runCloseStage(
  stage: CloseStage,
  work: () => Promise<void>,
): Promise<void> {
  traceCloseStage(stage, "start");
  try {
    await work();
    traceCloseStage(stage, "end");
  } catch (error) {
    traceCloseStage(stage, "error");
    const detail = errorMessage(error);
    throw new Error(`Close ${stage} failed: ${detail}`, { cause: error });
  }
}

/**
 * The close path holds and flushes Editor settings through this hook, so the
 * settings store and its catalogue load with the Workbench instead of in the
 * startup bundle. The store binds itself when its module loads; until then
 * nothing can be pending, so holding and flushing do nothing.
 */
interface EditorSettingsClose {
  hold(): () => void;
  flush(): Promise<void>;
}

let editorSettingsClose: EditorSettingsClose | null = null;

export function bindEditorSettingsClose(close: EditorSettingsClose) {
  editorSettingsClose = close;
}

export async function closeLiveWorkbench(
  get: () => WorkbenchState,
): Promise<void> {
  const releaseSettings = editorSettingsClose?.hold() ?? (() => undefined);
  try {
    await runCloseStage("editorFlush", () => get().flushActiveEditor());
    await runCloseStage(
      "appearanceFlush",
      () => editorSettingsClose?.flush() ?? Promise.resolve(),
    );
    await runCloseStage("workspaceUnwatch", async () => {
      const { unbindWorkspaceNotify } =
        await import("./workspaceNotifyLifecycle");
      await unbindWorkspaceNotify();
    });
    await runCloseStage("persistAndDestroy", () =>
      getWorkbenchPorts().project.persistProjectLayoutAndClose(
        currentProjectLayout(get()),
        get().nativeTransitionEpoch,
      ),
    );
  } finally {
    releaseSettings();
  }
}
