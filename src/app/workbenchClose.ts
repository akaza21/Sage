// Kept apart from the workspace watcher so the startup bundle does not carry
// it.

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
): Promise<void> {
  if (!isTauriRuntime()) return Promise.resolve();
  closeTraceQueue = closeTraceQueue
    .then(() => invokeNative<void>("trace_close_stage", { stage, status }))
    .catch(() => undefined);
  return closeTraceQueue;
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
 * Through this hook the settings store and its catalogue stay out of the
 * startup bundle; until it binds nothing can be pending.
 */
interface EditorSettingsClose {
  hold(): () => void;
  flush(): Promise<void>;
}

let editorSettingsClose: EditorSettingsClose | null = null;

export function bindEditorSettingsClose(close: EditorSettingsClose) {
  editorSettingsClose = close;
}

let workspaceNotifyClose: (() => Promise<void>) | null = null;

export function bindWorkspaceNotifyClose(unbind: () => Promise<void>) {
  workspaceNotifyClose = unbind;
}

export async function closeLiveWorkbench(
  get: () => WorkbenchState,
): Promise<void> {
  const releaseSettings = editorSettingsClose?.hold() ?? (() => undefined);
  const expectedTransitionEpoch = get().nativeTransitionEpoch;
  try {
    await flushLiveWorkbench(get);
    await runCloseStage(
      "workspaceUnwatch",
      () => workspaceNotifyClose?.() ?? Promise.resolve(),
    );
    await runCloseStage("persistAndDestroy", () =>
      getWorkbenchPorts().project.persistProjectLayoutAndClose(
        currentProjectLayout(get()),
        expectedTransitionEpoch,
      ),
    );
  } finally {
    releaseSettings();
  }
}

/** Persist disposable workbench state without unwatching or destroying its host. */
export async function flushLiveWorkbench(get: () => WorkbenchState) {
  const releaseSettings = editorSettingsClose?.hold() ?? (() => undefined);
  try {
    await runCloseStage("editorFlush", () => get().flushActiveEditor());
    await runCloseStage(
      "appearanceFlush",
      () => editorSettingsClose?.flush() ?? Promise.resolve(),
    );
    const state = get();
    if (state.projectTransitioning) {
      throw new Error("Project layout mutation is already in progress");
    }
    const layout = currentProjectLayout(state);
    if (layout) {
      await getWorkbenchPorts().project.persistProjectLayout(
        layout,
        state.nativeTransitionEpoch,
      );
    }
    return layout;
  } finally {
    releaseSettings();
  }
}
