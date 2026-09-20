import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem, SafeResourceLocation } from "../../../domain/acp";
import { ToolActivityRollup } from "./ToolActivityRollupView";
import {
  groupPrimaryActivities,
  toolRollupLabel,
  toolRollupState,
} from "./toolActivityRollup";

afterEach(cleanup);

type ToolItem = Extract<AcpItem, { kind: "toolCall" }>;

function tool(
  id: string,
  kind: ToolItem["payload"]["presentation"]["kind"],
  resources: readonly SafeResourceLocation[] = [],
): ToolItem {
  const presentation =
    kind === "file" || kind === "diff"
      ? { kind, label: `${kind} activity`, resources }
      : kind === "command"
        ? {
            kind,
            label: "Command activity",
            summary: null,
            resources: [],
            activity: "ordinary" as const,
          }
        : kind === "search"
          ? {
              kind,
              label: "Search activity",
              querySummary: "needle",
              resources: [],
            }
          : kind === "mcp"
            ? {
                kind,
                label: "MCP activity",
                serverLabel: "server",
                toolLabel: "tool",
              }
            : kind === "subagent"
              ? { kind, label: "Subagent activity", activityId: null }
              : { kind, label: `${kind} activity`, summary: null };
  return {
    id,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    kind: "toolCall",
    toolState: "completed",
    payload: { presentation, inputSummary: null },
  } as ToolItem;
}

const mainFile: SafeResourceLocation = {
  kind: "resource",
  resource: { rootId: "root-a", relativePath: "src/main.ts" },
};

const otherFile: SafeResourceLocation = {
  kind: "resource",
  resource: { rootId: "root-a", relativePath: "src/other.ts" },
};

const samePathOtherRoot: SafeResourceLocation = {
  kind: "resource",
  resource: { rootId: "root-b", relativePath: "src/main.ts" },
};

const unavailableFile: SafeResourceLocation = {
  kind: "unavailable",
  reason: "foreign",
  label: "Outside the project",
};

