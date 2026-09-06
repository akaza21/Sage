import type {
  PreviewProjectionOwner,
  PreviewTargetRef,
} from "../../domain/previewProjection";

export type { PreviewTargetRef } from "../../domain/previewProjection";

export type PreviewDiscoveryRequest = Omit<
  PreviewProjectionOwner,
  "transitionEpoch"
> & { expectedTransitionEpoch: number };

export type PreviewDiscoveryResponse = {
  projectId: string;
  transitionEpoch: number;
  targets: PreviewTargetRef[];
  truncated: boolean;
};

export type PreviewHealthResponse = {
  ref: PreviewTargetRef;
  origin: string | null;
  healthy: boolean;
};

export type PreviewStartResponse = { ref: PreviewTargetRef; origin: string };
export type PreviewStopResponse = { ref: PreviewTargetRef };

type PreviewFailure = {
  kind: "invalidRequest" | "conflict" | "unavailable" | "internal";
  message: string;
  resource?: string;
};

const MAX_COUNTER = "9223372036854775807";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function safeIdentifier(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 512 &&
    !value.includes("\0") &&
    new TextEncoder().encode(value).byteLength <= 512
  );
}

function safeCounter(value: unknown, allowZero: boolean): value is string {
  return (
    typeof value === "string" &&
    /^(0|[1-9][0-9]{0,18})$/.test(value) &&
    !(value.length === 19 && value > MAX_COUNTER) &&
    (allowZero || value !== "0")
  );
}

function isLoopbackOrigin(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    new TextEncoder().encode(value).byteLength > 2048
  ) {
    return false;
  }
  try {
    const url = new URL(value);
    return (
      (url.protocol === "http:" || url.protocol === "https:") &&
      url.username === "" &&
      url.password === "" &&
      (url.hostname === "127.0.0.1" || url.hostname === "[::1]") &&
      url.port !== ""
    );
  } catch {
    return false;
  }
}

function assertTarget(value: unknown): asserts value is PreviewTargetRef {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "projectId",
      "projectGeneration",
      "expectedTransitionEpoch",
      "chatId",
      "sessionId",
      "runtimeEpoch",
      "rootId",
      "scriptName",
    ]) ||
    !safeIdentifier(value.projectId) ||
    !safeCounter(value.projectGeneration, true) ||
    !safeEpoch(value.expectedTransitionEpoch) ||
    value.projectGeneration !== String(value.expectedTransitionEpoch) ||
    !safeIdentifier(value.chatId) ||
    !safeIdentifier(value.sessionId) ||
    !safeCounter(value.runtimeEpoch, false) ||
    !safeIdentifier(value.rootId) ||
    (value.scriptName !== "dev" && value.scriptName !== "start")
  ) {
    throw new PreviewDesktopError(invalid("Preview target is invalid."));
  }
}

function hasExactKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return (
    actual.length === keys.length && actual.every((key) => keys.includes(key))
  );
}

function sameTarget(left: PreviewTargetRef, right: PreviewTargetRef): boolean {
  return (
    left.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.expectedTransitionEpoch === right.expectedTransitionEpoch &&
    left.chatId === right.chatId &&
    left.sessionId === right.sessionId &&
    left.runtimeEpoch === right.runtimeEpoch &&
    left.rootId === right.rootId &&
    left.scriptName === right.scriptName
  );
}

function assertRequest(
  value: unknown,
): asserts value is PreviewDiscoveryRequest {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "projectId",
      "projectGeneration",
      "expectedTransitionEpoch",
      "chatId",
      "sessionId",
      "runtimeEpoch",
    ]) ||
    !safeIdentifier(value.projectId) ||
    !safeCounter(value.projectGeneration, true) ||
    !safeEpoch(value.expectedTransitionEpoch) ||
    Number(value.projectGeneration) !== value.expectedTransitionEpoch ||
    !safeIdentifier(value.chatId) ||
    !safeIdentifier(value.sessionId) ||
    !safeCounter(value.runtimeEpoch, false)
  ) {
    throw new PreviewDesktopError(
      invalid("Preview request identity is invalid."),
    );
  }
}

function invalid(message: string): PreviewFailure {
  return { kind: "invalidRequest", message };
}

function safeFailure(value: unknown): PreviewFailure {
  if (
    isRecord(value) &&
    typeof value.message === "string" &&
    value.message.length > 0 &&
    new TextEncoder().encode(value.message).byteLength <= 8192 &&
    (value.kind === "invalidRequest" ||
      value.kind === "conflict" ||
      value.kind === "unavailable" ||
      value.kind === "internal")
  ) {
    return {
      kind: value.kind,
      message: value.message,
      ...(safeIdentifier(value.resource) ? { resource: value.resource } : {}),
    };
  }
  return { kind: "internal", message: "The Preview request failed safely." };
}

