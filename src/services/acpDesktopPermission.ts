import type {
  ResolvePermissionDecisionRequest,
  ResolvePermissionDecisionResponse,
} from "../domain/acp";
import { isRecord, safeCounter, safeIdentifier } from "./acpDesktopResponses";

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
  // One table per source: its keys are the decisions that source accepts,
  // so a decision outside it has no expected state and is refused.
  const states: Record<string, string> = browser
    ? {
        allowOnce: "resolvedAllowed",
        allowForOriginSession: "resolvedAllowed",
        allowDeveloperMode: "resolvedAllowed",
        deny: "resolvedDeclined",
      }
    : {
        ask: "asked",
        allowOnce: "allowPending",
        allowAlways: "allowPending",
        deny: "declinePending",
      };
  const expectedState = Object.hasOwn(states, request.decision)
    ? states[request.decision]
    : null;
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
    value.state !== expectedState
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
    // Equal to the checked response values; the table holds only known states.
    decision: request.decision,
    state: expectedState,
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
