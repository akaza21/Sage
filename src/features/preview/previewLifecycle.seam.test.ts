import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  discoverPreviewTargets,
  getPreviewHealth,
  startPreviewTarget,
  stopPreviewTarget,
  type PreviewDiscoveryRequest,
  type PreviewTargetRef,
} from "./previewDesktop";

const request = {
  projectId: "project-preview",
  projectGeneration: "7",
  expectedTransitionEpoch: 7,
  chatId: "chat-preview",
  sessionId: "session-preview",
  runtimeEpoch: "3",
} as PreviewDiscoveryRequest;
const target: PreviewTargetRef = {
  ...request,
  rootId: "root-preview",
  scriptName: "dev",
} as never;

describe("Preview lifecycle public seam", () => {
  beforeEach(() => {
    invoke.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });

  it("sends the exact native owner fields and accepts serde counter shapes", async () => {
    invoke.mockResolvedValue({
      projectId: request.projectId,
      transitionEpoch: 7,
      targets: [target],
      truncated: false,
    });
    await expect(discoverPreviewTargets(request)).resolves.toEqual(
      expect.objectContaining({ targets: [target] }),
    );
    expect(invoke).toHaveBeenCalledWith("discover_preview_targets", {
      request,
    });
  });

  it("rejects numeric branded counters, string epochs, and wrong target keys", async () => {
    await expect(
      discoverPreviewTargets({ ...request, projectGeneration: 7 } as never),
    ).rejects.toThrow(/identity/i);
    await expect(
      discoverPreviewTargets({
        ...request,
        expectedTransitionEpoch: "7",
      } as never),
    ).rejects.toThrow(/identity/i);
    invoke.mockResolvedValue({
      projectId: request.projectId,
      transitionEpoch: 7,
      targets: [{ ...target, transitionEpoch: 7 }],
      truncated: false,
    });
    await expect(discoverPreviewTargets(request)).rejects.toThrow(/target/i);
    await expect(
      discoverPreviewTargets({ ...request, transitionEpoch: 7 } as never),
    ).rejects.toThrow(/identity/i);
  });

  it("rejects lifecycle responses for a different full native owner", async () => {
    const replacement = { ...target, runtimeEpoch: "4" } as PreviewTargetRef;
    invoke.mockResolvedValueOnce({
      ref: replacement,
      origin: "http://127.0.0.1:5173",
      healthy: true,
    });
    await expect(getPreviewHealth(target)).rejects.toThrow(/invalid response/i);

    invoke.mockResolvedValueOnce({
      ref: replacement,
      origin: "http://127.0.0.1:5173",
    });
    await expect(startPreviewTarget(target)).rejects.toThrow(
      /invalid response/i,
    );

    invoke.mockResolvedValueOnce({ ref: replacement });
    await expect(stopPreviewTarget(target)).rejects.toThrow(
      /invalid response/i,
    );
  });

  it("accepts only loopback lifecycle origins and exact response references", async () => {
    invoke.mockResolvedValueOnce({
      ref: target,
      origin: "http://127.0.0.1:5173",
      healthy: true,
    });
    await expect(getPreviewHealth(target)).resolves.toMatchObject({
      ref: target,
      healthy: true,
    });
    invoke.mockResolvedValueOnce({
      ref: target,
      origin: "https://example.com:443",
    });
    await expect(startPreviewTarget(target)).rejects.toThrow(/invalid origin/i);
    invoke.mockResolvedValueOnce({ ref: target });
    await expect(stopPreviewTarget(target)).resolves.toEqual({ ref: target });
  });
});
