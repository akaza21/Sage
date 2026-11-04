import type { AgentRailChat, SessionStatus } from "../domain/workbench";
import { sessionRailStatusLabel } from "../features/chat/acp/sessionRail";

export const AGENT_RAIL_ORGANIZE_MODES = ["project", "list"] as const;
export type AgentRailOrganizeMode = (typeof AGENT_RAIL_ORGANIZE_MODES)[number];

/** "Last updated" or the person's own order ("Manual order"). */
export const AGENT_RAIL_ORDERINGS = ["recency", "manual"] as const;
export type AgentRailOrdering = (typeof AGENT_RAIL_ORDERINGS)[number];

export type AgentRailTrailingStateKind =
  "failed" | "waiting" | "working" | "recovering" | "unread";

export interface AgentRailTrailingState {
  kind: AgentRailTrailingStateKind;
  label: string;
}

export function agentRailTrailingState(input: {
  status?: SessionStatus;
  unread: boolean;
}): AgentRailTrailingState | null {
  if (
    input.status === "failed" ||
    input.status === "waiting" ||
    input.status === "working" ||
    input.status === "recovering"
  ) {
    return {
      kind: input.status,
      label: sessionRailStatusLabel(input.status),
    };
  }
  return input.unread ? { kind: "unread", label: "Unread" } : null;
}

export function compareAgentRailViewChats(
  left: AgentRailChat,
  right: AgentRailChat,
  ordering: AgentRailOrdering,
): number {
  if (ordering === "manual") return left.position - right.position;
  return (
    (right.updatedAt ?? 0) - (left.updatedAt ?? 0) ||
    left.position - right.position
  );
}
