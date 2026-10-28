import type { ChatLiveSnapshot } from "./chatLiveSnapshot";

export const MAX_UNREAD_ACK_SLOTS = 2;

export function createUnreadAcks(maximumSlots: 2 | 64) {
  const acks = new Map<string, string>();
  const pinned = new Set<string>();
  return {
    pin(ids: readonly string[]) {
      pinned.clear();
      for (const id of ids) {
        if (pinned.size >= maximumSlots) break;
        pinned.add(id);
      }
      for (const id of acks.keys()) if (!pinned.has(id)) acks.delete(id);
    },
    ack(chatId: string, fingerprint: string) {
      if (!acks.has(chatId) && acks.size >= maximumSlots) {
        const evict =
          [...acks.keys()].find((id) => !pinned.has(id)) ??
          acks.keys().next().value;
        if (evict !== undefined) acks.delete(evict);
      }
      acks.set(chatId, fingerprint);
    },
    seen(chatId: string) {
      return acks.get(chatId);
    },
    clear() {
      acks.clear();
      pinned.clear();
    },
  };
}

export const {
  pin: pinUnreadAckIds,
  ack: ackChatUnread,
  seen: seenChatUnreadFingerprint,
  clear: clearUnreadAcks,
} = createUnreadAcks(MAX_UNREAD_ACK_SLOTS);

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
