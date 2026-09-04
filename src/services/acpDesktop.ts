import type {
  AcpChatPage,
  AcpCommandFailure,
  AgentEditDisposition,
  AcpMutationResult,
  AcpProjectionChangedEvent,
  ActivateAcpSessionRequest,
  CancelAcpTurnRequest,
  CloseAcpSessionRequest,
  CodexDiscoveryResponse,
  CodexAuthResponse,
  CodexAuthScope,
  GetAcpChatPageRequest,
  GetCodexDiscoveryRequest,
  ResolvePermissionDecisionRequest,
  ResolvePermissionDecisionResponse,
  SendAcpTurnRequest,
  StartAcpSessionRequest,
} from "../domain/acp";
import {
  isOneOf,
  isRecord,
  safeAcpMutationResponse,
  safeCodexDiscovery,
  safeCounter,
  safeIdentifier,
} from "./acpDesktopResponses";

export const ACP_PROJECTION_CHANGED_EVENT = "acp-projection-changed-v1";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function safeMessage(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > 8192) {
    return null;
  }
  return new TextEncoder().encode(value).byteLength <= 8192 ? value : null;
}

function safePermissionDecision(
  value: unknown,
  request: ResolvePermissionDecisionRequest,
): ResolvePermissionDecisionResponse | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const chatId = safeIdentifier(value.chatId);
  const sessionId = safeIdentifier(value.sessionId);
  const turnId = safeIdentifier(value.turnId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  const requestId = safeIdentifier(value.requestId);
  const expectedState = {
    ask: "asked",
    allowOnce: "allowPending",
    deny: "declinePending",
  }[request.decision];
  if (
    !projectId ||
    !chatId ||
    !sessionId ||
    !turnId ||
    !runtimeEpoch ||
    !requestId ||
    projectId !== request.projectId ||
    chatId !== request.chatId ||
    sessionId !== request.sessionId ||
    turnId !== request.turnId ||
    runtimeEpoch !== request.runtimeEpoch ||
    requestId !== request.requestId ||
    value.decision !== request.decision ||
    value.state !== expectedState ||
    !isOneOf(value.decision, ["ask", "allowOnce", "deny"] as const) ||
    !isOneOf(value.state, [
      "received",
      "asked",
      "allowPending",
      "allowCommitted",
      "declinePending",
      "declineCommitted",
      "responseQueued",
      "resolvedAllowed",
      "resolvedDeclined",
    ] as const)
  ) {
    return null;
  }
  return {
    projectId,
    chatId,
    sessionId,
    turnId,
    runtimeEpoch,
    requestId,
    decision: value.decision,
    state: value.state,
  } as ResolvePermissionDecisionResponse;
}

function safeCodexAuth(value: unknown): CodexAuthResponse | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const projectGeneration = safeCounter(value.projectGeneration, true);
  const bindingId = safeIdentifier(value.bindingId);
  const revision = safeCounter(value.revision, false);
  if (
    !projectId ||
    !projectGeneration ||
    !bindingId ||
    !revision ||
    !isOneOf(value.phase, [
      "signedOut",
      "waiting",
      "recovery",
      "signedIn",
    ] as const)
  ) {
    return null;
  }
  return {
    projectId,
    projectGeneration,
    bindingId,
    phase: value.phase,
    revision,
  } as CodexAuthResponse;
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

async function invokeAcpMutation<Request extends object>(
  command: string,
  request: Request,
): Promise<AcpMutationResult> {
  const response = await invokeAcp<unknown, Request>(command, request);
  const safe = safeAcpMutationResponse(response);
  if (!safe) {
    throw new AcpDesktopError({
      kind: "internal",
      message: "ACP mutation returned an invalid bounded projection.",
    });
  }
  return safe;
}

export const acpDesktop = {
  async getCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("get_codex_auth", request);
  },

  async startCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("start_codex_auth", { ...request, consent: true });
  },

  async cancelCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("cancel_codex_auth", request);
  },

  async logoutCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("logout_codex_auth", request);
  },

  async discoverCodex(
    request: GetCodexDiscoveryRequest,
  ): Promise<CodexDiscoveryResponse> {
    const response = await invokeAcp<unknown, GetCodexDiscoveryRequest>(
      "get_codex_discovery",
      request,
    );
    const safe = safeCodexDiscovery(response);
    if (!safe) {
      throw new AcpDesktopError({
        kind: "internal",
        message: "Codex discovery returned an invalid bounded response.",
      });
    }
    return safe;
  },

  getChatPage(request: GetAcpChatPageRequest): Promise<AcpChatPage> {
    return invokeAcp("get_acp_chat_page", request);
  },

  startSession(request: StartAcpSessionRequest): Promise<AcpMutationResult> {
    return invokeAcpMutation("start_acp_session", request);
  },

  activateSession(
    request: ActivateAcpSessionRequest,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("activate_acp_session", request);
  },

  sendTurn(request: SendAcpTurnRequest): Promise<AcpMutationResult> {
    return invokeAcpMutation("send_acp_turn", request);
  },

  cancelTurn(request: CancelAcpTurnRequest): Promise<AcpMutationResult> {
    return invokeAcpMutation("cancel_acp_turn", request);
  },

  closeSession(request: CloseAcpSessionRequest): Promise<AcpMutationResult> {
    return invokeAcpMutation("close_acp_session", request);
  },

  async resolvePermissionDecision(
    request: ResolvePermissionDecisionRequest,
  ): Promise<ResolvePermissionDecisionResponse> {
    const response = await invokeAcp<unknown, ResolvePermissionDecisionRequest>(
      "resolve_permission_decision",
      request,
    );
    const safe = safePermissionDecision(response, request);
    if (!safe) {
      throw new AcpDesktopError({
        kind: "internal",
        message: "Permission decision returned an invalid bounded response.",
      });
    }
    return safe;
  },

  async recordAgentEditDisposition(
    request: AgentEditDisposition,
  ): Promise<AcpMutationResult> {
    const response = await invokeAcp<unknown, AgentEditDisposition>(
      "record_agent_edit_disposition",
      request,
    );
    const safe = safeAcpMutationResponse(response);
    if (!safe) {
      throw new AcpDesktopError({
        kind: "internal",
        message: "Agent edit disposition returned an invalid projection.",
      });
    }
    return safe;
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

async function invokeCodexAuth(
  command: string,
  request: CodexAuthScope | (CodexAuthScope & { consent: true }),
): Promise<CodexAuthResponse> {
  const response = await invokeAcp<unknown, typeof request>(command, request);
  const safe = safeCodexAuth(response);
  if (!safe) {
    throw new AcpDesktopError({
      kind: "internal",
      message: "Codex authentication returned an invalid bounded response.",
    });
  }
  return safe;
}
