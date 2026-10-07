import { describe, expect, it } from "vitest";

import {
  DEFAULT_SESSION_ACCESS_MODE,
  SESSION_ACCESS_MODES,
  admitSessionAccessMode,
  sessionAccessModeCopy,
} from "./sessionAccessMode";

describe("session access mode", () => {
  it("defaults to the fail-closed read-only mode", () => {
    expect(DEFAULT_SESSION_ACCESS_MODE).toBe("readOnly");
    expect(SESSION_ACCESS_MODES[0]).toBe("readOnly");
  });

  it("carries the three modes the backend accepts", () => {
    expect([...SESSION_ACCESS_MODES]).toEqual([
      "readOnly",
      "ask",
      "workspaceWrite",
    ]);
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

  it("gives every mode a label and a description", () => {
    for (const mode of SESSION_ACCESS_MODES) {
      expect(sessionAccessModeCopy(mode).label.length).toBeGreaterThan(0);
      expect(sessionAccessModeCopy(mode).description.length).toBeGreaterThan(0);
    }
  });
});
