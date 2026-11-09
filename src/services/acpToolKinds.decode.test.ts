import { describe, expect, it } from "vitest";

import { TOOL_PROGRESS_BYTES } from "../domain/acpProjection";
import { safeToolPresentation } from "./acpDesktopResponsePayloads";
import { safeItem } from "./acpDesktopResponseItems";

function wireItem(
  kind: string,
  payload: Record<string, unknown>,
  toolState: string,
) {
  return {
    id: "item-1",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    kind,
    toolState,
    payload,
  };
}

describe("tool kind decoding", () => {
  it("admits the web search and image view kinds the bindings project", () => {
    expect(
      safeToolPresentation({ kind: "webSearch", label: "Web search" }),
    ).toEqual({
      kind: "webSearch",
      label: "Web search",
    });
    expect(
      safeToolPresentation({
        kind: "imageView",
        label: "Viewed an image",
        resources: [
          {
            kind: "resource",
            resource: { rootId: "root-1", relativePath: "chart.png" },
          },
        ],
      }),
    ).toMatchObject({ kind: "imageView", resources: [{ kind: "resource" }] });
  });

  it("carries a running MCP call's status line and rejects an oversized one", () => {
    const mcp = {
      kind: "mcp",
      label: "search",
      serverLabel: "docs",
      toolLabel: "search",
      argumentsSummary: null,
      resultSummary: null,
    };
    expect(
      safeToolPresentation({ ...mcp, progress: "Indexed 3 of 9 pages" }),
    ).toEqual({ ...mcp, progress: "Indexed 3 of 9 pages" });
    expect(safeToolPresentation(mcp)).toEqual(mcp);
    expect(
      safeToolPresentation({
        ...mcp,
        progress: "x".repeat(TOOL_PROGRESS_BYTES + 1),
      }),
    ).toBe(null);
    expect(safeToolPresentation({ ...mcp, progress: 3 })).toBe(null);
    expect(safeToolPresentation({ ...mcp, progress: null })).toBe(null);
    expect(safeToolPresentation({ ...mcp, progress: "" })).toBe(null);
  });

  it("carries a permission's category and harness reason, and admits their absence", () => {
    const base = {
      requestId: "request-1",
      state: "received",
      label: "Permission request",
      summary: "curl https://example.com",
      source: "harness",
      offeredOptionKinds: ["allow_once", "allow_always", "reject_once"],
    };
    const withReason = safeItem(
      wireItem(
        "permissionRequest",
        { ...base, category: "execute", reason: "May I retry?" },
        "waitingForApproval",
      ),
    );
    expect(
      withReason?.kind === "permissionRequest" && withReason.payload,
    ).toMatchObject({
      category: "execute",
      reason: "May I retry?",
    });
    const absent = safeItem(
      wireItem("permissionRequest", base, "waitingForApproval"),
    );
    expect(
      absent?.kind === "permissionRequest" && absent.payload,
    ).toMatchObject({
      category: undefined,
      reason: null,
    });
    const oversized = safeItem(
      wireItem(
        "permissionRequest",
        { ...base, reason: "x".repeat(2049) },
        "waitingForApproval",
      ),
    );
    expect(
      oversized?.kind === "permissionRequest" && oversized.payload.reason,
    ).toBe(null);
  });
});
