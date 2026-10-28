import type { SessionStatus } from "../../../domain/workbench";
import { currentLocalBranchName } from "../../git/localBranches";
import { createUnreadAcks, unreadDotVisible } from "./chatUnread";

export const MAX_SESSION_RAIL_ACK_SLOTS = 64;
export const MAX_SESSION_RAIL_GIT_CHARS = 24;

export const {
  pin: pinSessionRailIds,
  ack: ackSessionRailUnread,
  seen: seenSessionRailFingerprint,
  clear: clearSessionRailAcks,
} = createUnreadAcks(MAX_SESSION_RAIL_ACK_SLOTS);

export function sessionRailRelativeTime(
  updatedAtMs: number,
  nowMs: number,
): string {
  if (
    !Number.isFinite(updatedAtMs) ||
    !Number.isFinite(nowMs) ||
    updatedAtMs <= 0 ||
    nowMs < updatedAtMs
  ) {
    return "";
  }
  const delta = nowMs - updatedAtMs;
  if (delta < 45_000) {
    return "now";
  }
  if (delta < 60 * 60_000) {
    return `${Math.max(1, Math.round(delta / 60_000))}m`;
  }
  if (delta < 24 * 60 * 60_000) {
    return `${Math.max(1, Math.round(delta / 3_600_000))}h`;
  }
  if (delta < 48 * 60 * 60_000) {
    return "yesterday";
  }
  const days = Math.round(delta / 86_400_000);
  return days > 99 ? "99d+" : `${days}d`;
}

export function sessionRailGitLabel(branch: string | null | undefined): string {
  const name = currentLocalBranchName(branch);
  if (!name) {
    return "";
  }
  return name.length <= MAX_SESSION_RAIL_GIT_CHARS
    ? name
    : `${name.slice(0, MAX_SESSION_RAIL_GIT_CHARS - 1)}…`;
}

export function sessionRailStatusLabel(
  status: SessionStatus | undefined,
  awaitingApproval = false,
): string {
  if (awaitingApproval) return "Awaiting approval";
  switch (status) {
    case "working":
      return "Working…";
    case "waiting":
      return "Waiting";
    case "recovering":
      return "Recovering";
    case "failed":
      return "Failed";
    case "complete":
      return "Done";
    default:
      return "";
  }
}

export function sessionRailLive(status: SessionStatus | undefined): boolean {
  return (
    status === "working" || status === "waiting" || status === "recovering"
  );
}

export function sessionRailFingerprint(
  status: SessionStatus | undefined,
): string {
  return sessionRailStatusLabel(status);
}

export function sessionRailUnreadVisible(input: {
  focused: boolean;
  status: SessionStatus | undefined;
  seenFingerprint: string | undefined;
}): boolean {
  return unreadDotVisible({
    focused: input.focused,
    live: sessionRailLive(input.status),
    activityFingerprint: sessionRailFingerprint(input.status),
    seenFingerprint: input.seenFingerprint,
  });
}
