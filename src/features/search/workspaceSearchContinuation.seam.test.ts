import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  coordinateWorkspaceSearchIndex,
  parseWorkspaceSearchResponse,
  validateWorkspaceSearchRequest,
  type WorkspaceSearchResponse,
} from "./workspaceSearchDomain";
import { createWorkspaceSearchAdapter } from "./workspaceSearchDesktop";
import { useWorkspaceSearch } from "./useWorkspaceSearch";

function page(
  overrides: Partial<WorkspaceSearchResponse> = {},
): WorkspaceSearchResponse {
  return {
    requestId: "request-a",
    projectId: "project-a",
    transitionEpoch: 4,
    indexGeneration: 0,
    query: "needle",
    matches: [
      {
        resource: { rootId: "root-a", relativePath: "safe.txt" },
        lineNumber: 1,
        columnNumber: 1,
        preview: "needle",
      },
    ],
    roots: [
      {
        rootId: "root-a",
        rootName: "Sage",
        state: "complete",
        filesScanned: 1,
        bytesScanned: 6,
        entriesVisited: 1,
        skippedFiles: 0,
        errorCount: 0,
        message: null,
      },
    ],
    truncated: false,
    continuationToken: null,
    limitReasons: [],
    elapsedMs: 1,
    ...overrides,
  };
}

function continuationToken(
  indexGeneration = 0,
  epoch = 4,
  caseFlag = 0,
  wholeWordFlag = 0,
) {
  return `{"v":1,"g":${indexGeneration},"e":${epoch},"c":${caseFlag},"w":${wholeWordFlag},"f":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","q":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","r":"root-a","p":"safe.txt","l":1,"o":1}`;
}

const token = continuationToken();

function truncatedPage(
  overrides: Partial<WorkspaceSearchResponse> = {},
): WorkspaceSearchResponse {
  const indexGeneration = overrides.indexGeneration ?? 0;
  return page({
    truncated: true,
    continuationToken: continuationToken(indexGeneration),
    limitReasons: ["fileSizeLimit"],
    roots: [
      {
        ...page().roots[0],
        state: "partial",
        skippedFiles: 1,
      },
    ],
    ...overrides,
  });
}

