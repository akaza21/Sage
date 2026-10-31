// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
// Upstream: apps/web/src/components/Sidebar.logic.ts.
import type { AgentSessionState, TurnState } from "./acp";
import type { SessionStatus } from "./workbench";

export interface ChatAttentionV1 {
  projectId: string;
  chatId: string;
  sessionId: string | null;
  sessionState: AgentSessionState | null;
  turnState: TurnState | null;
  running: boolean;
  pendingApprovals: number;
  pendingRequests: number;
  activityPosition: string;
  readPosition: string | null;
  terminalPosition: string | null;
  terminalState: TurnState | null;
  forcedUnreadPosition: string | null;
  revision: string;
  lineage: {
    parentProjectId: string;
    parentChatId: string;
    relation: "nativeFork" | "sideChat";
  } | null;
}

export type ChatAttentionEvent =
  | { projectId: string; chatId: string; revision: string }
  | { resyncRequired: true };
export interface AttentionMembership {
  projectId: string;
  chatIds: readonly string[];
}
export interface PresentedAttention {
  projectId: string;
  chatId: string;
  observedPosition: string;
  presented: boolean;
  foreground: boolean;
  agent: boolean;
  settings: boolean;
}

export function validPosition(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^(0|[1-9][0-9]{0,18})$/.test(value) &&
    BigInt(value) <= 9223372036854775807n
  );
}

export function attentionUnread(
  summary: ChatAttentionV1 | undefined,
  focused: boolean,
): boolean {
  if (!summary || focused) return false;
  const newer = (position: string | null) =>
    position !== null &&
    (summary.readPosition === null ||
      BigInt(position) > BigInt(summary.readPosition));
  if (newer(summary.forcedUnreadPosition)) return true;
  if (summary.terminalPosition === null) return false;
  if (summary.readPosition === null)
    return (
      summary.terminalState === "failed" ||
      summary.terminalState === "deliveryUnknown"
    );
  return newer(summary.terminalPosition);
}

export function attentionStatus(summary: ChatAttentionV1): {
  status: SessionStatus;
  approval: boolean;
} {
  if (summary.pendingApprovals > 0)
    return { status: "waiting", approval: true };
  if (summary.pendingRequests > 0)
    return { status: "waiting", approval: false };
  if (summary.running)
    return {
      status:
        summary.sessionState === "recovering"
          ? "recovering"
          : summary.turnState === "waitingUser" ||
              summary.turnState === "waitingApproval"
            ? "waiting"
            : "working",
      approval: false,
    };
  if (
    summary.terminalState === "failed" ||
    summary.terminalState === "deliveryUnknown" ||
    summary.sessionState === "failed" ||
    summary.sessionState === "incompatible"
  )
    return { status: "failed", approval: false };
  return {
    status: summary.terminalState === "completed" ? "complete" : "idle",
    approval: false,
  };
}
