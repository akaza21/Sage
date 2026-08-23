export type PermissionDecision = "ask" | "allowOnce" | "deny";

export type PermissionState =
  | "received"
  | "asked"
  | "allowPending"
  | "allowCommitted"
  | "declinePending"
  | "declineCommitted"
  | "responseQueued"
  | "resolvedAllowed"
  | "resolvedDeclined";

export interface ResolvePermissionDecisionRequest {
  projectId: string;
  chatId: string;
  sessionId: string;
  requestId: string;
  decision: PermissionDecision;
}

export interface ResolvePermissionDecisionResponse {
  projectId: string;
  chatId: string;
  sessionId: string;
  requestId: string;
  decision: PermissionDecision;
  state: PermissionState;
}
