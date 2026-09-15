import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
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
): ToolItem {
  const presentation =
    kind === "file" || kind === "diff"
      ? { kind, label: `${kind} activity`, resources: [] }
      : kind === "command"
        ? { kind, label: "Command activity", summary: null, resources: [] }
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

  it("uses kind-faithful counts without inferring file or diff intent", () => {
    expect(toolRollupLabel("command", 2)).toBe("Ran 2 commands");
    expect(toolRollupLabel("file", 2)).toBe("File activity · 2 items");
    expect(toolRollupLabel("diff", 2)).toBe("Diff activity · 2 items");
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
  });

  it("expands into the existing ToolItem cards", () => {
    const first = tool("file-1", "file");
    const second = tool("file-2", "file");
    render(
      <ToolActivityRollup
        rollup={{ kind: "file", items: [first, second] }}
        linkedResults={new Map()}
        interactive
      />,
    );

    const disclosure = document.querySelector(".acp-tool-rollup");
    expect(disclosure).not.toBeNull();
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.queryByRole("group", { name: "file activity" })).toBeNull();
    fireEvent.click(screen.getByText("File activity · 2 items"));
    expect(
      screen.getAllByRole("group", { name: "file activity" }),
    ).toHaveLength(2);
    expect(screen.getByText("Completed · Hide")).toBeVisible();
  });
});
