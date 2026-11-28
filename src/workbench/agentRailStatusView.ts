import type { AgentRailChat, SessionStatus } from "../domain/workbench";
import {
  sessionRailLive,
  sessionRailStatusLabel,
  type RailAttention,
} from "../features/chat/acp/sessionRail";

export const AGENT_RAIL_ORGANIZE_MODES = ["project", "list"] as const;
export type AgentRailOrganizeMode = (typeof AGENT_RAIL_ORGANIZE_MODES)[number];

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
  attention?: RailAttention;
}): AgentRailTrailingState | null {
  if (input.status === "failed" || sessionRailLive(input.status)) {
    return {
      kind: input.status as AgentRailTrailingStateKind,
      label: sessionRailStatusLabel(input.status, input.attention),
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

/** A chat row's age at `now`: "now" under a minute, then minutes, hours,
 *  days and weeks ("3m", "2h", "1d", "3w"). Null without a usable time. */
export function railChatAge(
  updatedAt: number | undefined,
  now: number,
): string | null {
  if (typeof updatedAt !== "number" || !Number.isFinite(updatedAt)) return null;
  if (updatedAt <= 0) return null;
  const minutes = Math.floor(Math.max(0, now - updatedAt) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  return days < 7 ? `${days}d` : `${Math.floor(days / 7)}w`;
}
