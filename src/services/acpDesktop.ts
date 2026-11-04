import type {
  AcpChatPage,
  AcpMutationResult,
  AcpProjectionChangedEvent,
  AcpSessionMutationIntent,
  ActivateAcpSessionRequest,
  AgentEditDisposition,
  CancelAcpTurnRequest,
  CloseAcpSessionRequest,
  EndAcpSessionRequest,
  GetAcpChatPageRequest,
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
import { isRecord, safeIdentifier } from "./acpDesktopGuards";
import { safeAcpMutationResponse } from "./acpDesktopResponses";

import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { sessionConfigDesktop } from "./acpSessionConfigDesktop";
export { AcpDesktopError, safeFailure } from "./acpDesktopCommand";

export const ACP_PROJECTION_CHANGED_EVENT = "acp-projection-changed-v1";

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

  async startClaudeAuthLogin(): Promise<void> {
    await invokeNative("start_claude_auth_login");
  },

  async getClaudeAuthStatus(): Promise<{ loggedIn: boolean }> {
    const value: unknown = await invokeNative("get_claude_auth_status");
    if (!isRecord(value) || typeof value.loggedIn !== "boolean") {
      throw new Error("Claude Code sign-in status was invalid.");
    }
    return { loggedIn: value.loggedIn };
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

/** Native Git ledger request, independent of the full Chat adapter object. */
export function getTurnWorkspaceChanges(request: {
  projectId: string;
  projectGeneration: string;
  chatId: string;
  turnId: string;
}): Promise<unknown> {
  return invokeNative("get_acp_turn_workspace_changes", { request });
}
