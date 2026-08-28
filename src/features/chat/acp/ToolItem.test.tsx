import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import {
  browseSessionForChat,
  clearBrowseSessions,
} from "../../preview/harnessBrowse";
import { ToolItem } from "./ToolItem";

afterEach(() => {
  cleanup();
  clearBrowseSessions();
});

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

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
    expect(browseSessionForChat("chat-1")?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
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
