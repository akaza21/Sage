import { afterEach, describe, expect, it } from "vitest";

import type { ChatLiveSnapshot } from "./chatLiveSnapshot";
import {
  MAX_UNREAD_ACK_SLOTS,
  ackChatUnread,
  chatActivityFingerprint,
  clearUnreadAcks,
  pinUnreadAckIds,
  seenChatUnreadFingerprint,
  snapshotIsLive,
  unreadDotVisible,
} from "./chatUnread";

afterEach(() => {
  clearUnreadAcks();
});

const idle: ChatLiveSnapshot = {
  chatId: "chat-2",
  statusLabel: "Completed",
  statusTone: "idle",
  latestText: "Done.",
  streaming: false,
  waiting: false,
  working: false,
};

const neverStarted: ChatLiveSnapshot = {
  chatId: "chat-2",
  statusLabel: "Idle",
  statusTone: "idle",
  latestText: "",
  streaming: false,
  waiting: false,
  working: false,
};

describe("chatActivityFingerprint", () => {
  it("returns an empty fingerprint for a never-started idle snapshot", () => {
    expect(snapshotIsLive(neverStarted)).toBe(false);
    expect(chatActivityFingerprint(neverStarted)).toBe("");
  });

  it("keeps a finished-turn fingerprint when latestText is empty", () => {
    expect(
      chatActivityFingerprint({
        ...neverStarted,
        statusLabel: "Completed",
      }),
    ).toBe("Completed:");
    expect(
      chatActivityFingerprint({
        ...neverStarted,
        statusLabel: "Failed",
      }),
    ).toBe("Failed:");
  });
});

describe("unreadDotVisible", () => {
  it("hides the Devin unread dot on the focused pane", () => {
    expect(
      unreadDotVisible({
        focused: true,
        live: true,
        activityFingerprint: chatActivityFingerprint(idle),
        seenFingerprint: undefined,
      }),
    ).toBe(false);
  });

  it("shows the unread dot on an unfocused live turn", () => {
    expect(
      unreadDotVisible({
        focused: false,
        live: true,
        activityFingerprint: "Running:working",
        seenFingerprint: "Idle:",
      }),
    ).toBe(true);
  });

  it("hides the unread dot when an unfocused idle pane has no activity fingerprint", () => {
    expect(
      unreadDotVisible({
        focused: false,
        live: false,
        activityFingerprint: chatActivityFingerprint(neverStarted),
        seenFingerprint: undefined,
      }),
    ).toBe(false);
  });

  it("still treats an empty-text completed turn as unseen when unfocused", () => {
    const fingerprint = chatActivityFingerprint({
      ...neverStarted,
      statusLabel: "Completed",
    });
    expect(fingerprint).toBe("Completed:");
    expect(
      unreadDotVisible({
        focused: false,
        live: false,
        activityFingerprint: fingerprint,
        seenFingerprint: undefined,
      }),
    ).toBe(true);
  });

  it("keeps the unread dot after an unfocused turn finishes until the pane is seen", () => {
    const fingerprint = chatActivityFingerprint(idle);
    expect(snapshotIsLive(idle)).toBe(false);
    expect(
      unreadDotVisible({
        focused: false,
        live: false,
        activityFingerprint: fingerprint,
        seenFingerprint: "Running:partial",
      }),
    ).toBe(true);
    expect(
      unreadDotVisible({
        focused: false,
        live: false,
        activityFingerprint: fingerprint,
        seenFingerprint: fingerprint,
      }),
    ).toBe(false);
  });
});

describe("unread ack slots", () => {
  it("reuses a slot when the same visible chat is seen again", () => {
    ackChatUnread("chat-a", "Running:one");
    ackChatUnread("chat-a", "Completed:Done.");
    expect(seenChatUnreadFingerprint("chat-a")).toBe("Completed:Done.");
    expect(MAX_UNREAD_ACK_SLOTS).toBe(2);
    expect(seenChatUnreadFingerprint("chat-b")).toBeUndefined();
  });

  it("evicts an unpinned ack instead of growing past two visible panes", () => {
    ackChatUnread("chat-a", "Idle:");
    ackChatUnread("chat-b", "Running:two");
    pinUnreadAckIds(["chat-b", "chat-c"]);
    ackChatUnread("chat-c", "Completed:Done.");
    expect(seenChatUnreadFingerprint("chat-a")).toBeUndefined();
    expect(seenChatUnreadFingerprint("chat-b")).toBe("Running:two");
    expect(seenChatUnreadFingerprint("chat-c")).toBe("Completed:Done.");
  });

  it("evicts the oldest ack when a third chat is seen and every slot is pinned", () => {
    pinUnreadAckIds(["chat-a", "chat-b"]);
    ackChatUnread("chat-a", "Idle:");
    ackChatUnread("chat-b", "Running:two");
    ackChatUnread("chat-c", "Completed:Done.");
    expect(seenChatUnreadFingerprint("chat-a")).toBeUndefined();
    expect(seenChatUnreadFingerprint("chat-b")).toBe("Running:two");
    expect(seenChatUnreadFingerprint("chat-c")).toBe("Completed:Done.");
  });
});
