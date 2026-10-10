import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem, SafeResourceLocation } from "../../../domain/acp";
import { ToolActivityRollup } from "./ToolActivityRollupView";
import {
  groupPrimaryActivities,
  toolRollupLabel,
  toolRollupDurationLabel,
  toolRollupLeadDetail,
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
    kind === "read" || kind === "edit" || kind === "delete"
      ? { kind, label: `${kind} activity`, resources }
      : kind === "move"
        ? {
            kind,
            label: "Move activity",
            resources,
            originalResources: [],
          }
        : kind === "execute"
          ? {
              kind,
              label: "Command activity",
              summary: null,
              cwd: null,
              exitCode: null,
              outputTail: null,
              outputTruncated: false,
              resources: [],
              activity: "ordinary" as const,
            }
          : kind === "search"
            ? {
                kind,
                label: "Search activity",
                querySummary: "needle",
                resources: [],
                matchCount: null,
                truncated: false,
              }
            : kind === "mcp"
              ? {
                  kind,
                  label: "MCP activity",
                  serverLabel: "server",
                  toolLabel: "tool",
                  argumentsSummary: null,
                  resultSummary: null,
                }
              : kind === "subagent"
                ? { kind, label: "Subagent activity", activityId: null }
                : kind === "fetch"
                  ? { kind, label: "Web activity", url: null, summary: null }
                  : kind === "switchMode"
                    ? {
                        kind,
                        label: "Mode activity",
                        modeId: null,
                        modeLabel: null,
                      }
                    : kind === "think"
                      ? { kind, label: "Thinking", summary: null }
                      : {
                          kind,
                          label: `${kind} activity`,
                          rawKind: null,
                          summary: null,
                        };
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
  it("groups adjacent completed tools in order and bounds a group", () => {
    const items = [
      tool("file-1", "read"),
      tool("file-2", "read"),
      tool("command", "execute"),
      tool("file-3", "read"),
    ];
    const grouped = groupPrimaryActivities(items);

    expect(grouped).toHaveLength(1);
    expect(grouped[0]).toMatchObject({
      kind: "toolRollup",
      rollup: {
        kind: "mixed",
        items: [
          { id: "file-1" },
          { id: "file-2" },
          { id: "command" },
          { id: "file-3" },
        ],
      },
    });
    const bounded = groupPrimaryActivities(
      Array.from({ length: 65 }, (_, index) => tool(`file-${index}`, "read")),
    );
    expect(bounded).toHaveLength(2);
    expect(
      bounded[0].kind === "toolRollup" && bounded[0].rollup.items,
    ).toHaveLength(64);
    expect(bounded[1]).toMatchObject({
      kind: "toolRollup",
      rollup: { items: [{ id: "file-64" }] },
    });
  });

  it("gives singleton edit, command, and read activity compact disclosures", () => {
    for (const [kind, item] of [
      ["edit", tool("edit-one", "edit", [mainFile])],
      ["execute", tool("command-one", "execute")],
      ["read", tool("read-one", "read", [otherFile])],
    ] as const) {
      expect(groupPrimaryActivities([item])).toMatchObject([
        { kind: "toolRollup", rollup: { kind, items: [{ id: item.id }] } },
      ]);
    }
  });

  it("keeps a single MCP call in a collapsed row named for its server and tool", () => {
    const mcp = tool("mcp-one", "mcp");
    expect(groupPrimaryActivities([mcp])).toMatchObject([
      {
        kind: "toolRollup",
        rollup: { kind: "mcp", items: [{ id: "mcp-one" }] },
      },
    ]);
    render(
      <ToolActivityRollup
        rollup={{ kind: "mcp", commandActivity: "ordinary", items: [mcp] }}
        interactive
      />,
    );
    const row = document.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    expect(row).not.toHaveAttribute("open");
    expect(row?.querySelector("summary")).toHaveTextContent("server · tool");
    expect(screen.queryByRole("group", { name: "MCP activity" })).toBeNull();
  });

  it("counts distinct resource identities for complete file and diff groups", () => {
    const repeatedFile = [
      tool("file-1", "read", [mainFile]),
      tool("file-2", "read", [mainFile]),
      tool("file-3", "read", [mainFile]),
    ];

    expect(toolRollupLabel("read", repeatedFile)).toBe("Read 1 file");
    expect(
      toolRollupLabel("edit", [
        tool("diff-1", "edit", [mainFile]),
        tool("diff-2", "edit", [samePathOtherRoot]),
      ]),
    ).toBe("Edited 2 files");
  });

  it("keeps item counts when a group has no locations", () => {
    const items = [tool("file-1", "read"), tool("file-2", "read")];

    expect(toolRollupLabel("read", items)).toBe("Read 2 items");
  });

  it("keeps item counts for mixed location coverage", () => {
    const items = [tool("file-1", "read", [mainFile]), tool("file-2", "read")];

    expect(toolRollupLabel("read", items)).toBe("Read 2 items");
  });

  it("keeps item counts when a location is unavailable", () => {
    const items = [
      tool("file-1", "read", [mainFile]),
      tool("file-2", "read", [unavailableFile]),
    ];

    expect(toolRollupLabel("read", items)).toBe("Read 2 items");
  });

  it("does not imply one known file in a mixed row with unknown locations", () => {
    const command = tool("command", "execute");
    const { rerender } = render(
      <ToolActivityRollup
        rollup={{
          kind: "mixed",
          commandActivity: "ordinary",
          items: [tool("edit-unknown", "edit"), command],
        }}
        interactive
      />,
    );
    expect(document.querySelector(".acp-tool-rollup__label")).toHaveTextContent(
      "Edited files, ran 1 command",
    );

    rerender(
      <ToolActivityRollup
        rollup={{
          kind: "mixed",
          commandActivity: "ordinary",
          items: [tool("read-unavailable", "read", [unavailableFile]), command],
        }}
        interactive
      />,
    );
    expect(document.querySelector(".acp-tool-rollup__label")).toHaveTextContent(
      "Read files, ran 1 command",
    );
  });

  it("uses call counts for ordinary commands", () => {
    expect(toolRollupLabel("execute", [tool("command", "execute")])).toBe(
      "Ran 1 command",
    );
  });

  it.each([
    ["inProgress", "Running"],
    ["completed", "Ran"],
    ["pending", "Pending"],
    ["waitingForApproval", "Awaiting approval \u00b7"],
    ["failed", "Failed"],
    ["cancelled", "Cancelled"],
    ["declined", "Declined"],
  ] as const)(
    "shows native input in one collapsed %s row and the same expanded command",
    (toolState, action) => {
      const base = tool("native-command", "execute");
      const command: ToolItem = {
        ...base,
        toolState,
        lifecycle:
          toolState === "pending"
            ? "started"
            : toolState === "inProgress" || toolState === "waitingForApproval"
              ? "streaming"
              : toolState === "failed"
                ? "failed"
                : toolState === "completed"
                  ? "completed"
                  : "cancelled",
        terminalAt:
          toolState === "pending" ||
          toolState === "inProgress" ||
          toolState === "waitingForApproval"
            ? null
            : 1,
        payload: { ...base.payload, inputSummary: "npm run check" },
      };
      render(
        <ToolActivityRollup
          rollup={{
            kind: "execute",
            commandActivity: "ordinary",
            items: [command],
          }}
          interactive
        />,
      );
      const row =
        document.querySelector<HTMLDetailsElement>(".acp-tool-rollup")!;
      const summary = row.querySelector("summary")!;
      expect(row).not.toHaveAttribute("open");
      expect(row).toHaveAttribute("data-activity-state", toolState);
      expect(
        summary.querySelector(".acp-tool-rollup__label"),
      ).toHaveTextContent(`${action} npm run check`);
      if (toolState !== "completed" && toolState !== "inProgress") {
        expect(summary.querySelector(".acp-live-shimmer")).toBeNull();
        expect(summary.querySelector(".acp-state")).toHaveTextContent(
          toolState === "pending"
            ? "Pending"
            : toolState === "waitingForApproval"
              ? "Waiting for approval"
              : action,
        );
      }
      expect(screen.queryByLabelText("Command line")).toBeNull();
      row.open = true;
      fireEvent(row, new Event("toggle"));
      expect(screen.getByLabelText("Command line")).toHaveTextContent(
        "npm run check",
      );
    },
  );

  it("refreshes a mounted collapsed row when its input arrives later", () => {
    const command = {
      ...tool("late-command", "execute"),
      toolState: "inProgress" as const,
    };
    const rollup = {
      kind: "execute" as const,
      commandActivity: "ordinary" as const,
      items: [command],
    };
    const { rerender } = render(
      <ToolActivityRollup rollup={rollup} interactive />,
    );
    const row = document.querySelector(".acp-tool-rollup");
    expect(row).toHaveTextContent("Running command");
    command.payload.inputSummary = "npm run lint";
    rerender(<ToolActivityRollup rollup={rollup} interactive />);
    expect(document.querySelectorAll(".acp-tool-rollup")).toHaveLength(1);
    expect(document.querySelector(".acp-tool-rollup")).toBe(row);
    expect(row).toHaveTextContent("Running npm run lint");
    expect(row).not.toHaveAttribute("open");
  });

  it("shows Codex-shaped input that equals its label without duplicating the expanded command", () => {
    const base = tool("codex-command", "execute");
    const command = {
      ...base,
      toolState: "inProgress" as const,
      payload: {
        inputSummary: "npm run check",
        presentation: { ...base.payload.presentation, label: "npm run check" },
      },
    };
    render(
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [command],
        }}
        interactive
      />,
    );
    const row = document.querySelector<HTMLDetailsElement>(".acp-tool-rollup")!;
    expect(row.querySelector("summary")).toHaveTextContent(
      "Running npm run check",
    );
    row.open = true;
    fireEvent(row, new Event("toggle"));
    expect(screen.getByRole("group", { name: "npm run check" })).toBeVisible();
    expect(screen.queryByLabelText("Command line")).toBeNull();
  });

  it("shows one live command row with its command before expansion", () => {
    const command: ToolItem = {
      ...tool("live-command", "execute"),
      lifecycle: "streaming",
      toolState: "inProgress",
      terminalAt: null,
      payload: {
        presentation: {
          kind: "execute",
          label: "Run command",
          summary: "powershell -NoProfile -File build.ps1",
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
          resources: [],
          activity: "ordinary",
        },
        inputSummary: null,
      },
    };
    render(
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [command],
        }}
        interactive
      />,
    );

    const row = document.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    const summary = row?.querySelector("summary");
    expect(row).not.toHaveAttribute("open");
    expect(summary).toHaveTextContent(
      "Running powershell -NoProfile -File build.ps1",
    );
    expect(
      summary?.querySelectorAll('svg[data-activity-icon="execute"]'),
    ).toHaveLength(1);
    expect(summary?.querySelector(".acp-tool-rollup__working")).toBeNull();
    expect(screen.queryByText("In progress")).toBeNull();
    fireEvent.click(summary!);
    expect(row).toHaveAttribute("open");
  });

  it("shows a live edit as one shimmering row without a status chip", () => {
    const edit: ToolItem = {
      ...tool("live-edit", "edit", [mainFile]),
      lifecycle: "streaming",
      toolState: "inProgress",
      terminalAt: null,
    };
    render(
      <ToolActivityRollup
        rollup={{ kind: "edit", commandActivity: "ordinary", items: [edit] }}
        interactive
      />,
    );

    const row = document.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    expect(row?.querySelector("summary")).toHaveTextContent("Editing files");
    expect(row?.querySelector(".acp-tool-rollup__label")).toHaveClass(
      "acp-live-shimmer",
    );
    expect(row?.querySelector("summary .acp-state")).toBeNull();
    expect(
      row?.querySelector("summary svg[data-activity-icon='edit']"),
    ).not.toBeNull();
  });

  it("rolls up a declared test singleton without a pass count", () => {
    const test = {
      ...tool("test", "execute"),
      payload: {
        presentation: {
          kind: "execute" as const,
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
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
        rollup: { kind: "execute", commandActivity: "declaredTest" },
      },
    ]);
    expect(toolRollupLabel("execute", [test], "declaredTest")).toBe(
      "Ran tests · 1 command",
    );
  });

  it("shows a failed declared-test rollup without a false success meaning", () => {
    const failed = {
      ...tool("failed-test", "execute"),
      toolState: "failed" as const,
      payload: {
        presentation: {
          kind: "execute" as const,
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
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
          kind: "execute",
          commandActivity: "declaredTest",
          items: [failed],
        }}
        interactive
      />,
    );

    expect(screen.getByText("Test commands failed · 1 command")).toBeVisible();
    expect(screen.getByText("Failed")).toBeVisible();
  });

  it("keeps declared tests separate from ordinary command groups", () => {
    const ordinary = tool("ordinary", "execute");
    const declared = {
      ...tool("test", "execute"),
      payload: {
        presentation: {
          kind: "execute" as const,
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
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
      kind: "toolRollup",
      rollup: { commandActivity: "ordinary", items: [{ id: "ordinary" }] },
    });
    expect(grouped[1]).toMatchObject({
      kind: "toolRollup",
      rollup: { commandActivity: "declaredTest", items: [{ id: "test" }] },
    });
  });

  it("reflects failed, declined, cancelled, or active members in the rollup state", () => {
    const failed = {
      ...tool("failed", "execute"),
      toolState: "failed" as const,
    };
    const active = {
      ...tool("active", "execute"),
      toolState: "inProgress" as const,
    };
    const declined = {
      ...tool("declined", "execute"),
      toolState: "declined" as const,
    };
    const cancelled = {
      ...tool("cancelled", "execute"),
      toolState: "cancelled" as const,
    };
    expect(toolRollupState([failed, tool("completed", "execute")])).toBe(
      "failed",
    );
    expect(toolRollupState([active, tool("completed", "execute")])).toBe(
      "inProgress",
    );
    expect(toolRollupState([declined, tool("completed", "execute")])).toBe(
      "declined",
    );
    expect(toolRollupState([cancelled, tool("completed", "execute")])).toBe(
      "cancelled",
    );

    const declared = {
      ...tool("declared", "execute"),
      payload: {
        presentation: {
          kind: "execute" as const,
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
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

  it("keeps command details inside the expanded activity row", () => {
    const first = {
      ...tool("command-1", "execute"),
      payload: {
        presentation: {
          kind: "execute" as const,
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
          label: "Command activity",
          summary: "npm run build",
          resources: [],
          activity: "ordinary" as const,
        },
        inputSummary: null,
      },
    };
    render(
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [first, tool("command-2", "execute")],
        }}
        interactive
      />,
    );

    const disclosure =
      document.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.getByText("Ran 2 commands")).toBeVisible();
    expect(screen.queryByText("npm run build")).toBeNull();
    if (!disclosure) throw new Error("Expected the tool rollup disclosure");

    disclosure.open = true;
    fireEvent(disclosure, new Event("toggle"));

    expect(screen.getByText("npm run build")).toBeVisible();
  });

  it("uses concise activity labels with semantic icons", () => {
    const { rerender } = render(
      <ToolActivityRollup
        rollup={{
          kind: "edit",
          commandActivity: "ordinary",
          items: [tool("file-1", "edit", [mainFile])],
        }}
        interactive
      />,
    );

    expect(screen.getByText("Edited 1 file")).toBeVisible();
    expect(document.querySelector('[data-activity-icon="edit"]')).toBeVisible();

    rerender(
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [tool("command-1", "execute")],
        }}
        interactive
      />,
    );

    expect(screen.getByText("Ran 1 command")).toBeVisible();
    expect(
      document.querySelector('[data-activity-icon="execute"]'),
    ).toBeVisible();
  });

  it("omits a lead detail that repeats the rollup label or is absent", () => {
    expect(
      toolRollupLeadDetail("Read activity · 2 items", [
        {
          ...tool("file-1", "read"),
          payload: {
            presentation: {
              kind: "read" as const,
              label: "   ",
              resources: [],
            },
            inputSummary: null,
          },
        },
      ]),
    ).toBeNull();
    expect(toolRollupLeadDetail("Ran 1 command", [])).toBeNull();
    expect(
      toolRollupLeadDetail("Command activity", [tool("command", "execute")]),
    ).toBeNull();
  });

  it("summarises non-command leads without hiding unknown kinds", () => {
    expect(
      toolRollupLeadDetail("Search activity · 1 item", [
        tool("search", "search"),
      ]),
    ).toBe("needle");
    expect(
      toolRollupLeadDetail("MCP activity · 1 item", [tool("mcp", "mcp")]),
    ).toBe("server · tool");
    expect(
      toolRollupLeadDetail("Tool activity · 1 item", [
        tool("generic", "other"),
      ]),
    ).toBe("other activity");
  });

  it("keeps a generic write projection out of the edited-file rollup", () => {
    const genericWrite = tool("write-generic", "other");
    expect(toolRollupLabel("other", [genericWrite])).toBe(
      "Tool activity · 1 item",
    );
  });

  it("names a schema-defined context compaction as one muted activity row", () => {
    const compaction = {
      ...tool("compaction-1", "other"),
      payload: {
        presentation: {
          kind: "other" as const,
          rawKind: null,
          label: "Context automatically compacted",
          summary: null,
        },
        inputSummary: null,
      },
    };
    expect(toolRollupLabel("other", [compaction])).toBe(
      "Context automatically compacted",
    );
    expect(groupPrimaryActivities([compaction])).toMatchObject([
      { kind: "toolRollup", rollup: { items: [{ id: "compaction-1" }] } },
    ]);
  });

  it("reports a rollup total duration instead of a wall-clock time", () => {
    const first = {
      ...tool("command-1", "execute"),
      createdAt: 1_000,
      terminalAt: 3_000,
    };
    const second = {
      ...tool("command-2", "execute"),
      createdAt: 3_000,
      terminalAt: 9_500,
    };

    expect(toolRollupDurationLabel([first, second])).toBe("9s");
    expect(
      toolRollupDurationLabel([first, { ...second, terminalAt: null }]),
    ).toBeNull();
    expect(toolRollupDurationLabel([])).toBeNull();

    render(
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [first, second],
        }}
        interactive
      />,
    );

    const meta = document.querySelector(".acp-tool-rollup__meta");
    expect(meta?.querySelector("time")).toBeNull();
    expect(meta).toHaveTextContent("9s");
  });

  it("expands into the existing ToolItem cards", () => {
    const first = tool("file-1", "read", [mainFile]);
    const second = tool("file-2", "read", [otherFile]);
    render(
      <ToolActivityRollup
        rollup={{
          kind: "read",
          commandActivity: "ordinary",
          items: [first, second],
        }}
        interactive
      />,
    );

    const disclosure =
      document.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    expect(disclosure).not.toBeNull();
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.queryByRole("group", { name: "read activity" })).toBeNull();
    if (!disclosure) throw new Error("Expected the tool rollup disclosure");

    disclosure.open = true;
    fireEvent(disclosure, new Event("toggle"));

    expect(disclosure.open).toBe(true);
    expect(disclosure).toHaveAttribute("open");
    expect(
      screen.getAllByRole("group", { name: "read activity" }),
    ).toHaveLength(2);
    expect(
      [...disclosure.querySelectorAll("[data-acp-item-id]")].map((card) =>
        card.getAttribute("data-acp-item-id"),
      ),
    ).toEqual(["file-1", "file-2"]);
    expect(disclosure.querySelector("summary .acp-state")).toHaveTextContent(
      "Completed",
    );
  });
});
