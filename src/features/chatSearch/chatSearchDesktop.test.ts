import { describe, expect, it, vi } from "vitest";

import { createProjectChatSearchAdapter } from "./chatSearchDesktop";

describe("Project Chat search desktop adapter", () => {
  it("normalizes the query and rejects a response from another intent", async () => {
    const invoke = vi.fn().mockResolvedValue({
      projectId: "project-a",
      projectGeneration: 5,
      transitionEpoch: 2,
      query: "needle",
      results: [],
      hasMore: false,
      elapsedMs: 1,
    });
    const adapter = createProjectChatSearchAdapter(invoke);
    await expect(
      adapter.search({
        projectId: "project-a",
        projectGeneration: 4,
        expectedTransitionEpoch: 2,
        query: " needle ",
      }),
    ).rejects.toThrow("intent identity");
    expect(invoke).toHaveBeenCalledWith("search_project_chats", {
      request: {
        projectId: "project-a",
        projectGeneration: 4,
        expectedTransitionEpoch: 2,
        query: "needle",
      },
    });
  });

  it("reports the browser seam as explicitly unavailable", async () => {
    const adapter = createProjectChatSearchAdapter(async () => {
      throw new Error("Chat search is available in the Sage desktop runtime.");
    });
    await expect(
      adapter.search({
        projectId: "project-a",
        projectGeneration: 0,
        expectedTransitionEpoch: 0,
        query: "needle",
      }),
    ).rejects.toThrow("desktop runtime");
  });
});