describe("workspace Search continuation public seam", () => {
  it("complete first page requires a null continuation token and an index generation", () => {
    const {
      continuationToken: _token,
      indexGeneration: _generation,
      ...legacy
    } = page();
    expect(() => parseWorkspaceSearchResponse(legacy)).toThrow(
      "invalid workspace Search response",
    );
    expect(parseWorkspaceSearchResponse(page())).toMatchObject({
      continuationToken: null,
      indexGeneration: 0,
    });
  });

  it("truncated page may carry a bounded token; a complete page may not", () => {
    expect(
      parseWorkspaceSearchResponse(truncatedPage()).continuationToken,
    ).toBe(token);
    expect(() =>
      parseWorkspaceSearchResponse(page({ continuationToken: token })),
    ).toThrow("invalid workspace Search response");
  });

  it("coordinateWorkspaceSearchIndex is an exact triple", () => {
    const stamp = {
      projectId: "project-a",
      transitionEpoch: 4,
      projectGeneration: 9,
    };
    expect(coordinateWorkspaceSearchIndex(stamp)).toEqual(stamp);
    expect(coordinateWorkspaceSearchIndex({ ...stamp })).toEqual(stamp);
    expect(
      coordinateWorkspaceSearchIndex({ ...stamp, projectId: "project-b" }),
    ).not.toEqual(stamp);
    expect(
      coordinateWorkspaceSearchIndex({ ...stamp, transitionEpoch: 5 }),
    ).not.toEqual(stamp);
    expect(
      coordinateWorkspaceSearchIndex({ ...stamp, projectGeneration: 10 }),
    ).not.toEqual(stamp);
  });

  it("continueSearch sends the previous token with the live index generation", async () => {
    const search = vi
      .fn()
      .mockResolvedValueOnce(truncatedPage({ indexGeneration: 7 }))
      .mockResolvedValueOnce(
        page({ requestId: "request-b", indexGeneration: 7 }),
      );
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 7,
        gateway: { search },
      }),
    );

    act(() => result.current.setQuery("needle"));
    await act(async () => void (await result.current.submit()));
    await act(async () => void (await result.current.continueSearch()));

    expect(search).toHaveBeenLastCalledWith(
      expect.objectContaining({
        continuationToken: continuationToken(7),
        indexGeneration: 7,
      }),
      expect.any(AbortSignal),
    );
  });

  it("stale index generation drops the token and refuses continue", async () => {
    const search = vi
      .fn()
      .mockResolvedValue(truncatedPage({ indexGeneration: 7 }));
    const { result, rerender } = renderHook(
      ({ indexGeneration }) =>
        useWorkspaceSearch({
          projectId: "project-a",
          transitionEpoch: 4,
          indexGeneration,
          gateway: { search },
        }),
      { initialProps: { indexGeneration: 7 } },
    );
    act(() => result.current.setQuery("needle"));
    await act(async () => void (await result.current.submit()));
    rerender({ indexGeneration: 8 });
    await act(async () => void (await result.current.continueSearch()));
    expect(search).toHaveBeenCalledTimes(1);
  });

  it("adapter forwards indexGeneration and continuationToken on search_workspace", async () => {
    const invoke = vi.fn((_command, args) =>
      Promise.resolve({
        ...page(),
        requestId: (args.request as { requestId: string }).requestId,
        indexGeneration: 7,
      }),
    );
    await createWorkspaceSearchAdapter(invoke).search({
      projectId: "project-a",
      expectedTransitionEpoch: 4,
      indexGeneration: 7,
      continuationToken: continuationToken(7),
      query: "needle",
      caseSensitive: false,
      wholeWord: false,
      includePatterns: [],
      excludePatterns: [],
    });
    expect(invoke).toHaveBeenCalledWith("search_workspace", {
      request: expect.objectContaining({
        indexGeneration: 7,
        continuationToken: continuationToken(7),
      }),
    });
  });

  it("adapter rejects a response whose indexGeneration disagrees", async () => {
    const adapter = createWorkspaceSearchAdapter(
      vi.fn((_command, args) =>
        Promise.resolve({
          ...page(),
          requestId: (args.request as { requestId: string }).requestId,
          indexGeneration: 8,
        }),
      ),
    );
    await expect(
      adapter.search({
        projectId: "project-a",
        expectedTransitionEpoch: 4,
        indexGeneration: 7,
        continuationToken: null,
        query: "needle",
        caseSensitive: false,
        wholeWord: false,
        includePatterns: [],
        excludePatterns: [],
      }),
    ).rejects.toThrow("invalid workspace Search response");
  });

  it("malformed, oversize, or unknown-key token is invalid", () => {
    for (const continuationToken of [
      "not json",
      "x".repeat(1025),
      token.replace("}", ',"x":1}'),
    ]) {
      expect(() =>
        parseWorkspaceSearchResponse(truncatedPage({ continuationToken })),
      ).toThrow("invalid workspace Search response");
    }
  });

  it("token epoch or case mismatch is stale", () => {
    const request = {
      projectId: "project-a",
      expectedTransitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      caseSensitive: false,
      wholeWord: false,
      includePatterns: [],
      excludePatterns: [],
    };
    expect(() =>
      validateWorkspaceSearchRequest({
        ...request,
        continuationToken: continuationToken(0, 5),
      }),
    ).toThrow("workspace Search continuation is stale");
    expect(() =>
      validateWorkspaceSearchRequest({
        ...request,
        continuationToken: continuationToken(0, 4, 1),
      }),
    ).toThrow("workspace Search continuation is stale");
    expect(() =>
      validateWorkspaceSearchRequest({
        ...request,
        continuationToken: continuationToken(0, 4, 0, 1),
      }),
    ).toThrow("workspace Search continuation is stale");
  });

  it("rejects unsafe include and exclude patterns before native invocation", () => {
    const request = {
      projectId: "project-a",
      expectedTransitionEpoch: 4,
      indexGeneration: 0,
      continuationToken: null,
      query: "needle",
      caseSensitive: false,
      wholeWord: false,
      includePatterns: ["../outside"],
      excludePatterns: [],
    };
    expect(() => validateWorkspaceSearchRequest(request)).toThrow(
      "workspace Search path filters are invalid",
    );
    expect(() =>
      validateWorkspaceSearchRequest({
        ...request,
        includePatterns: [],
        excludePatterns: ["/absolute"],
      }),
    ).toThrow("workspace Search path filters are invalid");
  });

  it("page 4 is retained and a fifth continue is refused", async () => {
    const truncated = (requestId: string) => truncatedPage({ requestId });
    const search = vi
      .fn()
      .mockResolvedValueOnce(truncated("request-1"))
      .mockResolvedValueOnce(truncated("request-2"))
      .mockResolvedValueOnce(truncated("request-3"))
      .mockResolvedValueOnce(truncated("request-4"));
    const { result } = renderHook(() =>
      useWorkspaceSearch({
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        gateway: { search },
      }),
    );
    act(() => result.current.setQuery("needle"));
    await act(async () => void (await result.current.submit()));
    for (let count = 0; count < 4; count += 1) {
      await act(async () => void (await result.current.continueSearch()));
    }
    expect(result.current.state.pages).toHaveLength(4);
    expect(search).toHaveBeenCalledTimes(4);
  });
});
