import { describe, expect, it } from "vitest";

import {
  DEFAULT_SESSION_ACCESS_MODE,
  SESSION_ACCESS_MODES,
  admitSessionAccessMode,
  admitSessionAccessModes,
} from "./sessionAccessMode";

describe("session access mode", () => {
  it("defaults to the fail-closed read-only mode", () => {
    expect(DEFAULT_SESSION_ACCESS_MODE).toBe("readOnly");
    expect(SESSION_ACCESS_MODES[0]).toBe("readOnly");
  });

  it("carries the modes the backend accepts", () => {
    expect([...SESSION_ACCESS_MODES]).toEqual([
      "readOnly",
      "ask",
      "workspaceWrite",
      "acceptEdits",
      "plan",
      "auto",
      "fullAccess",
    ]);
  });

  it("never admits Full access as a stored default", () => {
    expect(
      admitSessionAccessModes({
        "claude-agent-acp": "fullAccess",
        other: "acceptEdits",
        third: "plan",
      }),
    ).toEqual({ other: "acceptEdits", third: "plan" });
  });

  it("admits each known mode unchanged", () => {
    for (const mode of SESSION_ACCESS_MODES)
      expect(admitSessionAccessMode(mode)).toBe(mode);
  });

  it("degrades an unknown persisted value to read-only", () => {
    expect(admitSessionAccessMode("bypassPermissions")).toBe("readOnly");
    expect(admitSessionAccessMode(null)).toBe("readOnly");
    expect(admitSessionAccessMode(undefined)).toBe("readOnly");
    expect(admitSessionAccessMode(7)).toBe("readOnly");
  });
});
