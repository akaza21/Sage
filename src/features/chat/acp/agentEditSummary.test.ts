import { describe, expect, it } from "vitest";

import type {
  AcpChatPage,
  AcpItem,
  AgentSessionProjection,
  TurnProjection,
} from "../../../domain/acp";
import {
  MAX_AGENT_EDIT_SUMMARY_FILES,
  projectAgentEditSummary,
} from "./agentEditSummary";

type FileChangeItem = Extract<AcpItem, { kind: "fileChangeDiff" }>;

function fileItem(
  id: string,
  ordinal: number,
  rootId: string,
  relativePath: string,
  summary: string,
  overrides: Partial<AcpItem> = {},
): FileChangeItem {
  return {
    id,
    projectId: "project-a",
    chatId: "chat-a",
    sessionId: "session-a",
    turnId: "turn-a",
    runtimeEpoch: "3",
    ordinal,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: ordinal,
    updatedAt: ordinal,
    terminalAt: ordinal,
    kind: "fileChangeDiff",
    toolState: null,
    payload: {
      changes: [
        {
          resource: {
            kind: "resource",
            resource: { rootId, relativePath },
          },
          operation: "updated",
          originalResource: null,
          summary,
          additions: null,
          deletions: null,
        },
      ],
      truncated: false,
    },
    ...overrides,
  } as FileChangeItem;
}

function turn(items: readonly AcpItem[]): TurnProjection {
  return {
    id: "turn-a",
    sessionId: "session-a",
    runtimeEpoch: "3",
    ordinal: 2,
    state: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    items,
    activities: [],
  } as unknown as TurnProjection;
}

