export type PreviewScriptName = "dev" | "start";

export type PreviewTargetRef = {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  rootId: string;
  scriptName: PreviewScriptName;
};

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

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isSafeCounter(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= 9_007_199_254_740_991
  );
}

function isIdentifier(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 512 &&
    !value.includes("\0")
  );
}

function isLoopbackOrigin(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0 || value.length > 2048) {
    return false;
  }
  try {
    const url = new URL(value);
    return (
      (url.protocol === "http:" || url.protocol === "https:") &&
      url.username === "" &&
      url.password === "" &&
      ["127.0.0.1", "[::1]"].includes(url.hostname) &&
      url.port !== ""
    );
  } catch {
    return false;
  }
}

function assertTarget(value: unknown): asserts value is PreviewTargetRef {
  if (!isRecord(value))
    throw new PreviewDesktopError(invalid("Preview target is invalid."));
  if (
    !isIdentifier(value.projectId) ||
    !isSafeCounter(value.projectGeneration) ||
    !isSafeCounter(value.expectedTransitionEpoch) ||
    !isIdentifier(value.rootId) ||
    (value.scriptName !== "dev" && value.scriptName !== "start")
  ) {
    throw new PreviewDesktopError(invalid("Preview target is invalid."));
  }
  if ("origin" in value && !isLoopbackOrigin(value.origin)) {
    throw new PreviewDesktopError(
      invalid("Preview origins must be loopback HTTP(S) URLs."),
    );
  }
}

function invalid(message: string): PreviewFailure {
  return { kind: "invalidRequest", message };
}

function safeFailure(value: unknown): PreviewFailure {
  if (isRecord(value) && typeof value.message === "string") {
    if (
      value.kind === "invalidRequest" ||
      value.kind === "conflict" ||
      value.kind === "unavailable" ||
      value.kind === "internal"
    ) {
      return {
        kind: value.kind,
        message: value.message,
        ...(typeof value.resource === "string"
          ? { resource: value.resource }
          : {}),
      };
    }
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
  request: Pick<
    PreviewTargetRef,
    "projectId" | "projectGeneration" | "expectedTransitionEpoch"
  >,
): Promise<PreviewDiscoveryResponse> {
  assertTarget({ ...request, rootId: "discovery", scriptName: "dev" });
  const response = await invokePreview<unknown>(
    "discover_preview_targets",
    request,
  );
  if (!isRecord(response) || !Array.isArray(response.targets)) {
    throw new PreviewDesktopError(
      invalid("Preview discovery returned an invalid response."),
    );
  }
  for (const target of response.targets) assertTarget(target);
  if (
    response.projectId !== request.projectId ||
    response.transitionEpoch !== request.expectedTransitionEpoch ||
    response.targets.length > 16 ||
    typeof response.truncated !== "boolean"
  ) {
    throw new PreviewDesktopError(
      invalid("Preview discovery returned an invalid response."),
    );
  }
  return response as PreviewDiscoveryResponse;
}

export async function getPreviewHealth(
  ref: PreviewTargetRef,
): Promise<PreviewHealthResponse> {
  assertTarget(ref);
  const response = await invokePreview<unknown>("get_preview_health", ref);
  if (
    !isRecord(response) ||
    response.ref === undefined ||
    typeof response.healthy !== "boolean"
  ) {
    throw new PreviewDesktopError(
      invalid("Preview health returned an invalid response."),
    );
  }
  assertTarget(response.ref);
  if (response.origin !== null && !isLoopbackOrigin(response.origin)) {
    throw new PreviewDesktopError(
      invalid("Preview health returned a non-loopback origin."),
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
  return response as PreviewStopResponse;
}
