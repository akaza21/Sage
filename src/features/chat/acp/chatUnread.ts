import type { ChatLiveSnapshot } from "./chatLiveSnapshot";

export const MAX_UNREAD_ACK_SLOTS = 2;

const unreadAcks = new Map<string, string>();
const pinnedUnreadIds = new Set<string>();

export function pinUnreadAckIds(ids: readonly string[]): void {
  pinnedUnreadIds.clear();
  for (const id of ids) {
    if (pinnedUnreadIds.size >= MAX_UNREAD_ACK_SLOTS) break;
    pinnedUnreadIds.add(id);
  }
  for (const id of [...unreadAcks.keys()]) {
    if (!pinnedUnreadIds.has(id)) unreadAcks.delete(id);
  }
}

function evictUnpinnedAck(requestedId: string): boolean {
  for (const id of unreadAcks.keys()) {
    if (id !== requestedId && !pinnedUnreadIds.has(id)) {
      unreadAcks.delete(id);
      return true;
    }
  }
  return false;
}

export function ackChatUnread(chatId: string, fingerprint: string): void {
  if (unreadAcks.has(chatId)) {
    unreadAcks.set(chatId, fingerprint);
    return;
  }
  if (unreadAcks.size >= MAX_UNREAD_ACK_SLOTS) {
    if (!evictUnpinnedAck(chatId)) {
      const oldest = unreadAcks.keys().next().value;
      if (oldest !== undefined) unreadAcks.delete(oldest);
    }
  }
  unreadAcks.set(chatId, fingerprint);
}

export function seenChatUnreadFingerprint(chatId: string): string | undefined {
  return unreadAcks.get(chatId);
}

export function clearUnreadAcks(): void {
  unreadAcks.clear();
  pinnedUnreadIds.clear();
}

export function chatActivityFingerprint(snapshot: ChatLiveSnapshot): string {
  if (
    !snapshotIsLive(snapshot) &&
    snapshot.statusLabel === "Idle" &&
    snapshot.latestText.length === 0
  ) {
    return "";
  }
  return `${snapshot.statusLabel}:${snapshot.latestText}`;
}

export function unreadDotVisible(input: {
  focused: boolean;
  live: boolean;
  activityFingerprint: string;
  seenFingerprint: string | undefined;
}): boolean {
  if (input.focused) return false;
  if (input.live) return true;
  if (input.activityFingerprint.length === 0) return false;
  return input.seenFingerprint !== input.activityFingerprint;
}

export function snapshotIsLive(snapshot: ChatLiveSnapshot): boolean {
  return snapshot.working || snapshot.waiting || snapshot.streaming;
}
