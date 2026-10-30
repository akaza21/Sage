import { describe, expect, it } from "vitest";
import { attention } from "../../../domain/chatAttention.fixture";
import {
  sessionRailGitLabel,
  sessionRailLive,
  sessionRailRelativeTime,
  sessionRailStatusLabel,
  sessionRailUnreadVisible,
} from "./sessionRail";

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
  it("hides focused detail without acknowledging persistent attention", () => {
    const summary = attention({ readPosition: "0" });
    const before = structuredClone(summary);
    expect(sessionRailUnreadVisible(summary, true)).toBe(false);
    expect(sessionRailUnreadVisible(summary, false)).toBe(true);
    expect(summary).toEqual(before);
  });
  it("does not turn a never-visited completion or a live process into unread", () => {
    expect(sessionRailUnreadVisible(attention(), false)).toBe(false);
    expect(
      sessionRailUnreadVisible(
        attention({ running: true, terminalPosition: null }),
        false,
      ),
    ).toBe(false);
    expect(sessionRailLive("working")).toBe(true);
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
