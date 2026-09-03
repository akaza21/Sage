import { describe, expect, it } from "vitest";

import type {
  AcpChatPage,
  AcpItem,
  AgentSessionProjection,
  FileChangeProjection,
  TurnProjection,
} from "../domain/acp";
import { artifactChipStatsForTab } from "./artifactChipStats";

type FileChangeItem = Extract<AcpItem, { kind: "fileChangeDiff" }>;

function change(
  rootId: string,
  relativePath: string,
  additions: number | null,
  deletions: number | null,
): FileChangeProjection {
  return {
    resource: {
      kind: "resource",
      resource: { rootId, relativePath },
    },
    operation: "updated",
    originalResource: null,
    summary: "edited",
    additions,
    deletions,
  };
}

function fileItem(
  id: string,
  ordinal: number,
  changes: readonly FileChangeProjection[],
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
      changes: [...changes],
      truncated: false,
    },
    ...overrides,
  } as FileChangeItem;
}

function turn(
  items: readonly AcpItem[],
  overrides: Partial<TurnProjection> = {},
): TurnProjection {
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
    ...overrides,
  } as unknown as TurnProjection;
}

function page(turns: readonly TurnProjection[]): AcpChatPage {
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
    turns: [...turns],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

const matchedTab = {
  kind: "diff" as const,
  resource: { rootId: "root-a", relativePath: "src/app.ts" },
};

describe("artifact chip stats", () => {
  it("matches a file or diff tab to the current turn summary and formats +3 −1", () => {
    const projection = page([
      turn([fileItem("item-a", 1, [change("root-a", "src/app.ts", 3, 1)])]),
    ]);

    expect(artifactChipStatsForTab(matchedTab, projection)).toEqual({
      additionsLabel: "+3",
      deletionsLabel: "-1",
    });
    expect(
      artifactChipStatsForTab({ ...matchedTab, kind: "file" }, projection),
    ).toEqual({
      additionsLabel: "+3",
      deletionsLabel: "-1",
    });
  });

  it("returns no counters when the tab resource is not in the summary", () => {
    const projection = page([
      turn([fileItem("item-a", 1, [change("root-a", "src/app.ts", 3, 1)])]),
    ]);

    expect(
      artifactChipStatsForTab(
        {
          kind: "file",
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        },
        projection,
      ),
    ).toBeNull();
    expect(
      artifactChipStatsForTab(
        {
          kind: "diff",
          resource: { rootId: "root-b", relativePath: "src/app.ts" },
        },
        projection,
      ),
    ).toBeNull();
  });

  it("omits a null side and does not forge 0/0", () => {
    const additionsOnly = page([
      turn([fileItem("item-a", 1, [change("root-a", "src/app.ts", 3, null)])]),
    ]);
    const deletionsOnly = page([
      turn([fileItem("item-a", 1, [change("root-a", "src/app.ts", null, 1)])]),
    ]);
    const bothNull = page([
      turn([
        fileItem("item-a", 1, [change("root-a", "src/app.ts", null, null)]),
      ]),
    ]);

    expect(artifactChipStatsForTab(matchedTab, additionsOnly)).toEqual({
      additionsLabel: "+3",
      deletionsLabel: null,
    });
    expect(artifactChipStatsForTab(matchedTab, deletionsOnly)).toEqual({
      additionsLabel: null,
      deletionsLabel: "-1",
    });
    expect(artifactChipStatsForTab(matchedTab, bothNull)).toBeNull();
  });

  it("caps displayed integers at 999+", () => {
    const projection = page([
      turn([
        fileItem("item-a", 1, [change("root-a", "src/app.ts", 1000, 999)]),
      ]),
    ]);

    expect(artifactChipStatsForTab(matchedTab, projection)).toEqual({
      additionsLabel: "+999+",
      deletionsLabel: "-999",
    });
  });

  it("uses the current in-progress turn instead of an older completed summary", () => {
    const projection = page([
      turn([fileItem("old", 1, [change("root-a", "src/app.ts", 9, 8)])], {
        id: "turn-old" as never,
        ordinal: 1,
        state: "completed",
      }),
      turn(
        [
          fileItem("live", 1, [change("root-a", "src/app.ts", 3, 1)], {
            turnId: "turn-live" as never,
          }),
        ],
        {
          id: "turn-live" as never,
          ordinal: 2,
          state: "running",
          terminalAt: null,
        },
      ),
    ]);

    expect(artifactChipStatsForTab(matchedTab, projection)).toEqual({
      additionsLabel: "+3",
      deletionsLabel: "-1",
    });
  });

  it("never falls back to an older session when the current session has no turn", () => {
    const projection = page([
      turn([fileItem("old", 1, [change("root-a", "src/app.ts", 9, 8)])], {
        sessionId: "session-old" as never,
      }),
    ]);

    expect(artifactChipStatsForTab(matchedTab, projection)).toBeNull();
  });

  it("returns no counters without a focused page or tab resource", () => {
    expect(artifactChipStatsForTab(matchedTab, null)).toBeNull();
    expect(
      artifactChipStatsForTab({ kind: "diff" }, page([turn([])])),
    ).toBeNull();
    expect(
      artifactChipStatsForTab(
        { kind: "chat", resource: matchedTab.resource },
        page([
          turn([fileItem("item-a", 1, [change("root-a", "src/app.ts", 3, 1)])]),
        ]),
      ),
    ).toBeNull();
  });
});
