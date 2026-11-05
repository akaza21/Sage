import { describe, expect, it } from "vitest";

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
  it("hides the rail dot on the focused session", () => {
    expect(
      sessionRailUnreadVisible({
        focused: true,
        status: "working",
        readThrough: undefined,
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
        readThrough: undefined,
      }),
    ).toBe(true);
    expect(
      sessionRailUnreadVisible({
        focused: false,
        status: "waiting",
        readThrough: 100,
      }),
    ).toBe(true);
  });

  it("hides the rail dot on a never-started idle session", () => {
    expect(
      sessionRailUnreadVisible({
        focused: false,
        status: "idle",
        readThrough: undefined,
      }),
    ).toBe(false);
  });

  it("keeps the rail dot after Done until that update is seen", () => {
    const done = {
      focused: false,
      status: "complete" as const,
      updatedAt: 200,
    };
    expect(sessionRailUnreadVisible({ ...done, readThrough: undefined })).toBe(
      true,
    );
    expect(sessionRailUnreadVisible({ ...done, readThrough: 200 })).toBe(false);
    expect(sessionRailUnreadVisible({ ...done, readThrough: 100 })).toBe(true);
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
