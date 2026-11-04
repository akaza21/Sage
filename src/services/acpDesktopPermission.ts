import type {
  ResolvePermissionDecisionRequest,
  ResolvePermissionDecisionResponse,
} from "../domain/acp";
import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
} from "./acpDesktopGuards";

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
  const browser = value.source === "browser";
  const expectedState = browser
    ? {
        ask: null,
        allowOnce: "resolvedAllowed",
        allowAlways: null,
        allowForOriginSession: "resolvedAllowed",
        deny: "resolvedDeclined",
      }[request.decision]
    : {
        ask: "asked",
        allowOnce: "allowPending",
        allowAlways: "allowPending",
        allowForOriginSession: null,
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
    (value.source !== undefined && value.source !== "browser") ||
    !expectedState ||
    value.state !== expectedState ||
    !isOneOf(value.decision, [
      "ask",
      "allowOnce",
      "allowAlways",
      "allowForOriginSession",
      "deny",
    ] as const) ||
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
      "cancelled",
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
    ...(browser ? { source: "browser" as const } : {}),
  } as ResolvePermissionDecisionResponse;
}

export async function resolvePermissionDecision(
  request: ResolvePermissionDecisionRequest,
  invoke: (
    command: string,
    request: ResolvePermissionDecisionRequest,
  ) => Promise<unknown>,
  invalidResponse: () => Error,
): Promise<ResolvePermissionDecisionResponse> {
  const response = await invoke("resolve_permission_decision", request);
  const safe = safePermissionDecision(response, request);
  if (!safe) throw invalidResponse();
  return safe;
}
