import { readFile } from "node:fs/promises";
import path from "node:path";

import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke =
  vi.fn<(command: string, args: Record<string, unknown>) => Promise<unknown>>();

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  discoverPreviewTargets,
  getPreviewHealth,
  startPreviewTarget,
  stopPreviewTarget,
  type PreviewTargetRef,
} from "./previewDesktop";

const request = {
  projectId: "project-preview",
  projectGeneration: 3,
  expectedTransitionEpoch: 7,
};

const target: PreviewTargetRef = {
  ...request,
  rootId: "root-preview",
  scriptName: "dev",
};

describe("Preview lifecycle public seam", () => {
  let active: PreviewTargetRef | null;

  beforeEach(() => {
    active = null;
    invoke.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    invoke.mockImplementation(async (command, args) => {
      const ref = (args.request ?? args.ref) as PreviewTargetRef;
      switch (command) {
        case "discover_preview_targets":
          return {
            ...request,
            transitionEpoch: 7,
            targets: [target],
            truncated: false,
          };
        case "get_preview_health":
          return { ref, origin: "http://127.0.0.1:5173", healthy: true };
        case "start_preview_target":
          if (ref.expectedTransitionEpoch !== 7)
            throw {
              kind: "conflict",
              resource: "transitionEpoch",
              message: "Preview target is stale.",
            };
          if (active)
            throw {
              kind: "conflict",
              resource: "activePreview",
              message: "A Preview target is already active.",
            };
          active = ref;
          return { ref, origin: "http://127.0.0.1:5173" };
        case "stop_preview_target":
          active = null;
          return { ref };
        default:
          throw new Error(`unexpected preview command: ${command}`);
      }
    });
  });

  it("discovers only curated scripts without exposing paths or argv", async () => {
    const response = await discoverPreviewTargets(request);

    expect(response.targets).toEqual([target]);
    expect(
      response.targets.every(
        ({ scriptName }) => scriptName === "dev" || scriptName === "start",
      ),
    ).toBe(true);
    expect(invoke).toHaveBeenCalledWith("discover_preview_targets", {
      request,
    });
    expect(JSON.stringify(response)).not.toMatch(
      /[A-Za-z]:\\|"(?:argv|path)"/i,
    );
  });

  it("rejects remote origins before requesting health or a start", async () => {
    const remote = {
      ...target,
      origin: "http://example.com",
    } as PreviewTargetRef;

    await expect(getPreviewHealth(remote)).rejects.toThrow(/loopback/i);
    await expect(startPreviewTarget(remote)).rejects.toThrow(/loopback/i);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects lifecycle responses for a different Preview target", async () => {
    const differentTarget: PreviewTargetRef = {
      ...target,
      rootId: "root-other",
    };

    invoke.mockResolvedValueOnce({
      ref: differentTarget,
      origin: "http://127.0.0.1:5173",
      healthy: true,
    });
    await expect(getPreviewHealth(target)).rejects.toThrow(/invalid response/i);

    invoke.mockResolvedValueOnce({
      ref: differentTarget,
      origin: "http://127.0.0.1:5173",
    });
    await expect(startPreviewTarget(target)).rejects.toThrow(
      /invalid response/i,
    );

    invoke.mockResolvedValueOnce({ ref: differentTarget });
    await expect(stopPreviewTarget(target)).rejects.toThrow(
      /invalid response/i,
    );
  });

  it("rejects stale starts, permits one active child, and releases it on stop", async () => {
    await expect(
      startPreviewTarget({ ...target, expectedTransitionEpoch: 6 }),
    ).rejects.toMatchObject({ failure: { kind: "conflict" } });
    await expect(startPreviewTarget(target)).resolves.toEqual({
      ref: target,
      origin: "http://127.0.0.1:5173",
    });
    await expect(startPreviewTarget(target)).rejects.toMatchObject({
      failure: { kind: "conflict" },
    });
    await expect(stopPreviewTarget(target)).resolves.toEqual({ ref: target });
    await expect(startPreviewTarget(target)).resolves.toEqual({
      ref: target,
      origin: "http://127.0.0.1:5173",
    });
  });

  it("keeps Preview isolated from generic desktop and process authority", async () => {
    const source = await readFile(
      path.join(import.meta.dirname, "previewDesktop.ts"),
      "utf8",
    );

    expect(source).not.toMatch(/from\s+["'][^"']*desktop["']/);
    expect(source).not.toMatch(/\b(?:spawn|shell|cmd\.exe)\b/i);
  });
});
