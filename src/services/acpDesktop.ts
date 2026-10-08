import type {
  AcpChatPage,
  AcpCommandFailure,
  AgentEditDisposition,
  AcpMutationResult,
  AcpProjectionChangedEvent,
  ActivateAcpSessionRequest,
  CancelAcpTurnRequest,
  SteerAcpTurnRequest,
  CloseAcpSessionRequest,
  EndAcpSessionRequest,
  CodexDiscoveryResponse,
  CodexAuthResponse,
  CodexAuthScope,
  GetAcpChatPageRequest,
  GetCodexDiscoveryRequest,
  ResolvePermissionDecisionRequest,
  SendAcpTurnRequest,
  StartAcpSessionRequest,
} from "../domain/acp";
import type { PreviewProjectionChangedEvent } from "../domain/previewProjection";
import type {
  AgentDiffSnapshot,
  GetAgentDiffRequest,
} from "../domain/agentDiff";
import { invokeNative } from "../ports/nativeInvoke";
import type { ResourceRef } from "../domain/workbench";
import type { AcpGoalState } from "../domain/acpGoal";
import { subscribePreviewProjectionChanges } from "./acpDesktopPreview";
import { safeProjectionChanged } from "./acpDesktopProjection";
import { acknowledgeReadTextFileFlush } from "./acpDesktopReadText";
import { resolvePermissionDecision } from "./acpDesktopPermission";
import {
  isOneOf,
  isRecord,
  safeCodexDiscovery,
  safeCounter,
  safeIdentifier,
  safeMessage,
  safeAcpMutationResponse,
} from "./acpDesktopResponses";

export const ACP_PROJECTION_CHANGED_EVENT = "acp-projection-changed-v1";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
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

export function safeFailure(value: unknown): AcpCommandFailure {
  if (
    isRecord(value) &&
    isOneOf(value.promptDispatch, ["notStarted", "unknown"] as const)
  ) {
    const { promptDispatch, ...detail } = value;
    const failure = safeFailure(
      isRecord(value.failure) ? value.failure : detail,
    );
    return { ...failure, promptDispatch };
  }
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
        "imageInputUnsupported",
      ])
    ) {
      return { kind, reason: value.reason, message };
    }
    if (
      kind === "conflict" &&
      message &&
      isOneOf(value.resource, [
        "projectGeneration",
        "configRevision",
        "projectionRevision",
        "session",
        "runtimeEpoch",
        "turnState",
        "turnRevision",
        "clientIntent",
        "steerOutcomeUnknown",
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
    if (kind === "signInRequired" && message) {
      return { kind, message };
    }
    if (
      (kind === "invalidRequest" ||
        kind === "rejected" ||
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
    return await invokeNative<T>(command, { request });
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

import { sessionConfigDesktop } from "./acpSessionConfigDesktop";
export const acpDesktop = {
  ...sessionConfigDesktop(invokeAcpMutation, isTauriRuntime),
  async resolveChatFileReference(
    projectId: string,
    destination: string,
  ): Promise<{ resource: ResourceRef; lineNumber: number | null } | null> {
    const value = await invokeAcp<
      unknown,
      { projectId: string; destination: string }
    >("resolve_chat_file_reference", { projectId, destination });
    if (value === null) return null;
    if (!isRecord(value) || !isRecord(value.resource)) return null;
    const rootId = safeIdentifier(value.resource.rootId);
    const relativePath = value.resource.relativePath;
    const lineNumber = value.lineNumber;
    if (
      !rootId ||
      typeof relativePath !== "string" ||
      relativePath.length === 0 ||
      relativePath.length > 16_384 ||
      !(
        lineNumber === null ||
        (Number.isSafeInteger(lineNumber) &&
          typeof lineNumber === "number" &&
          lineNumber > 0)
      )
    )
      return null;
    return { resource: { rootId, relativePath }, lineNumber };
  },
  async getFileSnapshot(
    request: GetAgentDiffRequest,
  ): Promise<AgentDiffSnapshot> {
    const { fetchAgentDiffSnapshot } = await import("./acpFileSnapshot");
    return fetchAgentDiffSnapshot(
      request,
      (bounded) => invokeAcp("get_acp_file_snapshot", bounded),
      (kind, message) => new AcpDesktopError({ kind, message }),
    );
  },

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

  async getChatPage(
    request: GetAcpChatPageRequest,
    decodeGoal?: (value: unknown) => AcpGoalState | null,
  ): Promise<AcpChatPage> {
    const page = await invokeAcp("get_acp_chat_page", request);
    const safe = safeAcpMutationResponse({ projection: page }, decodeGoal);
    if (!safe)
      throw new AcpDesktopError({
        kind: "internal",
        message: "ACP page returned an invalid bounded projection.",
      });
    return safe.projection;
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

  steerTurn(request: SteerAcpTurnRequest): Promise<AcpMutationResult> {
    return invokeAcpMutation("steer_acp_turn", request);
  },

  closeSession(request: CloseAcpSessionRequest): Promise<AcpMutationResult> {
    return invokeAcpMutation("close_acp_session", request);
  },

  endSession(request: EndAcpSessionRequest): Promise<AcpMutationResult> {
    return invokeAcpMutation("end_acp_session", request);
  },

  resolvePermissionDecision: (request: ResolvePermissionDecisionRequest) =>
    resolvePermissionDecision(
      request,
      invokeAcp,
      () =>
        new AcpDesktopError({
          kind: "internal",
          message: "Permission decision returned an invalid bounded response.",
        }),
    ),

  async recordAgentEditDisposition(
    request: AgentEditDisposition,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("record_agent_edit_disposition", request);
  },

  acknowledgeReadTextFileFlush,

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

  async subscribePreviewProjectionChanges(
    onChange: (event: PreviewProjectionChangedEvent) => void,
  ): Promise<() => void> {
    return subscribePreviewProjectionChanges(onChange);
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
