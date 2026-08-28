import { describe, expect, it } from "vitest";

import { parseWorkspaceSearchResponse } from "./workspaceSearchDomain";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";

describe("workspace Search domain", () => {
  it("accepts bounded canonical native results", () => {
    const result = parseWorkspaceSearchResponse({
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      matches: [
        {
          resource: { rootId: "root-a", relativePath: "src/main.ts" },
          lineNumber: 7,
          columnNumber: 4,
          preview: "const needle = true;",
        },
      ],
      roots: [
        {
          rootId: "root-a",
          rootName: "Sage",
          state: "complete",
          filesScanned: 12,
          bytesScanned: 840,
          entriesVisited: 15,
          skippedFiles: 1,
          errorCount: 0,
          message: null,
        },
      ],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 8,
    });

    expect(result.matches[0].resource).toEqual({
      rootId: "root-a",
      relativePath: "src/main.ts",
    });
  });

  it("rejects alias paths and over-capacity result lists", () => {
    const match = {
      resource: { rootId: "root-a", relativePath: "src/../secret.txt" },
      lineNumber: 1,
      columnNumber: 1,
      preview: "needle",
    };
    const response = {
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      matches: [match],
      roots: [],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    };
    expect(() => parseWorkspaceSearchResponse(response)).toThrow(
      "invalid workspace Search response",
    );
    expect(() =>
      parseWorkspaceSearchResponse({
        ...response,
        matches: Array.from({ length: 501 }, () => ({
          ...match,
          resource: { rootId: "root-a", relativePath: "safe.txt" },
        })),
      }),
    ).toThrow("invalid workspace Search response");
  });

  it("rejects contradictory root, match, and truncation semantics", () => {
    const unavailable = {
      rootId: "root-a",
      rootName: "Missing",
      state: "unavailable",
      filesScanned: 0,
      bytesScanned: 0,
      entriesVisited: 0,
      skippedFiles: 0,
      errorCount: 1,
      message: "root is missing",
    };
    const match = {
      resource: { rootId: "root-a", relativePath: "safe.txt" },
      lineNumber: 1,
      columnNumber: 1,
      preview: "needle",
    };
    const base = {
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      matches: [],
      roots: [unavailable],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    };

    for (const response of [
      { ...base, matches: [match] },
      {
        ...base,
        roots: [
          {
            ...unavailable,
            state: "complete",
            message: "impossible complete root error",
          },
        ],
      },
      { ...base, truncated: false, limitReasons: ["timeLimit"] },
      { ...base, roots: [] },
      {
        ...base,
        roots: [
          {
            ...unavailable,
            rootId: "root-b",
            state: "complete",
            filesScanned: 200_000,
            bytesScanned: 64 * 1024 * 1024,
            entriesVisited: 200_000,
            errorCount: 0,
            message: null,
          },
          {
            ...unavailable,
            rootId: "root-c",
            state: "complete",
            filesScanned: 1,
            bytesScanned: 1,
            entriesVisited: 1,
            errorCount: 0,
            message: null,
          },
        ],
      },
    ]) {
      expect(() => parseWorkspaceSearchResponse(response)).toThrow(
        "invalid workspace Search response",
      );
    }
  });

  it("enforces native preview and result-limit semantics exactly", () => {
    const root = {
      rootId: "root-a",
      rootName: "Sage",
      state: "complete",
      filesScanned: 1,
      bytesScanned: 1,
      entriesVisited: 1,
      skippedFiles: 0,
      errorCount: 0,
      message: null,
    };
    const match = {
      resource: { rootId: "root-a", relativePath: "safe.txt" },
      lineNumber: 1,
      columnNumber: 1,
      preview: "😀".repeat(240),
    };
    const base = {
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query: "needle",
      matches: [match],
      roots: [root],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    };

    expect(parseWorkspaceSearchResponse(base).matches[0].preview).toBe(
      match.preview,
    );
    expect(() =>
      parseWorkspaceSearchResponse({
        ...base,
        matches: [{ ...match, preview: "a".repeat(241) }],
      }),
    ).toThrow("invalid workspace Search response");
    expect(() =>
      parseWorkspaceSearchResponse({
        ...base,
        matches: Array.from({ length: 500 }, () => match),
      }),
    ).toThrow("invalid workspace Search response");
    expect(() =>
      parseWorkspaceSearchResponse({
        ...base,
        matches: Array.from({ length: 499 }, () => match),
        roots: [{ ...root, state: "partial", skippedFiles: 1 }],
        truncated: true,
        limitReasons: ["resultLimit"],
      }),
    ).toThrow("invalid workspace Search response");
  });

  it("admits a 200k file walk and keeps the WebView match list capped", () => {
    expect(WORKSPACE_SEARCH_LIMITS.files).toBe(200_000);
    expect(WORKSPACE_SEARCH_LIMITS.entries).toBe(200_000);
    expect(WORKSPACE_SEARCH_LIMITS.results).toBe(500);
    const root = {
      rootId: "root-a",
      rootName: "Sage",
      state: "partial" as const,
      filesScanned: 200_000,
      bytesScanned: 64 * 1024 * 1024,
      entriesVisited: 200_000,
      skippedFiles: 0,
      errorCount: 0,
      message: null,
    };
    const match = {
      resource: { rootId: "root-a", relativePath: "safe.txt" },
      lineNumber: 1,
      columnNumber: 1,
      preview: "needle",
    };
    expect(
      parseWorkspaceSearchResponse({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        query: "needle",
        matches: [match],
        roots: [root],
        truncated: true,
        continuationToken: null,
        limitReasons: ["fileLimit", "byteLimit", "entryLimit"],
        elapsedMs: 8,
      }).roots[0]?.filesScanned,
    ).toBe(200_000);
    expect(() =>
      parseWorkspaceSearchResponse({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        query: "needle",
        matches: [match],
        roots: [{ ...root, filesScanned: 200_001 }],
        truncated: true,
        continuationToken: null,
        limitReasons: ["fileLimit"],
        elapsedMs: 8,
      }),
    ).toThrow("invalid workspace Search response");
    expect(() =>
      parseWorkspaceSearchResponse({
        requestId: "request-a",
        projectId: "project-a",
        transitionEpoch: 4,
        indexGeneration: 0,
        query: "needle",
        matches: [match],
        roots: [{ ...root, filesScanned: 1, entriesVisited: 200_001 }],
        truncated: true,
        continuationToken: null,
        limitReasons: ["entryLimit"],
        elapsedMs: 8,
      }),
    ).toThrow("invalid workspace Search response");
  });
});
