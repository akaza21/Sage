import type { SessionStatus } from "../../../domain/workbench";
import { currentLocalBranchName } from "../../git/localBranches";
import { unreadDotVisible } from "./chatUnread";

export const MAX_SESSION_RAIL_ACK_SLOTS = 64;
export const MAX_SESSION_RAIL_GIT_CHARS = 24;

const railAcks = new Map<string, string>();
const pinnedRailIds = new Set<string>();
const railActivityAt = new Map<string, number>();
const railActivityFingerprint = new Map<string, string>();

export function pinSessionRailIds(ids: readonly string[]): void {
  pinnedRailIds.clear();
  for (const id of ids) {
    if (pinnedRailIds.size >= MAX_SESSION_RAIL_ACK_SLOTS) break;
    pinnedRailIds.add(id);
  }
  for (const id of [...railAcks.keys()]) {
    if (!pinnedRailIds.has(id)) railAcks.delete(id);
  }
  for (const id of [...railActivityAt.keys()]) {
    if (!pinnedRailIds.has(id)) {
      railActivityAt.delete(id);
      railActivityFingerprint.delete(id);
    }
  }
}

function evictUnpinnedRailAck(requestedId: string): boolean {
  for (const id of railAcks.keys()) {
    if (id !== requestedId && !pinnedRailIds.has(id)) {
      railAcks.delete(id);
      return true;
    }
  }
  return false;
}

export function ackSessionRailUnread(
  chatId: string,
  fingerprint: string,
): void {
  if (railAcks.has(chatId)) {
    railAcks.set(chatId, fingerprint);
    return;
  }
  if (railAcks.size >= MAX_SESSION_RAIL_ACK_SLOTS) {
    if (!evictUnpinnedRailAck(chatId)) {
      const oldest = railAcks.keys().next().value;
      if (oldest !== undefined) railAcks.delete(oldest);
    }
  }
  railAcks.set(chatId, fingerprint);
}

export function seenSessionRailFingerprint(chatId: string): string | undefined {
  return railAcks.get(chatId);
}

export function clearSessionRailAcks(): void {
  railAcks.clear();
  pinnedRailIds.clear();
  railActivityAt.clear();
  railActivityFingerprint.clear();
}

export function noteSessionRailActivity(
  chatId: string,
  fingerprint: string,
  nowMs: number,
): void {
  if (fingerprint.length === 0 || !Number.isFinite(nowMs) || nowMs <= 0) {
    return;
  }
  if (railActivityFingerprint.get(chatId) === fingerprint) {
    return;
  }
  if (
    !railActivityAt.has(chatId) &&
    railActivityAt.size >= MAX_SESSION_RAIL_ACK_SLOTS
  ) {
    const oldest = railActivityAt.keys().next().value;
    if (oldest !== undefined) {
      railActivityAt.delete(oldest);
      railActivityFingerprint.delete(oldest);
    }
  }
  railActivityFingerprint.set(chatId, fingerprint);
  railActivityAt.set(chatId, nowMs);
}

export function sessionRailActivityAt(chatId: string): number | undefined {
  return railActivityAt.get(chatId);
}

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

export function sessionRailDisplayTime(
  chatId: string,
  updatedAtMs: number | undefined,
  nowMs: number,
): string {
  const durable =
    updatedAtMs !== undefined && Number.isFinite(updatedAtMs) && updatedAtMs > 0
      ? updatedAtMs
      : sessionRailActivityAt(chatId);
  return durable === undefined ? "" : sessionRailRelativeTime(durable, nowMs);
}

export function sessionRailCalendarLabel(updatedAtMs: number): string {
  if (!Number.isFinite(updatedAtMs) || updatedAtMs <= 0) {
    return "";
  }
  const date = new Date(updatedAtMs);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  }).format(date);
}

export function sessionRailCardTime(input: {
  updatedAt?: number;
  live: boolean;
}): string {
  if (input.updatedAt !== undefined) {
    return sessionRailCalendarLabel(input.updatedAt);
  }
  return input.live ? "now" : "";
}

export function sessionRailStatusLabel(
  status: SessionStatus | undefined,
): string {
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
