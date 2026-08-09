import { describe, expect, it, vi } from "vitest";

import { createWorkspaceSearchAdapter } from "./workspaceSearchDesktop";

const response = {
  requestId: "request-a",
  projectId: "project-a",
  transitionEpoch: 4,
  query: "needle",
  matches: [],
  roots: [
    {
      rootId: "root-a",
      rootName: "Sage",
      state: "complete",
      filesScanned: 1,
      bytesScanned: 12,
      entriesVisited: 1,
      skippedFiles: 0,
      errorCount: 0,
      message: null,
    },
  ],
  truncated: false,
  limitReasons: [],
  elapsedMs: 1,
} as const;

describe("workspace Search desktop adapter", () => {
  it("validates and trims requests at the native command boundary", async () => {
    const invoke = vi.fn().mockImplementation((_command, args) =>
      Promise.resolve({
        ...response,
        requestId: (args.request as { requestId: string }).requestId,
      }),
    );
    const adapter = createWorkspaceSearchAdapter(invoke);

    await adapter.search({
      projectId: "project-a",
      expectedTransitionEpoch: 4,
      query: "  needle  ",
      caseSensitive: false,
    });

    expect(invoke).toHaveBeenCalledWith("search_workspace", {
      request: {
        projectId: "project-a",
        expectedTransitionEpoch: 4,
        query: "needle",
        caseSensitive: false,
        requestId: expect.any(String),
      },
    });
  });

  it("drops a native response when the caller cancelled in flight", async () => {
    let resolve!: (value: unknown) => void;
    let requestId = "";
    const invoke = vi.fn((command: string, args: Record<string, unknown>) => {
      if (command === "cancel_workspace_search") return Promise.resolve(true);
      requestId = (args.request as { requestId: string }).requestId;
      return new Promise<unknown>((done) => (resolve = done));
    });
    const adapter = createWorkspaceSearchAdapter(invoke);
    const controller = new AbortController();
    const pending = adapter.search(
      {
        projectId: "project-a",
        expectedTransitionEpoch: 4,
        query: "needle",
        caseSensitive: false,
      },
      controller.signal,
    );

    controller.abort();
    resolve({ ...response, requestId });

    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(invoke).toHaveBeenCalledWith("cancel_workspace_search", {
      requestId,
    });
  });

  it("rejects malformed native responses instead of projecting them", async () => {
    const adapter = createWorkspaceSearchAdapter(
      vi.fn((_command, args) =>
        Promise.resolve({
          ...response,
          requestId: (args.request as { requestId: string }).requestId,
          matches: "unbounded",
        }),
      ),
    );

    await expect(
      adapter.search({
        projectId: "project-a",
        expectedTransitionEpoch: 4,
        query: "needle",
        caseSensitive: false,
      }),
    ).rejects.toThrow("invalid workspace Search response");
  });
});