function page(items: readonly AcpItem[]): AcpChatPage {
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "Owned chat",
    projectionRevision: "8",
    currentSessionId: "session-a",
    sessions: [
      {
        id: "session-a",
        ordinal: 1,
      } as AgentSessionProjection,
    ],
    turns: [turn(items)],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

describe("agent edit summary projection", () => {
  it("keeps edits inside their exact Project, Chat, session attempt, Turn, and runtime epoch", () => {
    const valid = fileItem("valid", 1, "root-a", "src/valid.ts", "valid");
    const projection = page([
      valid,
      fileItem("wrong-project", 2, "root-a", "src/project.ts", "wrong", {
        projectId: "project-b" as never,
      }),
      fileItem("wrong-chat", 3, "root-a", "src/chat.ts", "wrong", {
        chatId: "chat-b" as never,
      }),
      fileItem("wrong-attempt", 4, "root-a", "src/session.ts", "wrong", {
        sessionId: "session-b" as never,
      }),
      fileItem("wrong-turn", 5, "root-a", "src/turn.ts", "wrong", {
        turnId: "turn-b" as never,
      }),
      fileItem("wrong-runtime", 6, "root-a", "src/runtime.ts", "wrong", {
        runtimeEpoch: "4" as never,
      }),
    ]);

    expect(projectAgentEditSummary(projection, projection.turns[0])).toEqual({
      changes: [valid.payload.changes[0]],
      omittedCount: 0,
      sourceTruncated: false,
      rejectedUnsafeOrStale: true,
    });

    const switched = {
      ...projection,
      projectId: "project-b",
      chatId: "chat-b",
    } as AcpChatPage;
    expect(projectAgentEditSummary(switched, switched.turns[0])).toEqual({
      changes: [],
      omittedCount: 0,
      sourceTruncated: false,
      rejectedUnsafeOrStale: true,
    });
  });

  it("dedupes out-of-order revisions and file events deterministically", () => {
    const newest = fileItem(
      "diff-newest",
      9,
      "root-a",
      "src/shared.ts",
      "latest file event",
    );
    const staleSameItem = fileItem(
      "same-item",
      8,
      "root-b",
      "src/other.ts",
      "stale item revision",
      { revision: "2" as never },
    );
    const currentSameItem = fileItem(
      "same-item",
      8,
      "root-b",
      "src/other.ts",
      "current item revision",
      { revision: "10" as never },
    );
    const tool = {
      ...fileItem("tool", 10, "root-a", "src/ignored.ts", "ignored"),
      kind: "toolCall",
      toolState: "completed",
      payload: {
        presentation: {
          kind: "edit",
          label: "Apply patch",
          resources: [],
        },
        inputSummary: null,
      },
    } as AcpItem;
    const projection = page([
      newest,
      tool,
      staleSameItem,
      fileItem("diff-oldest", 1, "root-a", "src/shared.ts", "old event"),
      currentSameItem,
    ]);

    const result = projectAgentEditSummary(projection, projection.turns[0]);
    expect(result.changes.map((change) => change.summary)).toEqual([
      "current item revision",
      "latest file event",
    ]);
    expect(result.rejectedUnsafeOrStale).toBe(false);
  });

  it("uses code-unit item ordering and treats a rename destination as the changed-file identity", () => {
    const first = fileItem(
      "item-z",
      4,
      "root-a",
      "src/renamed.ts",
      "code-unit later",
    );
    const second = fileItem(
      "item-A",
      4,
      "root-a",
      "src/renamed.ts",
      "code-unit earlier",
    );
    for (const [item, originalPath] of [
      [first, "src/before-z.ts"],
      [second, "src/before-a.ts"],
    ] as const) {
      if (item.kind === "fileChangeDiff") {
        item.payload = {
          ...item.payload,
          changes: [
            {
              ...item.payload.changes[0],
              operation: "renamed",
              originalResource: {
                kind: "resource",
                resource: { rootId: "root-a", relativePath: originalPath },
              },
            },
          ],
        };
      }
    }
    const projection = page([first, second]);

    const result = projectAgentEditSummary(projection, projection.turns[0]);
    expect(result.changes).toHaveLength(1);
    expect(result.changes[0].summary).toBe("code-unit later");
    expect(result.changes[0].originalResource).toEqual({
      kind: "resource",
      resource: { rootId: "root-a", relativePath: "src/before-z.ts" },
    });
  });

  it("retains duplicate paths in different roots without aliasing", () => {
    const projection = page([
      fileItem("left", 1, "root-left", "src/main.ts", "left"),
      fileItem("right", 2, "root-right", "src/main.ts", "right"),
    ]);

    expect(
      projectAgentEditSummary(projection, projection.turns[0]).changes.map(
        (change) =>
          change.resource.kind === "resource"
            ? change.resource.resource.rootId
            : "unavailable",
      ),
    ).toEqual(["root-left", "root-right"]);
  });

  it("caps rendered files and rejects oversized path and summary text", () => {
    const bounded = Array.from(
      { length: MAX_AGENT_EDIT_SUMMARY_FILES + 2 },
      (_, index) =>
        fileItem(
          `item-${index}`,
          index,
          "root-a",
          `src/file-${index}.ts`,
          `change ${index}`,
        ),
    );
    const projection = page([
      ...bounded,
      fileItem("long-path", 100, "root-a", "x".repeat(16_385), "unsafe"),
      fileItem(
        "long-summary",
        101,
        "root-a",
        "src/unsafe.ts",
        "x".repeat(65_537),
      ),
    ]);
    const first = projection.turns[0].items[0];
    if (first.kind === "fileChangeDiff") {
      first.payload = { ...first.payload, truncated: true };
    }

    const result = projectAgentEditSummary(projection, projection.turns[0]);
    expect(result.changes).toHaveLength(MAX_AGENT_EDIT_SUMMARY_FILES);
    expect(result.changes[0].summary).toBe("change 2");
    expect(result.omittedCount).toBe(2);
    expect(result.sourceTruncated).toBe(true);
    expect(result.rejectedUnsafeOrStale).toBe(true);
  });

  it("rejects oversized identities and unsafe ordering numbers before sorting", () => {
    const projection = page([
      fileItem("valid", 1, "root-a", "src/valid.ts", "valid"),
      fileItem("x".repeat(513), 2, "root-a", "src/id.ts", "unsafe id"),
      fileItem("bad-ordinal", Number.NaN, "root-a", "src/ordinal.ts", "unsafe"),
      fileItem("bad-time", 3, "root-a", "src/time.ts", "unsafe", {
        updatedAt: Number.POSITIVE_INFINITY,
      }),
      fileItem("bad-revision", 4, "root-a", "src/revision.ts", "unsafe", {
        revision: "9".repeat(10_000) as never,
      }),
      fileItem("bad-root", 5, "x".repeat(513), "src/root.ts", "unsafe"),
    ]);

    const result = projectAgentEditSummary(projection, projection.turns[0]);
    expect(result.changes).toEqual([
      projection.turns[0].items[0].kind === "fileChangeDiff"
        ? projection.turns[0].items[0].payload.changes[0]
        : null,
    ]);
    expect(result.rejectedUnsafeOrStale).toBe(true);

    const oversizedPage = {
      ...projection,
      projectId: "x".repeat(513),
    } as AcpChatPage;
    expect(
      projectAgentEditSummary(oversizedPage, oversizedPage.turns[0]),
    ).toEqual({
      changes: [],
      omittedCount: 0,
      sourceTruncated: false,
      rejectedUnsafeOrStale: true,
    });
  });
});
