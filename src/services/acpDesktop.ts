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
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 512 ||
    value.includes("\0")
  ) {
    return null;
  }
  return new TextEncoder().encode(value).byteLength <= 512 ? value : null;
}

function safeCounter(value: unknown, allowZero: boolean): string | null {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,18})$/.test(value)) {
    return null;
  }
  if (value.length === 19 && value > "9223372036854775807") {
    return null;
  }
  return allowZero || value !== "0" ? value : null;
}

function safeCodexDiscovery(value: unknown): CodexDiscoveryResponse | null {
  if (!isRecord(value) || !Array.isArray(value.models)) return null;
  const projectId = safeIdentifier(value.projectId);
  const projectGeneration = safeCounter(value.projectGeneration, true);
  const rootId = safeIdentifier(value.rootId);
  const bindingId = safeIdentifier(value.bindingId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  if (
    !projectId ||
    !projectGeneration ||
    !rootId ||
    !bindingId ||
    !runtimeEpoch ||
    !isOneOf(value.account, ["managedChatgpt", "signedOut"] as const) ||
    value.models.length > 128
  ) {
    return null;
  }
  const ids = new Set<string>();
  let defaults = 0;
  const models = [];
  for (const candidate of value.models) {
    if (!isRecord(candidate)) return null;
    const id = safeIdentifier(candidate.id);
    const name = safeIdentifier(candidate.name);
    if (
      !id ||
      !name ||
      typeof candidate.isDefault !== "boolean" ||
      ids.has(id)
    ) {
      return null;
    }
    ids.add(id);
    defaults += Number(candidate.isDefault);
    models.push({ id, name, isDefault: candidate.isDefault });
  }
  const defaultModelId =
    value.defaultModelId === null ? null : safeIdentifier(value.defaultModelId);
  if (
    (value.account === "signedOut" &&
      (models.length !== 0 || defaultModelId !== null)) ||
    (value.account === "managedChatgpt" && models.length === 0) ||
    defaults > 1 ||
    (value.defaultModelId !== null && !defaultModelId) ||
    (defaultModelId !== null && !ids.has(defaultModelId)) ||
    (defaults === 0 && defaultModelId !== null) ||
    (defaults === 1 &&
      !models.some((model) => model.id === defaultModelId && model.isDefault))
  ) {
    return null;
  }
  return {
    projectId,
    projectGeneration,
    rootId,
    bindingId,
    runtimeEpoch,
    account: value.account,
    models,
    defaultModelId,
  } as unknown as CodexDiscoveryResponse;
}

function safePermissionDecision(
  value: unknown,
): ResolvePermissionDecisionResponse | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const chatId = safeIdentifier(value.chatId);
  const sessionId = safeIdentifier(value.sessionId);
  const requestId = safeIdentifier(value.requestId);
  if (
    !projectId ||
    !chatId ||
    !sessionId ||
    !requestId ||
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
    requestId,
    decision: value.decision,
    state: value.state,
  } as ResolvePermissionDecisionResponse;
}

function safeAgentEditDispositionResponse(value: unknown): { ok: true } | null {
  return isRecord(value) && value.ok === true ? { ok: true } : null;
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

  async resolvePermissionDecision(
    request: ResolvePermissionDecisionRequest,
  ): Promise<ResolvePermissionDecisionResponse> {
    const response = await invokeAcp<unknown, ResolvePermissionDecisionRequest>(
      "resolve_permission_decision",
      request,
    );
    const safe = safePermissionDecision(response);
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
  ): Promise<{ ok: true }> {
    const response = await invokeAcp<unknown, AgentEditDisposition>(
      "record_agent_edit_disposition",
      request,
    );
    const safe = safeAgentEditDispositionResponse(response);
    if (!safe) {
      throw new AcpDesktopError({
        kind: "internal",
        message: "Agent edit disposition returned an invalid response.",
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
