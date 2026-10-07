import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ResolvePermissionDecisionRequest } from "../domain/acp";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { acpDesktop } from "./acpDesktop";

const permissionRequest: ResolvePermissionDecisionRequest = {
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
  turnId: "turn-a",
  runtimeEpoch: "2",
  requestId: "request-a",
  decision: "deny",
};

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  invokeMock.mockReset();
});

describe("ACP permission decision bridge", () => {
  it("accepts a browser-source terminal denial but rejects a harness-source terminal denial", async () => {
    const terminalDenial = {
      ...permissionRequest,
      state: "resolvedDeclined",
    };
    invokeMock.mockResolvedValueOnce({ ...terminalDenial, source: "browser" });
    await expect(
      acpDesktop.resolvePermissionDecision(permissionRequest),
    ).resolves.toEqual({ ...terminalDenial, source: "browser" });

    invokeMock.mockResolvedValueOnce({ ...terminalDenial, source: "harness" });
    await expect(
      acpDesktop.resolvePermissionDecision(permissionRequest),
    ).rejects.toMatchObject({ failure: { kind: "internal" } });
  });

  it("sends and accepts only the exact six-field permission identity", async () => {
    invokeMock.mockResolvedValue({
      ...permissionRequest,
      state: "declinePending",
      rawFrame: "never-retain",
    });

    await expect(
      acpDesktop.resolvePermissionDecision(permissionRequest),
    ).resolves.toEqual({
      ...permissionRequest,
      state: "declinePending",
    });
    expect(invokeMock).toHaveBeenCalledWith("resolve_permission_decision", {
      request: permissionRequest,
    });
  });

  it.each([
    ["projectId", "project-forged"],
    ["chatId", "chat-forged"],
    ["sessionId", "session-forged"],
    ["turnId", "turn-forged"],
    ["runtimeEpoch", "0"],
    ["requestId", "request-forged"],
    ["decision", "ask"],
    ["state", "resolvedDeclined"],
  ] as const)(
    "rejects a forged permission receipt %s",
    async (field, value) => {
      invokeMock.mockResolvedValue({
        ...permissionRequest,
        state: "declinePending",
        [field]: value,
      });

      await expect(
        acpDesktop.resolvePermissionDecision(permissionRequest),
      ).rejects.toMatchObject({ failure: { kind: "internal" } });
    },
  );

  it.each([
    { ...permissionRequest, turnId: undefined, state: "declinePending" },
    { ...permissionRequest, runtimeEpoch: 2, state: "declinePending" },
    {
      ...permissionRequest,
      runtimeEpoch: "9223372036854775808",
      state: "declinePending",
    },
  ])("rejects a malformed permission receipt", async (response) => {
    invokeMock.mockResolvedValue(response);

    await expect(
      acpDesktop.resolvePermissionDecision(permissionRequest),
    ).rejects.toMatchObject({ failure: { kind: "internal" } });
  });
});