export class PreviewDesktopError extends Error {
  constructor(readonly failure: PreviewFailure) {
    super(failure.message);
    this.name = "PreviewDesktopError";
  }
}

async function invokePreview<T>(command: string, request: object): Promise<T> {
  if (!isTauriRuntime()) {
    throw new PreviewDesktopError({
      kind: "unavailable",
      message: "Preview is available only in the Sage desktop runtime.",
    });
  }
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(command, { request });
  } catch (error) {
    throw new PreviewDesktopError(safeFailure(error));
  }
}

export async function discoverPreviewTargets(
  request: PreviewDiscoveryRequest,
): Promise<PreviewDiscoveryResponse> {
  assertRequest(request);
  const exactRequest: PreviewDiscoveryRequest = {
    projectId: request.projectId,
    projectGeneration: request.projectGeneration,
    expectedTransitionEpoch: request.expectedTransitionEpoch,
    chatId: request.chatId,
    sessionId: request.sessionId,
    runtimeEpoch: request.runtimeEpoch,
  };
  const response = await invokePreview<unknown>(
    "discover_preview_targets",
    exactRequest,
  );
  if (!isRecord(response) || !Array.isArray(response.targets)) {
    throw new PreviewDesktopError(
      invalid("Preview discovery returned an invalid response."),
    );
  }
  response.targets.forEach(assertTarget);
  if (
    response.projectId !== exactRequest.projectId ||
    response.transitionEpoch !== exactRequest.expectedTransitionEpoch ||
    response.targets.length > 16 ||
    typeof response.truncated !== "boolean" ||
    !response.targets.every(
      (target) =>
        target.projectId === exactRequest.projectId &&
        target.projectGeneration === exactRequest.projectGeneration &&
        target.expectedTransitionEpoch ===
          exactRequest.expectedTransitionEpoch &&
        target.chatId === exactRequest.chatId &&
        target.sessionId === exactRequest.sessionId &&
        target.runtimeEpoch === exactRequest.runtimeEpoch,
    )
  ) {
    throw new PreviewDesktopError(
      invalid("Preview discovery returned an invalid response."),
    );
  }
  return response as PreviewDiscoveryResponse;
}

function safeEpoch(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= Number.MAX_SAFE_INTEGER
  );
}

export async function getPreviewHealth(
  ref: PreviewTargetRef,
): Promise<PreviewHealthResponse> {
  assertTarget(ref);
  const response = await invokePreview<unknown>("get_preview_health", ref);
  if (
    !isRecord(response) ||
    response.ref === undefined ||
    typeof response.healthy !== "boolean" ||
    (response.origin !== null && !isLoopbackOrigin(response.origin))
  ) {
    throw new PreviewDesktopError(
      invalid("Preview health returned an invalid response."),
    );
  }
  assertTarget(response.ref);
  if (!sameTarget(response.ref, ref)) {
    throw new PreviewDesktopError(
      invalid("Preview health returned an invalid response."),
    );
  }
  return response as PreviewHealthResponse;
}

export async function startPreviewTarget(
  ref: PreviewTargetRef,
): Promise<PreviewStartResponse> {
  assertTarget(ref);
  const response = await invokePreview<unknown>("start_preview_target", ref);
  if (
    !isRecord(response) ||
    response.ref === undefined ||
    !isLoopbackOrigin(response.origin)
  ) {
    throw new PreviewDesktopError(
      invalid("Preview start returned an invalid origin."),
    );
  }
  assertTarget(response.ref);
  if (!sameTarget(response.ref, ref)) {
    throw new PreviewDesktopError(
      invalid("Preview start returned an invalid response."),
    );
  }
  return response as PreviewStartResponse;
}

export async function stopPreviewTarget(
  ref: PreviewTargetRef,
): Promise<PreviewStopResponse> {
  assertTarget(ref);
  const response = await invokePreview<unknown>("stop_preview_target", ref);
  if (!isRecord(response) || response.ref === undefined) {
    throw new PreviewDesktopError(
      invalid("Preview stop returned an invalid response."),
    );
  }
  assertTarget(response.ref);
  if (!sameTarget(response.ref, ref)) {
    throw new PreviewDesktopError(
      invalid("Preview stop returned an invalid response."),
    );
  }
  return response as PreviewStopResponse;
}
