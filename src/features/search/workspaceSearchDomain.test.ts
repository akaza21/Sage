import { describe, expect, it } from "vitest";

import { parseWorkspaceSearchResponse } from "./workspaceSearchDomain";

describe("workspace Search domain", () => {
  it("accepts bounded canonical native results", () => {
    const result = parseWorkspaceSearchResponse({
      projectId: "project-a",
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
          skippedFiles: 0,
          errorCount: 0,
          message: null,
        },
      ],
      truncated: false,
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
      projectId: "project-a",
      query: "needle",
      matches: [match],
      roots: [],
      truncated: false,
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
});
