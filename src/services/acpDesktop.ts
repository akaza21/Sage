import type {
  AcpChatPage,
  AcpCommandFailure,
  AcpMutationResult,
  AcpProjectionChangedEvent,
  ActivateAcpSessionRequest,
  CancelAcpTurnRequest,
  CloseAcpSessionRequest,
  GetAcpChatPageRequest,
  SendAcpTurnRequest,
  StartAcpSessionRequest,
} from "../domain/acp";

export const ACP_PROJECTION_CHANGED_EVENT = "acp-projection-changed-v1";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function safeMessage(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > 8192) {
    return null;
  }
  return new TextEncoder().encode(value).byteLength <= 8192 ? value : null;
}

function safeIdentifier(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > 512) {
    return null;
  }
  return new TextEncoder().encode(value).byteLength <= 512 ? value : null;
}

function safeProjectionChanged(
  value: unknown,
): AcpProjectionChangedEvent | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const chatId = safeIdentifier(value.chatId);
  const sessionId = safeIdentifier(value.sessionId);
  const projectionRevision =
    typeof value.projectionRevision === "string"
      ? value.projectionRevision
      : null;
  if (
    !projectId ||
    !chatId ||
    !sessionId ||
    !projectionRevision ||
    !/^[1-9][0-9]{0,19}$/.test(projectionRevision)
  ) {
    return null;
  }
  return {
    projectId,
    chatId,
    sessionId,
    projectionRevision,
  } as AcpProjectionChangedEvent;
}

function isOneOf<const Value extends string>(
  value: unknown,
  allowed: readonly Value[],
): value is Value {
  return typeof value === "string" && allowed.includes(value as Value);
}

function safeFailure(value: unknown): AcpCommandFailure {
  if (isRecord(value)) {
    const kind = value.kind;
    const message = safeMessage(value.message);
    if (
      kind === "unavailable" &&
      message &&
      isOneOf(value.reason, [
        "desktopRuntimeUnavailable",
        "bindingUnavailable",
        "capabilityUnavailable",
        "platformUnsupported",
        "sessionUnavailable",
      ])
    ) {
      return { kind, reason: value.reason, message };
    }
    if (
      kind === "conflict" &&
      message &&
      isOneOf(value.resource, [
        "projectGeneration",
        "projectionRevision",
        "session",
        "runtimeEpoch",
        "turnState",
        "turnRevision",
        "clientIntent",
      ])
    ) {
      return { kind, resource: value.resource, message };
    }
    if (
      kind === "capacity" &&
      message &&
      isOneOf(value.resource, [
        "activeTurn",
        "liveTree",
        "history",
        "pageRead",
        "storage",
        "queue",
      ]) &&
      Number.isSafeInteger(value.limit) &&
      (value.limit as number) >= 0
    ) {
      return {
        kind,
        resource: value.resource,
        limit: value.limit as number,
        message,
      };
    }
    if (
      (kind === "invalidRequest" ||
        kind === "recoveryRequired" ||
        kind === "internal") &&
      message
    ) {
      return { kind, message };
    }
  }
  return { kind: "internal", message: "The ACP request failed safely." };
}

export class AcpDesktopError extends Error {
  readonly failure: AcpCommandFailure;

  constructor(failure: AcpCommandFailure) {
    super(failure.message);
    this.name = "AcpDesktopError";
    this.failure = failure;
  }
}

async function invokeAcp<T, Request extends object>(
  command: string,
  request: Request,
): Promise<T> {
  if (!isTauriRuntime()) {
    throw new AcpDesktopError({
      kind: "unavailable",
      reason: "desktopRuntimeUnavailable",
      message: "ACP is available only in the Sage desktop runtime.",
    });
  }

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(command, { request });
  } catch (error: unknown) {
    if (error instanceof AcpDesktopError) {
      throw error;
    }
    throw new AcpDesktopError(safeFailure(error));
  }
}

export const acpDesktop = {
  getChatPage(request: GetAcpChatPageRequest): Promise<AcpChatPage> {
    return invokeAcp("get_acp_chat_page", request);
  },

  startSession(request: StartAcpSessionRequest): Promise<AcpMutationResult> {
    return invokeAcp("start_acp_session", request);
  },

  activateSession(
    request: ActivateAcpSessionRequest,
  ): Promise<AcpMutationResult> {
    return invokeAcp("activate_acp_session", request);
  },

  sendTurn(request: SendAcpTurnRequest): Promise<AcpMutationResult> {
    return invokeAcp("send_acp_turn", request);
  },

  cancelTurn(request: CancelAcpTurnRequest): Promise<AcpMutationResult> {
    return invokeAcp("cancel_acp_turn", request);
  },

  closeSession(request: CloseAcpSessionRequest): Promise<AcpMutationResult> {
    return invokeAcp("close_acp_session", request);
  },

  async subscribeProjectionChanges(
    onChange: (event: AcpProjectionChangedEvent) => void,
  ): Promise<() => void> {
    if (!isTauriRuntime()) return () => undefined;
    const { listen } = await import("@tauri-apps/api/event");
    return listen<unknown>(ACP_PROJECTION_CHANGED_EVENT, (event) => {
      const normalized = safeProjectionChanged(event.payload);
      if (normalized) onChange(normalized);
    });
  },
};
