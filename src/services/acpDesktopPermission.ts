import type {
  ResolvePermissionDecisionRequest,
  ResolvePermissionDecisionResponse,
} from "../domain/acp";
import { isRecord, safeCounter, safeIdentifier } from "./acpDesktopGuards";

export function safePermissionIdentity(
  value: unknown,
  request: Omit<ResolvePermissionDecisionRequest, "decision">,
) {
  if (!isRecord(value)) return null;
  const identity = {
    projectId: safeIdentifier(value.projectId),
    chatId: safeIdentifier(value.chatId),
    sessionId: safeIdentifier(value.sessionId),
    turnId: safeIdentifier(value.turnId),
    runtimeEpoch: safeCounter(value.runtimeEpoch, false),
    requestId: safeIdentifier(value.requestId),
  };
  return Object.entries(identity).every(
    ([key, field]) => field && field === request[key as keyof typeof request],
  )
    ? (identity as typeof request)
    : null;
}

function safePermissionDecision(
  value: unknown,
  request: ResolvePermissionDecisionRequest,
): ResolvePermissionDecisionResponse | null {
  if (!isRecord(value)) return null;
  const identity = safePermissionIdentity(value, request);
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
    !identity ||
    value.decision !== request.decision ||
    (value.source !== undefined && value.source !== "browser") ||
    !expectedState ||
    value.state !== expectedState
  ) {
    return null;
  }
  return {
    ...identity,
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
