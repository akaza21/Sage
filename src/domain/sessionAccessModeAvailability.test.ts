import { describe, expect, it } from "vitest";

import {
  DEFAULT_SESSION_ACCESS_MODE,
  SESSION_ACCESS_MODES,
  SESSION_ACCESS_MODE_SCOPE,
  admitSessionAccessMode,
  sessionAccessModeAvailability,
  sessionAccessModeCopy,
} from "./sessionAccessMode";

describe("session access mode availability", () => {
  it("keeps read-only and workspace write available without permission prompts", () => {
    for (const mode of ["readOnly", "workspaceWrite"] as const) {
      expect(sessionAccessModeAvailability(mode, false)).toEqual({
        effective: mode,
        unavailableReason: null,
      });
      expect(sessionAccessModeAvailability(mode, true)).toEqual({
        effective: mode,
        unavailableReason: null,
      });
    }
  });

  it("degrades ask to read-only until permission prompts are confirmed", () => {
    expect(sessionAccessModeAvailability("ask", false)).toEqual({
      effective: "readOnly",
      unavailableReason:
        "This harness has not confirmed support for permission prompts.",
    });
  });

  it("admits ask once permission prompts are confirmed", () => {
    expect(sessionAccessModeAvailability("ask", true)).toEqual({
      effective: "ask",
      unavailableReason: null,
    });
  });

  it("fails closed on an unknown capability value", () => {
    expect(
      sessionAccessModeAvailability("ask", undefined as unknown as boolean)
        .effective,
    ).toBe("readOnly");
    expect(
      sessionAccessModeAvailability("ask", null as unknown as boolean)
        .effective,
    ).toBe("readOnly");
  });

  it("degrades an unknown requested mode before gating it", () => {
    expect(sessionAccessModeAvailability("elevated" as never, true)).toEqual({
      effective: "readOnly",
      unavailableReason: null,
    });
  });
});

describe("session access mode", () => {
  it("keeps the frozen list and default", () => {
    expect([...SESSION_ACCESS_MODES]).toEqual([
      "readOnly",
      "ask",
      "workspaceWrite",
    ]);
    expect(DEFAULT_SESSION_ACCESS_MODE).toBe("readOnly");
    expect(admitSessionAccessMode("nope")).toBe("readOnly");
    expect(sessionAccessModeCopy("ask").label).toBe("Ask each time");
  });

  it("states one scope sentence for every permission mode surface", () => {
    expect(SESSION_ACCESS_MODE_SCOPE).toBe(
      "Applies from your next message in any chat.",
    );
  });
});
