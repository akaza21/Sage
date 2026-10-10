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
                ? {
                    kind,
                    label: "Subagent activity",
                    activityId: null,
                    taskLifecycle: true,
                  }
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
  it("leaves generic subagent tool wording unchanged without a binding-supplied Task lifecycle", () => {
    const generic = tool("generic", "subagent");
    generic.payload.presentation = {
      kind: "subagent",
      label: "Subagent activity",
      activityId: null,
    };
    const task = tool("task", "subagent");
    task.payload.presentation = {
      kind: "subagent",
      label: "Reviewer",
      activityId: null,
      taskLifecycle: true,
    } as ToolItem["payload"]["presentation"];
    expect(toolRollupLabel("subagent", [generic])).toBe(
      "Subagent activity · 1 item",
    );
    expect(groupPrimaryActivities([task, generic])).toHaveLength(2);
  });
  it("summarizes all 64 admitted Task tools including a failure in the final slot", () => {
    const items = Array.from({ length: 65 }, (_, index) =>
      tool(`task-${index}`, "subagent"),
    );
    items[63].toolState = "failed";
    const groups = groupPrimaryActivities(items);
    expect(groups).toHaveLength(2);
    const first = groups[0];
    if (first.kind !== "toolRollup") throw new Error("Expected Task rollup");
    expect(first.rollup.items).toHaveLength(64);
    expect(toolRollupLabel("subagent", first.rollup.items)).toBe(
      "63 subagents finished · Subagent failed",
    );
  });
  it("uses supplied Task labels and each tool's state without claiming child reads", async () => {
    const completed = tool("task-a", "subagent");
    completed.payload.presentation.label = "Review accessibility";
    const working = tool("task-b", "subagent");
    working.payload.presentation.label = "Check build";
    working.toolState = "inProgress";
    working.terminalAt = null;
    render(
      <ToolActivityRollup
        interactive
        rollup={{
          kind: "subagent",
          commandActivity: "ordinary",
          items: [completed, working],
        }}
      />,
    );
    expect(
      screen.getByText(
        "Review accessibility finished · Check build started working",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: /Open Subagent/ })).toBeNull();
    expect(screen.queryByRole("tab", { name: /Subagent/ })).toBeNull();
    fireEvent.click(
      screen.getByText(
        "Review accessibility finished · Check build started working",
      ),
    );
    expect(await screen.findByText("Review accessibility")).toBeVisible();
    expect(screen.getByText("Check build")).toBeVisible();
  });

  it("does not call pending, rejected, cancelled or failed Task tools started or finished", () => {
    for (const [state, wording] of [
      ["pending", "pending"],
      ["waitingForApproval", "waiting for approval"],
      ["declined", "declined"],
      ["cancelled", "stopped"],
      ["failed", "failed"],
    ] as const) {
      const task = tool("task", "subagent");
      task.payload.presentation.label = "Reviewer";
      task.toolState = state;
      expect(toolRollupLabel("subagent", [task])).toBe(`Reviewer ${wording}`);
    }
  });
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

  it.each([
    [1, "Ran 1 command"],
    [2, "Ran 2 commands"],
    [3, "Ran 3 commands"],
  ])("counts %s ordinary commands", (count, expected) => {
    expect(
      toolRollupLabel(
        "execute",
        Array.from({ length: count }, (_, index) =>
          tool(`command-${index}`, "execute"),
        ),
      ),
    ).toBe(expected);
  });

  it("preserves read and edit verbs while counting mixed command calls", () => {
    expect(
      toolRollupLabel("mixed", [
        tool("read", "read"),
        tool("edit", "edit"),
        tool("command", "execute"),
      ]),
    ).toBe("Read files, edited files, ran 1 command");
    expect(
      toolRollupLabel("mixed", [
        tool("read", "read"),
        tool("command-1", "execute"),
        tool("command-2", "execute"),
      ]),
    ).toBe("Read files, ran 2 commands");
  });

  it.each([
    ["completed", "Ran", "Ran tests", "Ran tests"],
    ["inProgress", "Running", "Running tests", "Running test command"],
    ["pending", "Pending", "Test commands pending", "Test command pending"],
    [
      "waitingForApproval",
      "Awaiting approval \u00b7",
      "Test commands awaiting approval",
      "Test command awaiting approval",
    ],
    ["failed", "Failed", "Test commands failed", "Test command failed"],
    [
      "cancelled",
      "Cancelled",
      "Test commands cancelled",
      "Test command cancelled",
    ],
    ["declined", "Declined", "Test commands declined", "Test command declined"],
  ] as const)(
    "preserves uniform %s command states and declared-test classification",
    (toolState, ordinary, declared, declaredSingleton) => {
      const items = [
        { ...tool("command-1", "execute"), toolState },
        { ...tool("command-2", "execute"), toolState },
      ];
      expect(toolRollupLabel("execute", items)).toBe(`${ordinary} 2 commands`);
      expect(toolRollupLabel("execute", items, "declaredTest")).toBe(
        `${declared} \u00b7 2 commands`,
      );
      expect(toolRollupLabel("execute", [items[0]], "declaredTest")).toBe(
        `${declaredSingleton} \u00b7 1 command`,
      );
    },
  );

  it("keeps empty activity and input-less unknown tools out of command groups", () => {
    expect(groupPrimaryActivities([])).toEqual([]);
    expect(groupPrimaryActivities([tool("other", "other")])).toMatchObject([
      { kind: "toolRollup", rollup: { kind: "other" } },
    ]);
  });

  it("keeps chronological command groups separate across prose and expands in order", () => {
    const prose = {
      ...tool("prose", "think"),
      kind: "text" as const,
      toolState: null,
      payload: { role: "assistant" as const, text: "Next step" },
    };
    const grouped = groupPrimaryActivities([
      tool("one", "execute"),
      tool("two", "execute"),
      prose,
      tool("three", "execute"),
    ]);
    expect(
      grouped.map((row) =>
        row.kind === "item"
          ? row.item.id
          : row.kind === "toolRollup"
            ? row.rollup.items.map((item) => item.id)
            : row.items.map((item) => item.id),
      ),
    ).toEqual([["one", "two"], "prose", ["three"]]);
    const first = grouped[0];
    if (first.kind !== "toolRollup")
      throw new Error("Expected a command rollup");
    render(<ToolActivityRollup rollup={first.rollup} interactive />);
    const row = document.querySelector<HTMLDetailsElement>(".acp-tool-rollup")!;
    row.open = true;
    fireEvent(row, new Event("toggle"));
    expect(
      [...row.querySelectorAll("[data-acp-item-id]")].map((card) =>
        card.getAttribute("data-acp-item-id"),
      ),
    ).toEqual(["one", "two"]);
  });

  it("caps 65 command calls in groups of 64 and one", () => {
    const grouped = groupPrimaryActivities(
      Array.from({ length: 65 }, (_, index) =>
        tool(`command-${index}`, "execute"),
      ),
    );
    expect(grouped).toMatchObject([
      {
        kind: "toolRollup",
        rollup: {
          items: Array.from({ length: 64 }, (_, index) => ({
            id: `command-${index}`,
          })),
        },
      },
      { kind: "toolRollup", rollup: { items: [{ id: "command-64" }] } },
    ]);
    const first = grouped[0];
    if (first.kind !== "toolRollup")
      throw new Error("Expected a command rollup");
    expect(first.rollup.items).toHaveLength(64);
    expect(toolRollupLabel("execute", first.rollup.items)).toBe(
      "Ran 64 commands",
    );
  });

  describe.each([
    ["ordinary", "2 commands"],
    ["declaredTest", "2 test commands"],
  ] as const)("mixed-state %s commands", (commandActivity, count) => {
    it.each([
      ["failed", "1 failed"],
      ["cancelled", "1 cancelled"],
      ["declined", "1 declined"],
      ["inProgress", "1 running"],
      ["pending", "1 pending"],
      ["waitingForApproval", "1 awaiting approval"],
    ] as const)(
      "does not apply %s to the completed command",
      (toolState, stateCount) => {
        const completed = tool("completed", "execute");
        const unsettled = { ...tool("unsettled", "execute"), toolState };
        const { rerender } = render(
          <ToolActivityRollup
            rollup={{
              kind: "execute",
              commandActivity,
              items: [completed, unsettled],
            }}
            interactive
          />,
        );
        expect(
          document.querySelector(".acp-tool-rollup__label")?.textContent,
        ).toBe(count + " \u00b7 " + stateCount);
        rerender(
          <ToolActivityRollup
            rollup={{
              kind: "execute",
              commandActivity,
              items: [unsettled, completed],
            }}
            interactive
          />,
        );
        expect(
          document.querySelector(".acp-tool-rollup__label")?.textContent,
        ).toBe(count + " \u00b7 " + stateCount);
      },
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
        expect(summary.querySelector(".acp-state")).toBeNull();
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

  it("ellipsizes only the collapsed heredoc and keeps its full retained input on expansion", () => {
    const base = tool("heredoc", "execute");
    const inputSummary = "cat <<'EOF'\n" + "x".repeat(1_000) + "\nEOF";
    const command = {
      ...base,
      toolState: "inProgress" as const,
      payload: { ...base.payload, inputSummary },
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
    const label = row.querySelector(".acp-tool-rollup__label")!;
    expect(label.textContent?.length).toBeLessThanOrEqual(520);
    expect(label.textContent).not.toContain("\n");
    expect(label.textContent).toMatch(/\u2026$/);
    row.open = true;
    fireEvent(row, new Event("toggle"));
    expect(screen.getByLabelText("Command line").textContent).toBe(
      inputSummary,
    );
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
    const { rerender } = render(
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
    rerender(
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [{ ...command, toolState: "completed" }],
        }}
        interactive
      />,
    );
    expect(row.querySelector("summary")).toHaveTextContent("Ran npm run check");
    expect(screen.queryByLabelText("Command line")).toBeNull();
  });

  it("preserves literal quoted spaces in a collapsed retained command", () => {
    const base = tool("quoted-command", "execute");
    const command = {
      ...base,
      toolState: "inProgress" as const,
      payload: { ...base.payload, inputSummary: "printf 'two  spaces'" },
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
    expect(document.querySelector(".acp-tool-rollup__label")?.textContent).toBe(
      "Running printf 'two  spaces'",
    );
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

  it.each([
    ["ordinary", "3 commands \u00b7 2 running, 1 failed"],
    ["declaredTest", "3 test commands \u00b7 2 running, 1 failed"],
  ] as const)(
    "keeps live work visible alongside a failed %s command",
    (commandActivity, expected) => {
      const items = [
        {
          ...tool("failed", "execute"),
          toolState: "failed" as const,
          lifecycle: "failed" as const,
        },
        {
          ...tool("running-one", "execute"),
          toolState: "inProgress" as const,
          lifecycle: "streaming" as const,
          terminalAt: null,
        },
        {
          ...tool("running-two", "execute"),
          toolState: "inProgress" as const,
          lifecycle: "streaming" as const,
          terminalAt: null,
        },
      ];
      for (const item of items) {
        if (item.payload.presentation.kind === "execute")
          item.payload.presentation.activity = commandActivity;
      }
      const rollup = { kind: "execute" as const, commandActivity, items };
      const { rerender } = render(
        <ToolActivityRollup rollup={rollup} interactive />,
      );
      const row =
        document.querySelector<HTMLDetailsElement>(".acp-tool-rollup")!;
      expect(row).toHaveAttribute("data-activity-state", "inProgress");
      expect(row.querySelector(".acp-tool-rollup__label")?.textContent).toBe(
        expected,
      );
      expect(row.querySelector(".acp-tool-rollup__label")).toHaveClass(
        "acp-live-shimmer",
      );
      expect(row.querySelector("summary .acp-state")).toBeNull();
      rerender(
        <ToolActivityRollup
          rollup={{ ...rollup, items: [...items].reverse() }}
          interactive
        />,
      );
      expect(row).toHaveAttribute("data-activity-state", "inProgress");
      expect(row.querySelector(".acp-tool-rollup__label")?.textContent).toBe(
        expected,
      );
      expect(row.querySelector(".acp-tool-rollup__label")).toHaveClass(
        "acp-live-shimmer",
      );
    },
  );

  describe.each(["pending", "waitingForApproval"] as const)(
    "non-command %s rollups",
    (toolState) => {
      it.each([
        "read",
        "edit",
        "delete",
        "move",
        "search",
        "fetch",
        "think",
        "switchMode",
        "mcp",
        "subagent",
        "other",
      ] as const)("preserves main's in-progress rendering for %s", (kind) => {
        const item = {
          ...tool("unsettled", kind),
          toolState,
          terminalAt: null,
        };
        render(
          <ToolActivityRollup
            rollup={{ kind, commandActivity: "ordinary", items: [item] }}
            interactive
          />,
        );
        const row = document.querySelector(".acp-tool-rollup")!;
        expect(row).toHaveAttribute("data-activity-state", "inProgress");
        expect(row.querySelector(".acp-tool-rollup__label")).toHaveClass(
          "acp-live-shimmer",
        );
        expect(row.querySelector("summary .acp-state")).toBeNull();
        expect(
          row.querySelectorAll(".acp-tool-rollup__summary-copy svg"),
        ).toHaveLength(1);
      });
    },
  );

  it("preserves main's failure precedence for non-command rollups", () => {
    const items = [
      { ...tool("failed", "read"), toolState: "failed" as const },
      { ...tool("pending", "read"), toolState: "pending" as const },
    ];
    expect(toolRollupState(items)).toBe("failed");
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

    expect(screen.getByText("Test command failed · 1 command")).toBeVisible();
    expect(screen.queryByText("Failed")).toBeNull();
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
    expect(toolRollupState([failed, active])).toBe("inProgress");
    expect(toolRollupLabel("execute", [failed, active])).toBe(
      "2 commands \u00b7 1 running, 1 failed",
    );
    for (const unsettled of [failed, active, declined, cancelled]) {
      const grouped = groupPrimaryActivities([
        unsettled,
        tool("completed", "execute"),
      ]);
      expect(grouped).toMatchObject([
        {
          kind: "toolRollup",
          rollup: {
            kind: "execute",
            items: [{ id: unsettled.id }, { id: "completed" }],
          },
        },
      ]);
    }

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
