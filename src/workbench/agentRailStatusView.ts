import type { AgentRailChat, SessionStatus } from "../domain/workbench";
import {
  sessionRailRelativeTime,
  sessionRailStatusLabel,
} from "../features/chat/acp/sessionRail";
import type { AgentRailProjectGroup } from "./agentChatRail";

export const MAX_AGENT_RAIL_STATUS_SECTION = 64;

export const AGENT_RAIL_GROUPINGS = ["folder", "status"] as const;
export type AgentRailGrouping = (typeof AGENT_RAIL_GROUPINGS)[number];

export const AGENT_RAIL_ORDERINGS = ["recency", "title"] as const;
export type AgentRailOrdering = (typeof AGENT_RAIL_ORDERINGS)[number];

export const AGENT_RAIL_SHOW_FIELDS = [
  "updated",
  "workspace",
  "branch",
] as const;
export type AgentRailShowField = (typeof AGENT_RAIL_SHOW_FIELDS)[number];

export const AGENT_RAIL_OMITTED_SHOW_FIELDS = [
  "environment",
  "pr",
  "machine",
] as const;

export const AGENT_RAIL_FILTER_FIELDS = ["status", "archived"] as const;

export const AGENT_RAIL_OMITTED_FILTER_FIELDS = [
  "pr",
  "environment",
  "source",
] as const;

export const AGENT_RAIL_STATUS_SECTIONS = [
  "attention",
  "working",
  "done",
] as const;
export type AgentRailStatusSectionId =
  (typeof AGENT_RAIL_STATUS_SECTIONS)[number];

export const AGENT_RAIL_STATUS_LABELS: Record<
  AgentRailStatusSectionId,
  string
> = {
  attention: "Needs Attention",
  working: "Working",
  done: "Done",
};

export type AgentRailStatusFilter = "all" | AgentRailStatusSectionId;

export interface AgentRailStatusSection {
  id: AgentRailStatusSectionId;
  label: string;
  groups: AgentRailProjectGroup[];
  truncated: boolean;
}

export type AgentRailTrailingStateKind =
  "failed" | "waiting" | "working" | "recovering" | "unread" | "time";

export interface AgentRailTrailingState {
  kind: AgentRailTrailingStateKind;
  label: string;
}

export function agentRailTrailingState(input: {
  status?: SessionStatus;
  unread: boolean;
  stamp: string;
}): AgentRailTrailingState | null {
  if (input.status === "failed" || input.status === "waiting") {
    return {
      kind: input.status,
      label: sessionRailStatusLabel(input.status),
    };
  }
  if (input.status === "working" || input.status === "recovering") {
    return {
      kind: input.status,
      label: sessionRailStatusLabel(input.status),
    };
  }
  if (input.unread) return { kind: "unread", label: "Unread" };
  if (input.stamp) return { kind: "time", label: input.stamp };
  return null;
}

export function agentRailAgeStamp(input: {
  updatedAt?: number;
  live: boolean;
  nowMs: number;
}): string {
  if (
    typeof input.updatedAt === "number" &&
    Number.isFinite(input.updatedAt) &&
    input.updatedAt > 0
  ) {
    return sessionRailRelativeTime(input.updatedAt, input.nowMs);
  }
  return input.live ? "now" : "";
}

export function classifyAgentRailSection(input: {
  status?: SessionStatus;
  unread: boolean;
}): AgentRailStatusSectionId {
  if (input.status === "working" || input.status === "recovering") {
    return "working";
  }
  if (input.status === "waiting" || input.status === "failed" || input.unread) {
    return "attention";
  }
  return "done";
}

function compareRailChats(
  left: AgentRailChat,
  right: AgentRailChat,
  ordering: AgentRailOrdering,
): number {
  if (ordering === "title") {
    return left.title.localeCompare(right.title);
  }
  const leftAt = left.updatedAt ?? 0;
  const rightAt = right.updatedAt ?? 0;
  if (leftAt === rightAt) return left.position - right.position;
  return rightAt - leftAt;
}

export function groupAgentRailByStatus(input: {
  groups: ReadonlyArray<AgentRailProjectGroup>;
  unreadByChatId: Readonly<Record<string, boolean>>;
  statusFilter?: AgentRailStatusFilter;
  ordering?: AgentRailOrdering;
}): AgentRailStatusSection[] {
  const ordering = input.ordering ?? "recency";
  const wanted = input.statusFilter ?? "all";
  const sections: AgentRailStatusSection[] = [];
  for (const id of AGENT_RAIL_STATUS_SECTIONS) {
    if (wanted !== "all" && wanted !== id) continue;
    const nextGroups: AgentRailProjectGroup[] = [];
    let truncated = false;
    let remaining = MAX_AGENT_RAIL_STATUS_SECTION;
    for (const group of input.groups) {
      const chats = group.chats
        .filter(
          (chat) =>
            classifyAgentRailSection({
              status: chat.status,
              unread: input.unreadByChatId[chat.id] === true,
            }) === id,
        )
        .sort((left, right) => compareRailChats(left, right, ordering));
      if (chats.length === 0) continue;
      if (remaining <= 0) {
        truncated = true;
        break;
      }
      const sliced = chats.slice(0, remaining);
      if (sliced.length < chats.length || group.truncated) truncated = true;
      remaining -= sliced.length;
      nextGroups.push({
        ...group,
        chats: sliced,
        archived: id === "done" ? group.archived : [],
        truncated: false,
      });
    }
    if (nextGroups.length === 0 && !truncated) continue;
    sections.push({
      id,
      label: AGENT_RAIL_STATUS_LABELS[id],
      groups: nextGroups,
      truncated,
    });
  }
  return sections;
}
