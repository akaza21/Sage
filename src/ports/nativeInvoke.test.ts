import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { invokeNative } from "./nativeInvoke";

describe("invokeNative", () => {
  afterEach(() => invoke.mockReset());
  it("delegates its exact command and args", async () => {
    invoke.mockResolvedValue({ ok: true });
    await expect(
      invokeNative("command", { request: { id: "x" } }),
    ).resolves.toEqual({ ok: true });
    expect(invoke).toHaveBeenCalledWith("command", { request: { id: "x" } });
  });
  it("preserves rejection identity", async () => {
    const failure = new Error("native failure");
    invoke.mockRejectedValue(failure);
    await expect(invokeNative("command")).rejects.toBe(failure);
  });
});
