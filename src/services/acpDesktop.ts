import type {
  AcpChatPage,
  AcpCommandFailure,
  AcpMutationResult,
  AcpProjectionChangedEvent,
  AcpSessionMutationIntent,
  ActivateAcpSessionRequest,
  AgentEditDisposition,
  CancelAcpTurnRequest,
  CloseAcpSessionRequest,
  CodexAuthResponse,
  CodexAuthScope,
  CodexDiscoveryResponse,
  EndAcpSessionRequest,
  GetAcpChatPageRequest,
  GetCodexDiscoveryRequest,
  ResolvePermissionDecisionRequest,
  SendAcpTurnRequest,
  StartAcpSessionRequest,
  SteerAcpTurnRequest,
} from "../domain/acp";
import { decodeAcpGoal, type AcpGoalState } from "../domain/acpGoal";
import type {
  AgentDiffSnapshot,
  GetAgentDiffRequest,
} from "../domain/agentDiff";
import type { PreviewProjectionChangedEvent } from "../domain/previewProjection";
import type { ResourceRef } from "../domain/workbench";
import { isTauriRuntime } from "../ports/desktopRuntime";
import { invokeNative } from "../ports/nativeInvoke";
import { resolvePermissionDecision } from "./acpDesktopPermission";
import { subscribePreviewProjectionChanges } from "./acpDesktopPreview";
import { getChatStatus, safeProjectionChanged } from "./acpDesktopProjection";
import {
  isOneOf,
  isRecord,
  safeAcpMutationResponse,
  safeCodexDiscovery,
  safeCounter,
  safeIdentifier,
  safeMessage,
} from "./acpDesktopResponses";

export const ACP_PROJECTION_CHANGED_EVENT = "acp-projection-changed-v1";

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
  decodeGoal?: (value: unknown) => AcpGoalState | null,
): Promise<AcpMutationResult> {
  const response = await invokeAcp<unknown, Request>(command, request);
  const safe = safeAcpMutationResponse(response, decodeGoal);
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
  getChatStatus,
  ...sessionConfigDesktop(invokeAcpMutation, isTauriRuntime),
  pauseExistingGoal(
    request: AcpSessionMutationIntent,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("pause_existing_goal", request, decodeAcpGoal);
  },
  clearExistingGoal(
    request: AcpSessionMutationIntent,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("clear_existing_goal", request, decodeAcpGoal);
  },
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

  async startCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("start_codex_auth", { ...request, consent: true });
  },

  async cancelCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("cancel_codex_auth", request);
  },

  async logoutCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("logout_codex_auth", request);
  },

  async startClaudeAuthLogin(): Promise<void> {
    await invokeNative("start_claude_auth_login");
  },

  /** a terminal turn's workspace (Git) changes; decoded by caller. */
  getTurnWorkspaceChanges(request: {
    projectId: string;
    projectGeneration: string;
    chatId: string;
    turnId: string;
  }): Promise<unknown> {
    return invokeNative("get_acp_turn_workspace_changes", { request });
  },

  async getClaudeAuthStatus(): Promise<{ loggedIn: boolean }> {
    const value: unknown = await invokeNative("get_claude_auth_status");
    if (!isRecord(value) || typeof value.loggedIn !== "boolean") {
      throw new Error("Claude Code sign-in status was invalid.");
    }
    return { loggedIn: value.loggedIn };
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
