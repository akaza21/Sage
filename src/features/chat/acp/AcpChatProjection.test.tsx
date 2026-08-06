import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  AcpChatPage,
  AcpItem,
  HarnessActivityProjection,
  TurnProjection,
} from "../../../domain/acp";
import { AcpChatProjection } from "./AcpChatProjection";

afterEach(cleanup);

function textItem(): Extract<AcpItem, { kind: "text" }> {
  return {
    id: "item-text",
    kind: "text",
    origin: "primary",
    payload: { role: "assistant", text: "Normalized answer" },
  } as unknown as Extract<AcpItem, { kind: "text" }>;
}

function page(state: TurnProjection["state"]): AcpChatPage {
  return {
    projectId: "project-1",
    chatId: "chat-1",
    title: "Durable chat",
    projectionRevision: "12",
    currentSessionId: "session-1",
    sessions: [],
    nextCursor: null,
    turns: [
      {
        id: "turn-1",
        sessionId: "session-1",
        runtimeEpoch: "2",
        ordinal: 1,
        state,
        revision: "3",
        createdAt: 1,
        updatedAt: 2,
        terminalAt: state === "completed" ? 2 : null,
        items: [textItem()],
        activities: [],
      } as unknown as TurnProjection,
    ],
  } as unknown as AcpChatPage;
}

function activity(
  id: string,
  parentActivityId: string | null,
  parentItemId: string,
  depth: number,
): HarnessActivityProjection {
  return {
    id,
    turnId: "turn-1",
    parentActivityId,
    parentItemId,
    role: parentActivityId ? "child" : "root",
    lifecycle: "completed",
    depth,
    provenance: {
      schemaVersion: 1,
      sourceKind: "tool",
      externalKind: "child",
      extensionSchemaId: null,
      safeLabel: parentActivityId ? "Nested activity" : "Root activity",
    },
    revision: "1",
  } as HarnessActivityProjection;
}

function activityItem(
  id: string,
  activityId: string,
  item: Partial<AcpItem>,
): AcpItem {
  return {
    id,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "2",
    ordinal: 1,
    origin: "harnessActivity",
    activityId,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    ...item,
  } as AcpItem;
}

describe("AcpChatProjection", () => {
  it("exposes every Turn transition through the frozen atomic status semantics", () => {
    render(<AcpChatProjection page={page("waitingUser")} active />);
    const status = screen.getByRole("status", { name: "Waiting for user" });
    expect(status).toHaveTextContent("Waiting for user");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveAttribute("aria-atomic", "true");
  });

  it("renders lifecycle state and normalized items", () => {
    render(<AcpChatProjection page={page("waitingModel")} active />);
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for model");
    expect(screen.getByText("Normalized answer")).toBeInTheDocument();
  });

  it("keeps cancellation out of the transcript projection", () => {
    render(<AcpChatProjection page={page("completed")} active />);
    expect(screen.getByRole("status")).toHaveTextContent("Completed");
    expect(screen.queryByRole("button", { name: /cancel turn/i })).toBeNull();
  });

  it("fully unmounts an inactive heavy projection", () => {
    const { container } = render(
      <AcpChatProjection page={page("running")} active={false} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("exposes earlier history only as an explicit accessible action", () => {
    const loadEarlier = vi.fn();
    const history = page("completed");
    history.nextCursor = "cursor" as never;
    render(
      <AcpChatProjection
        page={history}
        active
        hasEarlier
        onLoadEarlier={loadEarlier}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Load earlier history" }),
    );
    expect(loadEarlier).toHaveBeenCalledTimes(1);
  });

  it("navigates real nested activity items without removing their controls from Tab order", () => {
    const nested = page("completed");
    nested.turns[0].items = [
      textItem(),
      activityItem("root-tool", "activity-root", {
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "file",
            label: "Read source",
            resources: [
              {
                kind: "resource",
                resource: { rootId: "root-a", relativePath: "src/main.ts" },
              },
            ],
          },
          inputSummary: "Read one source file",
        },
      }),
      activityItem("nested-unknown", "activity-child", {
        kind: "unsupported",
        toolState: null,
        payload: {
          category: "extension.future",
          summary: "Normalized unsupported activity",
          truncated: false,
        },
      }),
    ];
    nested.turns[0].activities = [
      activity("activity-root", null, "item-text", 1),
      activity("activity-child", "activity-root", "root-tool", 2),
    ];
    const openResource = vi.fn();
    render(
      <AcpChatProjection page={nested} active onOpenResource={openResource} />,
    );

    const activityGroup = screen.getByRole("group", {
      name: "Harness activity: Root activity",
    });
    const activityDisclosure = within(activityGroup)
      .getByText("Root activity")
      .closest("summary");
    fireEvent.click(activityDisclosure!);

    const tree = within(activityGroup).getByRole("tree");
    const rootNode = tree.querySelector<HTMLElement>(
      '[data-sage-id="root-tool"]',
    );
    expect(rootNode).not.toBeNull();
    expect(rootNode).toHaveAttribute("aria-level", "1");
    expect(rootNode).toHaveAttribute("aria-expanded", "false");
    expect(
      tree.querySelectorAll('[role="treeitem"][tabindex="0"]'),
    ).toHaveLength(1);

    const toolDisclosure = within(rootNode!).getByText("Tool details");
    expect(toolDisclosure).toHaveProperty("tabIndex", 0);
    fireEvent.click(toolDisclosure);
    expect(
      within(rootNode!).getByRole("button", {
        name: "Open file src/main.ts",
      }),
    ).toHaveProperty("tabIndex", 0);

    rootNode!.focus();
    fireEvent.keyDown(rootNode!, { key: "Enter" });
    expect(openResource).toHaveBeenCalledWith(
      { rootId: "root-a", relativePath: "src/main.ts" },
      "file",
    );
    fireEvent.keyDown(rootNode!, { key: "ArrowRight" });
    expect(rootNode).toHaveAttribute("aria-expanded", "true");
    const childNode = tree.querySelector<HTMLElement>(
      '[data-sage-id="nested-unknown"]',
    );
    expect(childNode).not.toBeNull();
    expect(childNode).toHaveAttribute("aria-level", "2");
    fireEvent.keyDown(rootNode!, { key: "ArrowRight" });
    expect(childNode).toHaveFocus();
    fireEvent.keyDown(childNode!, { key: "ArrowLeft" });
    expect(rootNode).toHaveFocus();
    fireEvent.keyDown(rootNode!, { key: "ArrowLeft" });
    expect(rootNode).toHaveAttribute("aria-expanded", "false");
    expect(tree.querySelector('[data-sage-id="nested-unknown"]')).toBeNull();
    fireEvent.keyDown(rootNode!, { key: "ArrowLeft" });
    expect(activityDisclosure).toHaveFocus();
  });
});