describe("ToolActivityRollup", () => {
  it("groups only consecutive presentation kinds and bounds a group", () => {
    const items = [
      tool("file-1", "file"),
      tool("file-2", "file"),
      tool("command", "command"),
      tool("file-3", "file"),
    ];
    const grouped = groupPrimaryActivities(items);

    expect(grouped).toHaveLength(3);
    expect(grouped[0]).toMatchObject({
      kind: "toolRollup",
      rollup: { kind: "file", items: [{ id: "file-1" }, { id: "file-2" }] },
    });
    expect(grouped[1]).toMatchObject({ kind: "item", item: { id: "command" } });
    expect(grouped[2]).toMatchObject({ kind: "item", item: { id: "file-3" } });
  });

  it("counts distinct resource identities for complete file and diff groups", () => {
    const repeatedFile = [
      tool("file-1", "file", [mainFile]),
      tool("file-2", "file", [mainFile]),
      tool("file-3", "file", [mainFile]),
    ];

    expect(toolRollupLabel("file", repeatedFile)).toBe(
      "File activity · 1 file",
    );
    expect(
      toolRollupLabel("diff", [
        tool("diff-1", "diff", [mainFile]),
        tool("diff-2", "diff", [samePathOtherRoot]),
      ]),
    ).toBe("Diff activity · 2 files");
  });

  it("keeps item counts when a group has no locations", () => {
    const items = [tool("file-1", "file"), tool("file-2", "file")];

    expect(toolRollupLabel("file", items)).toBe("File activity · 2 items");
  });

  it("keeps item counts for mixed location coverage", () => {
    const items = [tool("file-1", "file", [mainFile]), tool("file-2", "file")];

    expect(toolRollupLabel("file", items)).toBe("File activity · 2 items");
  });

  it("keeps item counts when a location is unavailable", () => {
    const items = [
      tool("file-1", "file", [mainFile]),
      tool("file-2", "file", [unavailableFile]),
    ];

    expect(toolRollupLabel("file", items)).toBe("File activity · 2 items");
  });

  it("uses call counts for ordinary commands", () => {
    expect(toolRollupLabel("command", [tool("command", "command")])).toBe(
      "Ran 1 command",
    );
  });

  it("rolls up a declared test singleton without a pass count", () => {
    const test = {
      ...tool("test", "command"),
      payload: {
        presentation: {
          kind: "command" as const,
          label: "Command activity",
          summary: null,
          resources: [],
          activity: "declaredTest" as const,
        },
        inputSummary: null,
      },
    };

    const grouped = groupPrimaryActivities([test]);
    expect(grouped).toMatchObject([
      {
        kind: "toolRollup",
        rollup: { kind: "command", commandActivity: "declaredTest" },
      },
    ]);
    expect(toolRollupLabel("command", [test], "declaredTest")).toBe(
      "Ran tests",
    );
  });

  it("shows a failed declared-test rollup without a false success meaning", () => {
    const failed = {
      ...tool("failed-test", "command"),
      toolState: "failed" as const,
      payload: {
        presentation: {
          kind: "command" as const,
          label: "Command activity",
          summary: null,
          resources: [],
          activity: "declaredTest" as const,
        },
        inputSummary: null,
      },
    };
    render(
      <ToolActivityRollup
        rollup={{
          kind: "command",
          commandActivity: "declaredTest",
          items: [failed],
        }}
        linkedResults={new Map()}
        interactive
      />,
    );

    expect(screen.getByText("Ran tests")).toBeVisible();
    expect(screen.getByText("Failed · Show")).toBeVisible();
  });

  it("keeps declared tests separate from ordinary command groups", () => {
    const ordinary = tool("ordinary", "command");
    const declared = {
      ...tool("test", "command"),
      payload: {
        presentation: {
          kind: "command" as const,
          label: "Command activity",
          summary: null,
          resources: [],
          activity: "declaredTest" as const,
        },
        inputSummary: null,
      },
    };
    const grouped = groupPrimaryActivities([ordinary, declared]);
    expect(grouped).toHaveLength(2);
    expect(grouped[0]).toMatchObject({
      kind: "item",
      item: { id: "ordinary" },
    });
    expect(grouped[1]).toMatchObject({
      kind: "toolRollup",
      rollup: { commandActivity: "declaredTest", items: [{ id: "test" }] },
    });
  });

  it("reflects failed, declined, cancelled, or active members in the rollup state", () => {
    const failed = {
      ...tool("failed", "command"),
      toolState: "failed" as const,
    };
    const active = {
      ...tool("active", "command"),
      toolState: "inProgress" as const,
    };
    const declined = {
      ...tool("declined", "command"),
      toolState: "declined" as const,
    };
    const cancelled = {
      ...tool("cancelled", "command"),
      toolState: "cancelled" as const,
    };
    expect(toolRollupState([failed, tool("completed", "command")])).toBe(
      "failed",
    );
    expect(toolRollupState([active, tool("completed", "command")])).toBe(
      "inProgress",
    );
    expect(toolRollupState([declined, tool("completed", "command")])).toBe(
      "declined",
    );
    expect(toolRollupState([cancelled, tool("completed", "command")])).toBe(
      "cancelled",
    );

    const declared = {
      ...tool("declared", "command"),
      payload: {
        presentation: {
          kind: "command" as const,
          label: "Command activity",
          summary: null,
          resources: [],
          activity: "declaredTest" as const,
        },
        inputSummary: null,
      },
    };
    expect(
      toolRollupState([{ ...declared, toolState: "failed" as const }]),
    ).toBe("failed");
    expect(
      toolRollupState([{ ...declared, toolState: "declined" as const }]),
    ).toBe("declined");
    expect(
      toolRollupState([{ ...declared, toolState: "cancelled" as const }]),
    ).toBe("cancelled");
  });

  it("expands into the existing ToolItem cards", () => {
    const first = tool("file-1", "file", [mainFile]);
    const second = tool("file-2", "file", [otherFile]);
    render(
      <ToolActivityRollup
        rollup={{
          kind: "file",
          commandActivity: "ordinary",
          items: [first, second],
        }}
        linkedResults={new Map()}
        interactive
      />,
    );

    const disclosure =
      document.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    expect(disclosure).not.toBeNull();
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.queryByRole("group", { name: "file activity" })).toBeNull();
    if (!disclosure) throw new Error("Expected the tool rollup disclosure");

    disclosure.open = true;
    fireEvent(disclosure, new Event("toggle"));

    expect(disclosure.open).toBe(true);
    expect(disclosure).toHaveAttribute("open");
    expect(
      screen.getAllByRole("group", { name: "file activity" }),
    ).toHaveLength(2);
    expect(screen.getByText("Completed · Hide")).toBeVisible();
  });
});
