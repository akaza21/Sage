import { describe, expect, it, vi } from "vitest";

import { createWorkspaceSearchAdapter } from "./workspaceSearchDesktop";

const response = {
  projectId: "project-a",
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
    const invoke = vi.fn().mockResolvedValue(response);
    const adapter = createWorkspaceSearchAdapter(invoke);

    await adapter.search({
      projectId: "project-a",
      query: "  needle  ",
      caseSensitive: false,
    });

    expect(invoke).toHaveBeenCalledWith("search_workspace", {
      request: {
        projectId: "project-a",
        query: "needle",
        caseSensitive: false,
      },
    });
  });

  it("drops a native response when the caller cancelled in flight", async () => {
    let resolve!: (value: unknown) => void;
    const invoke = vi.fn(
      () => new Promise<unknown>((done) => (resolve = done)),
    );
    const adapter = createWorkspaceSearchAdapter(invoke);
    const controller = new AbortController();
    const pending = adapter.search(
      {
        projectId: "project-a",
        query: "needle",
        caseSensitive: false,
      },
      controller.signal,
    );

    controller.abort();
    resolve(response);

    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });

  it("rejects malformed native responses instead of projecting them", async () => {
    const adapter = createWorkspaceSearchAdapter(
      vi.fn().mockResolvedValue({ ...response, matches: "unbounded" }),
    );

    await expect(
      adapter.search({
        projectId: "project-a",
        query: "needle",
        caseSensitive: false,
      }),
    ).rejects.toThrow("invalid workspace Search response");
  });
});
