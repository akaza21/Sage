import { sameResource } from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import type { ActiveEditorState } from "../features/files/fileEditorController";

export const ACP_READ_TEXT_FILE_FLUSH_EVENT = "acp-read-text-file-flush-v1";

type Owner = {
  projectId: string;
  projectGeneration: string;
  chatId: string;
  sessionId: string;
  runtimeEpoch: string;
};
export type AcpReadTextFileFlushEvent = {
  owner: Owner;
  resource: ResourceRef;
  expectedEditorRevision: number | null;
  token: string;
};
export type AcpReadTextFileFlushState =
  | {
      kind: "classify";
      state: "notOpen" | "openClean" | "dirty" | "persisted";
      editorRevision: number | null;
    }
  | {
      kind: "diskAllowed";
      state: "notOpen" | "openClean";
      editorRevision: number | null;
    }
  | { kind: "persisted"; editorRevision: number; durableRevision: number };
export type AcpReadTextFileFlushAck = {
  owner: Owner;
  resource: ResourceRef;
  expectedEditorRevision: number | null;
  token: string;
  state: AcpReadTextFileFlushState;
};
export type AcpReadTextNextAction =
  | {
      kind: "diskAllowed";
      state: "notOpen" | "openClean";
      editorRevision: number | null;
    }
  | {
      kind: "flushRequired";
      editorRevision: number;
      previousDurableRevision: number | null;
    }
  | { kind: "persisted"; editorRevision: number }
  | { kind: "complete" };

export type ReadTextFileEditorBridge = {
  projectId: string | undefined;
  projectGeneration: number;
  transitioning: boolean;
  activeEditor: ActiveEditorState | null;
  flushActiveEditor(): Promise<void>;
  currentEditor(): ActiveEditorState | null;
};
function sameOwner(
  event: AcpReadTextFileFlushEvent,
  bridge: ReadTextFileEditorBridge,
) {
  return (
    bridge.projectId === event.owner.projectId &&
    String(bridge.projectGeneration) === event.owner.projectGeneration &&
    !bridge.transitioning
  );
}
function classifyCurrent(
  event: AcpReadTextFileFlushEvent,
  editor: ActiveEditorState | null,
): Extract<AcpReadTextFileFlushState, { kind: "classify" }> | null {
  if (!editor || !sameResource(editor.resource, event.resource)) {
    return event.expectedEditorRevision === null
      ? { kind: "classify", state: "notOpen", editorRevision: null }
      : {
          kind: "classify",
          state: "persisted",
          editorRevision: event.expectedEditorRevision,
        };
  }
  if (editor.phase !== "ready") return null;
  return editor.document.dirty
    ? { kind: "classify", state: "dirty", editorRevision: editor.revision }
    : { kind: "classify", state: "openClean", editorRevision: editor.revision };
}
function sameClassification(
  left: Extract<AcpReadTextFileFlushState, { kind: "classify" }>,
  right: Extract<AcpReadTextFileFlushState, { kind: "classify" }> | null,
) {
  return (
    right?.state === left.state && right.editorRevision === left.editorRevision
  );
}
function envelope(
  event: AcpReadTextFileFlushEvent,
  state: AcpReadTextFileFlushState,
): AcpReadTextFileFlushAck {
  return {
    owner: event.owner,
    resource: event.resource,
    expectedEditorRevision: event.expectedEditorRevision,
    token: event.token,
    state,
  };
}

/** Classify/reserve is identity-only. The terminal ack is accepted only after
 * native returns the matching typed no-content action for this token. */
export async function subscribeAcpReadTextFileFlush(
  bridge: () => ReadTextFileEditorBridge,
  acknowledge: (ack: AcpReadTextFileFlushAck) => Promise<AcpReadTextNextAction>,
): Promise<() => void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
    return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<AcpReadTextFileFlushEvent>(
    ACP_READ_TEXT_FILE_FLUSH_EVENT,
    (event) => {
      void acknowledgeReadTextFileFlush(
        event.payload,
        bridge(),
        acknowledge,
      ).catch(() => undefined);
    },
  );
}

export async function acknowledgeReadTextFileFlush(
  event: AcpReadTextFileFlushEvent,
  bridge: ReadTextFileEditorBridge,
  acknowledge: (ack: AcpReadTextFileFlushAck) => Promise<AcpReadTextNextAction>,
): Promise<void> {
  if (!sameOwner(event, bridge)) return;
  const classify = classifyCurrent(event, bridge.activeEditor);
  if (!classify) return;
  const action = await acknowledge(envelope(event, classify));
  if (!sameOwner(event, bridge)) return;
  if (action.kind === "diskAllowed") {
    const current = classifyCurrent(event, bridge.currentEditor());
    if (
      !current ||
      !sameClassification(classify, current) ||
      action.state !== current.state ||
      action.editorRevision !== current.editorRevision
    )
      return;
    await acknowledge(
      envelope(event, {
        kind: "diskAllowed",
        state: action.state,
        editorRevision: action.editorRevision,
      }),
    );
    return;
  }
  if (action.kind === "persisted") {
    const current = classifyCurrent(event, bridge.currentEditor());
    if (
      !current ||
      !sameClassification(classify, current) ||
      current.state !== "persisted" ||
      action.editorRevision !== event.expectedEditorRevision
    )
      return;
    await acknowledge(
      envelope(event, {
        kind: "persisted",
        editorRevision: action.editorRevision,
        durableRevision: action.editorRevision,
      }),
    );
    return;
  }
  if (
    action.kind !== "flushRequired" ||
    classify.state !== "dirty" ||
    action.editorRevision !== classify.editorRevision
  )
    return;
  await bridge.flushActiveEditor();
  const current = bridge.currentEditor();
  if (
    !sameOwner(event, bridge) ||
    !current ||
    current.phase !== "ready" ||
    !sameResource(current.resource, event.resource) ||
    current.revision !== action.editorRevision ||
    current.persistedRevision !== action.editorRevision
  )
    return;
  await acknowledge(
    envelope(event, {
      kind: "persisted",
      editorRevision: action.editorRevision,
      durableRevision: action.editorRevision,
    }),
  );
}
