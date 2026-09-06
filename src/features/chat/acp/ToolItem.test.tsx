import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../../../app/agentArtifactFollowStore";
import type { AcpItem } from "../../../domain/acp";
import {
  browseSessionForOwner,
  clearBrowseSessions,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../../preview/harnessBrowse";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";
import { ToolItem } from "./ToolItem";

afterEach(() => {
  cleanup();
  clearBrowseSessions();
  useAgentArtifactFollowStore.getState().clear();
});

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;
const browseOwner = {
  projectId: "project-1",
  chatId: "chat-1",
  sessionId: "session-1",
} as AgentArtifactOwner;
const previewOwner = {
  ...browseOwner,
  projectGeneration: "1",
  transitionEpoch: 1,
  runtimeEpoch: "1",
} as HarnessBrowseOwner;

beforeEach(() => setHarnessBrowseOwner(previewOwner, [previewOwner.chatId]));

function tool(
  presentation: ToolProjectionItem["payload"]["presentation"],
  toolState: ToolProjectionItem["toolState"] = "inProgress",
): ToolProjectionItem {
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
    kind: "toolCall",
    toolState,
    payload: { presentation, inputSummary: "Bounded retained input" },
  } as ToolProjectionItem;
}

describe("ToolItem", () => {
  it("opens canonical file and diff ResourceRefs while leaving foreign paths inert", () => {
    const onOpen = vi.fn();
    const { rerender } = render(
      <ToolItem
        item={tool({
          kind: "file",
          label: "Read files",
          resources: [
            {
              kind: "resource",
              resource: { rootId: "root-a", relativePath: "src/main.ts" },
            },
            {
              kind: "unavailable",
              reason: "foreign",
              label: "Outside project",
            },
          ],
        })}
        onOpenResource={onOpen}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Open file src/main.ts" }),
    );
    expect(onOpen).toHaveBeenCalledWith(
      { rootId: "root-a", relativePath: "src/main.ts" },
      "file",
    );
    expect(
      screen.getByText("Outside project · unavailable"),
    ).toBeInTheDocument();

    rerender(
      <ToolItem
        item={tool({
          kind: "diff",
          label: "Changed file",
          resources: [
            {
              kind: "resource",
              resource: { rootId: "root-b", relativePath: "src/main.ts" },
            },
          ],
        })}
        onOpenResource={onOpen}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Open diff src/main.ts" }),
    );
    expect(onOpen).toHaveBeenLastCalledWith(
      { rootId: "root-b", relativePath: "src/main.ts" },
      "diff",
    );
  });

  it("shows Read and command markers inline without opening Tool details", () => {
    const fileItem = tool({
      kind: "file",
      label: "Read file",
      resources: [
        {
          kind: "resource",
          resource: { rootId: "root-a", relativePath: "src/main.ts" },
        },
      ],
    });
    fileItem.payload.inputSummary = null;
    const { rerender } = render(
      <ToolItem item={fileItem} onOpenResource={vi.fn()} />,
    );

    expect(
      screen.getByRole("button", { name: "Open file src/main.ts" }),
    ).toBeVisible();
    expect(screen.queryByText("Tool details")).toBeNull();

    rerender(
      <ToolItem
        item={tool({
          kind: "command",
          label: "Run tests",
          summary: "npm test -- src/main.ts",
        })}
      />,
    );
    expect(screen.getByLabelText("Command")).toHaveTextContent(
      "npm test -- src/main.ts",
    );
    expect(screen.getByText("Tool details")).toBeInTheDocument();
  });

  it("uses one generic card for unknown tools and typed safe summaries", () => {
    const { rerender } = render(
      <ToolItem
        item={tool({
          kind: "generic",
          label: "Unknown provider tool",
          summary: "Normalized fallback",
        })}
      />,
    );
    expect(
      screen.getByRole("group", {
        name: "Unknown provider tool",
        description: "Bounded retained input",
      }),
    ).toHaveTextContent("Normalized fallback");
    expect(screen.getByRole("status")).toHaveTextContent("In progress");

    rerender(
      <ToolItem
        item={tool({
          kind: "mcp",
          label: "MCP tool",
          serverLabel: "Issue tracker",
          toolLabel: "Read issue",
        })}
      />,
    );
    expect(screen.getByText("Issue tracker · Read issue")).toBeInTheDocument();
  });

  it("keeps a terminal tool state accessible without replaying it", () => {
    render(
      <ToolItem
        item={tool(
          {
            kind: "generic",
            label: "Unknown provider tool",
            summary: "Normalized fallback",
          },
          "completed",
        )}
      />,
    );
    const status = screen.getByRole("status", {
      name: "Unknown provider tool: Completed",
    });
    expect(status).toHaveTextContent("Completed");
    expect(status).toHaveAttribute("aria-live", "off");
    expect(status).toHaveAttribute("aria-atomic", "true");
  });

  it.each([
    "pending",
    "inProgress",
    "waitingForApproval",
    "completed",
    "failed",
    "declined",
    "cancelled",
  ] as const)("exposes the %s state to the shared card grammar", (state) => {
    const { container } = render(
      <ToolItem
        item={tool(
          { kind: "generic", label: "Tool state", summary: "Bounded detail" },
          state,
        )}
      />,
    );
    expect(container.querySelector(".acp-activity-card--tool")).toHaveAttribute(
      "data-activity-kind",
      "generic",
    );
    expect(container.querySelector(".acp-activity-card--tool")).toHaveAttribute(
      "data-activity-state",
      state,
    );
  });

  it("opens an admitted loopback web tool in the Sage browser", () => {
    render(
      <ToolItem
        item={tool({
          kind: "web",
          label: "Browse preview",
          summary: "Opened http://127.0.0.1:5173/app",
        })}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Open in Sage browser" }),
    );
    expect(browseSessionForOwner(previewOwner)?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        browseOwner,
      ),
    ).toMatchObject({ mode: "pinned", selected: { kind: "preview" } });
  });

  it("does not pin a historical or non-interactive browse item", () => {
    render(
      <ToolItem
        allowBrowse={false}
        item={tool({
          kind: "web",
          label: "Browse preview",
          summary: "Opened http://127.0.0.1:5173/history",
        })}
      />,
    );
    expect(
      screen.queryByRole("button", { name: "Open in Sage browser" }),
    ).toBeNull();
    expect(browseSessionForOwner(previewOwner)).toBeNull();
    expect(useAgentArtifactFollowStore.getState().records).toEqual([]);
  });

  it("keeps canonical resources inert until a native open handler is wired", () => {
    render(
      <ToolItem
        item={tool({
          kind: "file",
          label: "Read file",
          resources: [
            {
              kind: "resource",
              resource: { rootId: "root-a", relativePath: "src/main.ts" },
            },
          ],
        })}
      />,
    );
    expect(screen.queryByRole("button", { name: /open file/i })).toBeNull();
    expect(screen.getByText("main.ts")).toBeInTheDocument();
    expect(
      document.querySelector(".file-kind-icon--typescript"),
    ).not.toBeNull();
  });
});
