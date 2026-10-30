import { afterEach, describe, expect, it } from "vitest";
import {
  ackChatUnread,
  clearUnreadAcks,
  seenChatUnreadFingerprint,
} from "./chatUnread";

import {
  MAX_SESSION_RAIL_ACK_SLOTS,
  ackSessionRailUnread,
  clearSessionRailAcks,
  pinSessionRailIds,
  seenSessionRailFingerprint,
  sessionRailFingerprint,
  sessionRailGitLabel,
  sessionRailLive,
  sessionRailRelativeTime,
  sessionRailStatusLabel,
  sessionRailUnreadVisible,
} from "./sessionRail";

afterEach(() => {
  clearSessionRailAcks();
  clearUnreadAcks();
});

describe("sessionRailStatusLabel", () => {
  it("uses Devin session-card status lines", () => {
    expect(sessionRailStatusLabel("working")).toBe("Working…");
    expect(sessionRailStatusLabel("waiting")).toBe("Waiting");
    expect(sessionRailStatusLabel("complete")).toBe("Done");
  });

  it("names a chat waiting on a permission Awaiting approval", () => {
    expect(sessionRailStatusLabel("waiting", true)).toBe("Awaiting approval");
    expect(sessionRailStatusLabel("working", true)).toBe("Awaiting approval");
    expect(sessionRailStatusLabel("waiting", false)).toBe("Waiting");
    expect(sessionRailStatusLabel("failed")).toBe("Failed");
    expect(sessionRailStatusLabel("idle")).toBe("");
    expect(sessionRailStatusLabel(undefined)).toBe("");
  });
});

describe("sessionRailUnreadVisible", () => {
  it("hides the rail dot on the focused session", () => {
    expect(
      sessionRailUnreadVisible({
        focused: true,
        status: "working",
        seenFingerprint: undefined,
      }),
    ).toBe(false);
  });

  it("shows the rail dot on an unfocused working or waiting session", () => {
    expect(sessionRailLive("working")).toBe(true);
    expect(sessionRailLive("waiting")).toBe(true);
    expect(
      sessionRailUnreadVisible({
        focused: false,
        status: "working",
        seenFingerprint: undefined,
      }),
    ).toBe(true);
    expect(
      sessionRailUnreadVisible({
        focused: false,
        status: "waiting",
        seenFingerprint: "Working…",
      }),
    ).toBe(true);
  });

  it("hides the rail dot on a never-started idle session", () => {
    expect(sessionRailFingerprint("idle")).toBe("");
    expect(
      sessionRailUnreadVisible({
        focused: false,
        status: "idle",
        seenFingerprint: undefined,
      }),
    ).toBe(false);
  });

  it("keeps the rail dot after Done until that session is seen", () => {
    expect(
      sessionRailUnreadVisible({
        focused: false,
        status: "complete",
        seenFingerprint: undefined,
      }),
    ).toBe(true);
    expect(
      sessionRailUnreadVisible({
        focused: false,
        status: "complete",
        seenFingerprint: "Done",
      }),
    ).toBe(false);
  });
});

describe("sessionRailRelativeTime", () => {
  const now = 1_700_000_000_000;

  it("uses Devin compact session-card ages", () => {
    expect(sessionRailRelativeTime(now - 12_000, now)).toBe("now");
    expect(sessionRailRelativeTime(now - 2 * 60_000, now)).toBe("2m");
    expect(sessionRailRelativeTime(now - 3 * 3_600_000, now)).toBe("3h");
    expect(sessionRailRelativeTime(now - 30 * 3_600_000, now)).toBe(
      "yesterday",
    );
    expect(sessionRailRelativeTime(now - 4 * 86_400_000, now)).toBe("4d");
  });

  it("fails closed on invalid clocks", () => {
    expect(sessionRailRelativeTime(Number.NaN, now)).toBe("");
    expect(sessionRailRelativeTime(-1, now)).toBe("");
    expect(sessionRailRelativeTime(now + 3_600_000, now)).toBe("");
  });
});

describe("sessionRailGitLabel", () => {
  it("shows a named branch and hides detached HEAD", () => {
    expect(sessionRailGitLabel("abcdefghij0123456789ABCDEF")).toBe(
      "abcdefghij0123456789ABC…",
    );
    expect(sessionRailGitLabel("main")).toBe("main");
    expect(sessionRailGitLabel("main (detached)")).toBe("");
    expect(sessionRailGitLabel(null)).toBe("");
  });
});

describe("session rail ack slots", () => {
  it("caps rail acks at the attached-chat bound", () => {
    expect(MAX_SESSION_RAIL_ACK_SLOTS).toBe(64);
    pinSessionRailIds(["chat-a", "chat-b"]);
    ackSessionRailUnread("chat-a", "Working…");
    ackSessionRailUnread("chat-b", "Done");
    expect(seenSessionRailFingerprint("chat-a")).toBe("Working…");
    pinSessionRailIds(["chat-b"]);
    expect(seenSessionRailFingerprint("chat-a")).toBeUndefined();
    expect(seenSessionRailFingerprint("chat-b")).toBe("Done");
  });
});

it("keeps the 64-slot rail registry independent from the two-pane registry", () => {
  const ids = Array.from({ length: 64 }, (_, index) => `chat-${index}`);
  pinSessionRailIds(ids);
  for (const id of ids) ackSessionRailUnread(id, "Done");
  ackChatUnread(ids[0], "Pane seen");
  ackSessionRailUnread("overflow", "Waiting");
  expect(seenSessionRailFingerprint(ids[0])).toBeUndefined();
  expect(seenSessionRailFingerprint(ids[1])).toBe("Done");
  expect(seenSessionRailFingerprint(ids[63])).toBe("Done");
  expect(seenSessionRailFingerprint("overflow")).toBe("Waiting");
  expect(seenChatUnreadFingerprint(ids[0])).toBe("Pane seen");
  clearSessionRailAcks();
  expect(seenChatUnreadFingerprint(ids[0])).toBe("Pane seen");
});
