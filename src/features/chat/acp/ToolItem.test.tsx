import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { ToolItem } from "./ToolItem";

afterEach(cleanup);

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

function tool(
  presentation: ToolProjectionItem["payload"]["presentation"],
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
    toolState: "inProgress",
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
      screen.getByLabelText("Unknown provider tool tool"),
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
    expect(screen.getByText("src/main.ts")).toBeInTheDocument();
  });
});
