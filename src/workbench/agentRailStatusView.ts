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
export function railChatAge(updatedAt: number | undefined, now: number) {
  if (!((updatedAt as number) > 0)) return null;
  let age = Math.max(0, now - updatedAt!) / 60_000;
  if (age < 1) return "now";
  for (const [unit, size] of [
    ["m", 60],
    ["h", 24],
    ["d", 7],
  ] as const) {
    if (age < size) return `${Math.floor(age)}${unit}`;
    age /= size;
  }
  return `${Math.floor(age)}w`;
}

// One shared minute clock for row ages; it runs only while a row is shown.
let minuteNow = Date.now();
let minuteTimer: ReturnType<typeof setInterval> | undefined;
const minuteListeners = new Set<() => void>();

export function subscribeRailMinute(listener: () => void) {
  minuteListeners.add(listener);
  if (!minuteTimer) {
    minuteNow = Date.now();
    minuteTimer = setInterval(() => {
      minuteNow = Date.now();
      minuteListeners.forEach((notify) => notify());
    }, 60_000);
  }
  return () => {
    minuteListeners.delete(listener);
    if (!minuteListeners.size) minuteTimer = void clearInterval(minuteTimer);
  };
}

export const railMinuteNow = () => minuteNow;
