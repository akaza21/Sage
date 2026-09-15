import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import {
  resetAgentRailViewPrefs,
  syncAgentRailClock,
} from "../../../workbench/agentRailViewPrefs";
import { TurnActivityItem } from "./TurnActivityItem";

afterEach(() => {
  cleanup();
  resetAgentRailViewPrefs();
});

function item<T extends AcpItem["kind"]>(
  kind: T,
  payload: Extract<AcpItem, { kind: T }>["payload"],
): Extract<AcpItem, { kind: T }> {
  return {
    id: `item-${kind}`,
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
    updatedAt: 2,
    terminalAt: 2,
    kind,
    toolState: null,
    payload,
  } as Extract<AcpItem, { kind: T }>;
}

describe("TurnActivityItem", () => {
  it("freezes a completed Thought while a streaming Thought advances", () => {
    const thought = item("thoughtSummary", { summary: "Checking the files." });
    thought.createdAt = 1_000;
    thought.updatedAt = 9_000;
    thought.terminalAt = 3_000;
    const { rerender } = render(
      <TurnActivityItem item={thought} nowMs={20_000} />,
    );
    expect(screen.getByText("Thought 2s")).toBeVisible();

    rerender(<TurnActivityItem item={thought} nowMs={99_000} />);
    expect(screen.getByText("Thought 2s")).toBeVisible();

    const streaming = {
      ...thought,
      lifecycle: "streaming",
      terminalAt: null,
    } as typeof thought;
    rerender(<TurnActivityItem item={streaming} nowMs={4_000} />);
    expect(screen.getByText("Thought 3s")).toBeVisible();
    rerender(<TurnActivityItem item={streaming} nowMs={5_000} />);
    expect(screen.getByText("Thought 4s")).toBeVisible();

    rerender(
      <TurnActivityItem
        item={{ ...thought, terminalAt: 500 } as typeof thought}
        nowMs={99_000}
      />,
    );
    expect(screen.getByText("Thought")).toBeVisible();

    rerender(
      <TurnActivityItem
        item={
          {
            ...thought,
            terminalAt: null,
            updatedAt: 500,
          } as typeof thought
        }
        nowMs={99_000}
      />,
    );
    expect(screen.getByText("Thought")).toBeVisible();
  });

  it("responds to the real clock store only while a Thought is streaming", () => {
    const thought = item("thoughtSummary", { summary: "Checking the files." });
    thought.createdAt = 1_000;
    thought.updatedAt = 3_000;
    thought.terminalAt = null;
    thought.lifecycle = "streaming";
    act(() => syncAgentRailClock(4_000));
    const { rerender } = render(<TurnActivityItem item={thought} />);
    expect(screen.getByText("Thought 3s")).toBeVisible();

    act(() => syncAgentRailClock(5_000));
    expect(screen.getByText("Thought 4s")).toBeVisible();

    const terminal = {
      ...thought,
      lifecycle: "completed",
      terminalAt: 3_000,
    } as typeof thought;
    rerender(<TurnActivityItem item={terminal} />);
    expect(screen.getByText("Thought 2s")).toBeVisible();
    act(() => syncAgentRailClock(9_000));
    expect(screen.getByText("Thought 2s")).toBeVisible();
  });

  it("renders an unfinished streaming Mermaid Thought as receiving source", () => {
    const thought = item("thoughtSummary", {
      summary: "```mermaid\ngraph TD\nA -->",
    });
    thought.lifecycle = "streaming";
    thought.terminalAt = null;
    const { container } = render(
      <TurnActivityItem item={thought} nowMs={thought.createdAt} />,
    );
    const details = container.querySelector("details");
    expect(details).not.toBeNull();
    details!.open = true;
    expect(
      screen.getByRole("status", { name: "Receiving diagram source" }),
    ).toBeVisible();
    expect(screen.queryByText(/could not be drawn/i)).toBeNull();
  });

  it("presents Plan progress and human-readable entry states", () => {
    const plan = item("plan", {
      entries: [
        { label: "Inspect", state: "completed" },
        { label: "Implement", state: "inProgress" },
        { label: "Verify", state: "pending" },
        { label: "Publish", state: "failed" },
      ],
    });
    const { container } = render(<TurnActivityItem item={plan} />);

    const region = screen.getByRole("region", { name: "Agent plan" });
    expect(within(region).getByRole("status")).toHaveTextContent(
      "1 of 4 complete",
    );
    expect(region).toHaveTextContent("In progress");
    expect(region).not.toHaveTextContent("inProgress");
    expect(container.querySelector(".acp-plan")).toHaveAttribute(
      "data-activity-state",
      "failed",
    );
    const disclosure = within(region).getByRole("button", { name: /plan/i });
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(disclosure);
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(within(region).queryByRole("list")).toBeNull();
  });

  it("renders reported and unknown Usage as compact metadata", () => {
    const { container, rerender } = render(
      <TurnActivityItem
        item={item("usage", {
          inputTokens: 800,
          outputTokens: 434,
          cachedInputTokens: 100,
          totalTokens: 1234,
        })}
      />,
    );
    expect(screen.getByLabelText("Usage")).toHaveTextContent("1,234 tokens");
    expect(container.querySelector(".acp-usage")).toHaveAttribute(
      "data-activity-state",
      "reported",
    );

    rerender(
      <TurnActivityItem
        item={item("usage", {
          inputTokens: null,
          outputTokens: null,
          cachedInputTokens: null,
          totalTokens: null,
        })}
      />,
    );
    expect(screen.getByLabelText("Usage")).toHaveTextContent(
      "Tokens not reported",
    );
  });

  it("keeps recoverable and non-recoverable errors explicit", () => {
    const { rerender } = render(
      <TurnActivityItem
        item={item("error", {
          code: "PREVIEW_STOPPED",
          message: "Preview stopped safely.",
          recoverable: true,
        })}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Retry available");

    rerender(
      <TurnActivityItem
        item={item("error", {
          code: "SESSION_LOST",
          message: "Inspect the Chat before continuing.",
          recoverable: false,
        })}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Needs attention");
  });

  it("keeps Thought disclosure keyboard-native with visible working status", () => {
    const thought = item("thoughtSummary", { summary: "Checking the files." });
    const working = {
      ...item("error", {
        code: "unused",
        message: "unused",
        recoverable: false,
      }),
      id: "tool-working",
      kind: "toolCall",
      lifecycle: "streaming",
      terminalAt: null,
      toolState: "inProgress",
      payload: {
        presentation: {
          kind: "search",
          label: "Search workspace",
          querySummary: "activity cards",
          resources: [],
        },
        inputSummary: "Scanning files",
      },
    } as unknown as Extract<AcpItem, { kind: "toolCall" }>;
    const { container } = render(
      <TurnActivityItem item={thought} workingItems={[working]} />,
    );

    expect(screen.getByText("In progress")).toBeVisible();
    expect(screen.getByText("Search workspace")).not.toBeVisible();
    const details = container.querySelector("details");
    expect(details).not.toBeNull();
    details!.open = true;
    expect(screen.getByText("Search workspace")).toBeVisible();
  });
});
