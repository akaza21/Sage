export type PermissionDecision =
  "ask" | "allowOnce" | "allowAlways" | "allowForOriginSession" | "deny";

export type PermissionState =
  | "received"
  | "asked"
  | "allowPending"
  | "allowCommitted"
  | "declinePending"
  | "declineCommitted"
  | "responseQueued"
  | "resolvedAllowed"
  | "resolvedDeclined"
  | "cancelled";

export interface ResolvePermissionDecisionRequest {
  projectId: string;
  chatId: string;
  sessionId: string;
  turnId: string;
  runtimeEpoch: string;
  requestId: string;
  decision: PermissionDecision;
}

export interface ResolvePermissionDecisionResponse {
  source?: "browser";
  projectId: string;
  chatId: string;
  sessionId: string;
  turnId: string;
  runtimeEpoch: string;
  requestId: string;
  decision: PermissionDecision;
  state: PermissionState;
}
