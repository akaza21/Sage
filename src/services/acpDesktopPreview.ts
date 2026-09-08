import type {
  PreviewProjectionChangedEvent,
  PreviewTargetRef,
} from "../domain/previewProjection";
import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
} from "./acpDesktopResponses";

const EVENT = "acp-projection-changed-v1";

function epoch(value: unknown): number | null {
  return typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= Number.MAX_SAFE_INTEGER
    ? value
    : null;
}
function origin(value: unknown): string | undefined {
  if (typeof value !== "string" || utf8ByteLength(value) > 2048)
    return undefined;
  try {
    const url = new URL(value);
    return url.username === "" &&
      url.password === "" &&
      (url.protocol === "http:" || url.protocol === "https:") &&
      (url.hostname === "127.0.0.1" || url.hostname === "[::1]") &&
      url.port !== ""
      ? value
      : undefined;
  } catch {
    return undefined;
  }
}
function target(value: unknown): PreviewTargetRef | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId),
    projectGeneration = safeCounter(value.projectGeneration, true),
    expectedTransitionEpoch = epoch(value.expectedTransitionEpoch),
    chatId = safeIdentifier(value.chatId),
    sessionId = safeIdentifier(value.sessionId),
    runtimeEpoch = safeCounter(value.runtimeEpoch, false),
    rootId = safeIdentifier(value.rootId);
  return projectId &&
    projectGeneration &&
    expectedTransitionEpoch !== null &&
    projectGeneration === String(expectedTransitionEpoch) &&
    chatId &&
    sessionId &&
    runtimeEpoch &&
    rootId &&
    (value.scriptName === "dev" || value.scriptName === "start")
    ? ({
        projectId,
        projectGeneration,
        expectedTransitionEpoch,
        chatId,
        sessionId,
        runtimeEpoch,
        rootId,
        scriptName: value.scriptName,
      } as PreviewTargetRef)
    : null;
}
function valid(phase: unknown, failure: unknown): boolean {
  return (
    (phase === "idle" && (failure === null || failure === "scriptMissing")) ||
    (["discovering", "stopped"].includes(phase as string) &&
      failure === null) ||
    ([
      "starting",
      "waitingForOrigin",
      "healthChecking",
      "active",
      "stopping",
    ].includes(phase as string) &&
      (failure === null || failure === "conflict")) ||
    (phase === "healthFailed" &&
      [
        "scriptMissing",
        "originNotLoopback",
        "startupTimeout",
        "healthTimeout",
        "internal",
      ].includes(failure as string)) ||
    (phase === "exited" && failure === "exitedEarly")
  );
}
export function safePreviewProjectionChanged(
  value: unknown,
): PreviewProjectionChangedEvent | null {
  if (!isRecord(value) || value.kind !== "preview" || !isRecord(value.preview))
    return null;
  const p = value.preview,
    projectId = safeIdentifier(p.projectId),
    projectGeneration = safeCounter(p.projectGeneration, true),
    transitionEpoch = epoch(p.transitionEpoch),
    chatId = safeIdentifier(p.chatId),
    sessionId = safeIdentifier(p.sessionId),
    runtimeEpoch = safeCounter(p.runtimeEpoch, false),
    previewRevision = safeCounter(p.previewRevision, false),
    t = p.target === null ? null : target(p.target),
    o = p.origin === null ? null : origin(p.origin),
    failure = p.failure === null ? null : p.failure;
  if (
    !projectId ||
    !projectGeneration ||
    transitionEpoch === null ||
    projectGeneration !== String(transitionEpoch) ||
    !chatId ||
    !sessionId ||
    !runtimeEpoch ||
    !previewRevision ||
    (p.target !== null && !t) ||
    o === undefined ||
    !isOneOf(p.phase, [
      "idle",
      "discovering",
      "starting",
      "waitingForOrigin",
      "healthChecking",
      "active",
      "healthFailed",
      "exited",
      "stopping",
      "stopped",
    ] as const) ||
    (failure !== null &&
      !isOneOf(failure, [
        "scriptMissing",
        "originNotLoopback",
        "startupTimeout",
        "healthTimeout",
        "exitedEarly",
        "conflict",
        "internal",
      ] as const)) ||
    !valid(p.phase, failure) ||
    (t &&
      (t.projectId !== projectId ||
        t.projectGeneration !== projectGeneration ||
        t.expectedTransitionEpoch !== transitionEpoch ||
        t.chatId !== chatId ||
        t.sessionId !== sessionId ||
        t.runtimeEpoch !== runtimeEpoch))
  )
    return null;
  return {
    kind: "preview",
    preview: {
      projectId,
      projectGeneration,
      transitionEpoch,
      chatId,
      sessionId,
      runtimeEpoch,
      previewRevision,
      phase: p.phase,
      failure,
      target: t,
      origin: o,
    },
  } as PreviewProjectionChangedEvent;
}
export async function subscribePreviewProjectionChanges(
  onChange: (event: PreviewProjectionChangedEvent) => void,
): Promise<() => void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
    return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>(EVENT, (event) => {
    const parsed = safePreviewProjectionChanged(event.payload);
    if (parsed) onChange(parsed);
  });
}
import { utf8ByteLength } from "../domain/text";
