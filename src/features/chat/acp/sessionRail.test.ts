import { afterEach, describe, expect, it } from "vitest";

import {
  MAX_SESSION_RAIL_ACK_SLOTS,
  ackSessionRailUnread,
  clearSessionRailAcks,
  noteSessionRailActivity,
  pinSessionRailIds,
  seenSessionRailFingerprint,
  sessionRailActivityAt,
  sessionRailCalendarLabel,
  sessionRailCardTime,
  sessionRailDisplayTime,
  sessionRailFingerprint,
  sessionRailGitLabel,
  sessionRailLive,
  sessionRailRelativeTime,
  sessionRailStatusLabel,
  sessionRailUnreadVisible,
} from "./sessionRail";

afterEach(() => {
  clearSessionRailAcks();
});

describe("sessionRailStatusLabel", () => {
  it("uses Devin session-card status lines", () => {
    expect(sessionRailStatusLabel("working")).toBe("Working…");
    expect(sessionRailStatusLabel("waiting")).toBe("Waiting");
    expect(sessionRailStatusLabel("complete")).toBe("Done");
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

describe("sessionRailCardTime", () => {
  it("uses a UTC calendar stamp or live now", () => {
    expect(sessionRailCalendarLabel(Date.UTC(2026, 7, 26, 12, 0, 0))).toBe(
      "Aug 26",
    );
    expect(
      sessionRailCardTime({
        updatedAt: Date.UTC(2026, 7, 26, 12, 0, 0),
        live: false,
      }),
    ).toBe("Aug 26");
    expect(sessionRailCardTime({ live: true })).toBe("now");
    expect(sessionRailCardTime({ live: false })).toBe("");
  });
});

describe("session rail activity clock", () => {
  it("prefers a durable updatedAt over the session-local stamp", () => {
    const now = 1_700_000_000_000;
    noteSessionRailActivity("chat-a", "Working…", now);
    expect(sessionRailActivityAt("chat-a")).toBe(now);
    expect(sessionRailDisplayTime("chat-a", now - 2 * 60_000, now)).toBe("2m");
    expect(sessionRailDisplayTime("chat-a", undefined, now)).toBe("now");
  });

  it("does not stamp idle sessions", () => {
    noteSessionRailActivity("chat-idle", "", 1_700_000_000_000);
    expect(sessionRailActivityAt("chat-idle")).toBeUndefined();
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
