import { describe, expect, it } from "vitest";
import {
  sessionAccessModeAvailability,
  type SessionAccessModeCapability,
} from "./sessionAccessMode";

const readOnly: SessionAccessModeCapability = {
  id: "readOnly",
  label: "Read-only",
  description: "Read tools only.",
  fallback: true,
};
const ask: SessionAccessModeCapability = {
  id: "ask",
  label: "Ask each time",
  description: "Review permission requests.",
};
const workspaceWrite: SessionAccessModeCapability = {
  id: "workspaceWrite",
  label: "Workspace edits",
  description: "Approve workspace edits automatically.",
};

describe("admitted session access modes", () => {
  it("does not invent a fallback that native did not declare", () => {
    expect(
      sessionAccessModeAvailability("ask", [{ ...readOnly, fallback: false }])
        .effective,
    ).toBeNull();
  });
  it.each([readOnly, ask, workspaceWrite])(
    "admits an explicitly supported policy: $id",
    (capability) => {
      expect(
        sessionAccessModeAvailability(capability.id, [capability]),
      ).toEqual({ effective: capability.id, unavailableReason: null });
    },
  );
  it.each(["ask", "workspaceWrite"] as const)(
    "degrades unsupported %s only to an admitted read-only policy",
    (mode) => {
      expect(sessionAccessModeAvailability(mode, [readOnly])).toEqual({
        effective: "readOnly",
        unavailableReason: "This access mode is not supported by this harness.",
      });
    },
  );
  it("does not silently replace read-only with permission approval", () => {
    expect(
      sessionAccessModeAvailability("readOnly", [ask]).effective,
    ).toBeNull();
  });
  it("does not fabricate access capabilities before native admission", () => {
    expect(
      sessionAccessModeAvailability("readOnly", undefined).effective,
    ).toBeNull();
    expect(
      sessionAccessModeAvailability("workspaceWrite", []).effective,
    ).toBeNull();
  });
  it("fails closed for unknown stored modes", () => {
    expect(
      sessionAccessModeAvailability("elevated" as never, [readOnly]),
    ).toEqual({ effective: "readOnly", unavailableReason: null });
  });
});
