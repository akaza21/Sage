import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem } from "../../../domain/acp";
import {
  resetAgentRailViewPrefs,
  syncAgentRailClock,
} from "../../../workbench/agentRailViewPrefs";
import { AcpChatPresentation } from "./AcpChatPresentation";
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
  it("shows a streaming reasoning item as a muted Thinking row", () => {
    const thought = item("thoughtSummary", { summary: "Checking the files." });
    thought.lifecycle = "streaming";
    thought.terminalAt = null;
    const { container } = render(
      <TurnActivityItem item={thought} nowMs={4_000} />,
    );
    const details = container.querySelector(".acp-thought");
    expect(details?.querySelector("summary")).toHaveTextContent("Thinking");
    expect(details?.querySelector(".acp-thought__label")).toHaveClass(
      "acp-live-shimmer",
    );
    expect(details?.querySelector("summary .acp-state")).toBeNull();
    expect(details).toHaveAttribute("data-activity-state", "inProgress");
  });

  it("names a sub-second live Thought as Thinking briefly", () => {
    const thought = item("thoughtSummary", { summary: "Checking the files." });
    thought.createdAt = 1_000;
    thought.lifecycle = "streaming";
    thought.terminalAt = null;
    render(<TurnActivityItem item={thought} nowMs={1_500} />);

    expect(screen.getByText("Thinking").closest("summary")).toHaveAttribute(
      "aria-label",
      "Thinking briefly",
    );
  });

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
    expect(screen.getByText("Thinking").closest("summary")).toHaveAttribute(
      "aria-label",
      "Thinking for 3s",
    );
    rerender(<TurnActivityItem item={streaming} nowMs={5_000} />);
    expect(screen.getByText("Thinking").closest("summary")).toHaveAttribute(
      "aria-label",
      "Thinking for 4s",
    );

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
    expect(screen.getByText("Thinking").closest("summary")).toHaveAttribute(
      "aria-label",
      "Thinking for 3s",
    );

    act(() => syncAgentRailClock(5_000));
    expect(screen.getByText("Thinking").closest("summary")).toHaveAttribute(
      "aria-label",
      "Thinking for 4s",
    );

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

  it("keeps Thought active while a tool waits for approval", () => {
    const thought = item("thoughtSummary", { summary: "Waiting on approval." });
    const waiting = {
      ...item("error", {
        code: "unused",
        message: "unused",
        recoverable: false,
      }),
      id: "tool-waiting",
      kind: "toolCall" as const,
      lifecycle: "streaming" as const,
      terminalAt: null,
      toolState: "waitingForApproval" as const,
      payload: {
        presentation: {
          kind: "other" as const,
          rawKind: null,
          label: "Run tool",
          summary: null,
        },
        inputSummary: null,
      },
    } as unknown as Extract<AcpItem, { kind: "toolCall" }>;
    const { container } = render(
      <TurnActivityItem item={thought} workingItems={[waiting]} />,
    );
    const details = container.querySelector("details");
    expect(details).toHaveAttribute("data-activity-state", "inProgress");
    expect(within(details!).getByText("Thinking")).toBeVisible();
    expect(within(details!).queryByText("In progress")).toBeNull();
    expect(within(details!).queryByLabelText("Working tools")).toBeNull();
  });

  it("shows bounded current work only under the in-progress plan entry", () => {
    const plan = item("plan", {
      entries: [
        { label: "Inspect", state: "completed" },
        { label: "Implement", state: "inProgress" },
      ],
    });
    const workingItems = Array.from({ length: 6 }, (_, index) => ({
      ...item("error", {
        code: `unused-${index}`,
        message: "unused",
        recoverable: false,
      }),
      id: `tool-working-${index}`,
      kind: "toolCall" as const,
      lifecycle: "streaming" as const,
      terminalAt: null,
      toolState:
        index < 4
          ? ("inProgress" as const)
          : index === 4
            ? ("pending" as const)
            : ("waitingForApproval" as const),
      payload: {
        presentation: {
          kind: "other" as const,
          rawKind: null,
          label: `Run tool ${index + 1}`,
          summary: null,
        },
        inputSummary: index === 0 ? null : `Doing work ${index + 1}`,
      },
    })) as unknown as Extract<AcpItem, { kind: "toolCall" }>[];

    const { container } = render(
      <TurnActivityItem item={plan} workingItems={workingItems} />,
    );
    const region = screen.getByRole("region", { name: "Agent plan" });
    expect(within(region).getByText("Run tool 1")).toBeVisible();
    expect(within(region).getAllByText("In progress").length).toBeGreaterThan(
      1,
    );
    expect(within(region).getByText("Run tool 3")).toBeVisible();
    expect(within(region).queryByText("Run tool 4")).toBeNull();
    expect(within(region).queryByText("Run tool 5")).toBeNull();
    expect(within(region).queryByText("Run tool 6")).toBeNull();
    expect(
      within(region).getByText("1 more activity item running"),
    ).toBeVisible();
    const completed = within(region).getByText("Inspect").closest("li");
    expect(completed).not.toContainElement(
      within(region).getByText("Run tool 1"),
    );
    expect(
      container.querySelector('[aria-live]:not([aria-live="off"])'),
    ).toBeNull();
  });

  it("renders no plan detail when no tool is running and preserves collapse state", () => {
    const plan = item("plan", {
      entries: [{ label: "Implement", state: "inProgress" }],
    });
    const page = {
      projectId: "project-1",
      chatId: "chat-1",
      title: "Plan detail",
      projectionRevision: "1",
      currentSessionId: "session-1",
      sessions: [],
      turns: [
        {
          id: "turn-1",
          sessionId: "session-1",
          runtimeEpoch: "1",
          ordinal: 1,
          state: "running",
          revision: "1",
          createdAt: 1,
          updatedAt: 2,
          terminalAt: null,
          items: [plan],
          activities: [],
        },
      ],
      nextCursor: null,
    } as unknown as AcpChatPage;
    const { rerender } = render(
      <AcpChatPresentation
        page={page}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
      />,
    );
    const region = screen.getByRole("region", { name: "Agent plan" });
    expect(within(region).queryByLabelText("Current activity")).toBeNull();

    const disclosure = within(region).getByRole("button", { name: /plan/i });
    fireEvent.click(disclosure);
    const working = {
      ...item("error", {
        code: "unused",
        message: "unused",
        recoverable: false,
      }),
      id: "tool-working",
      kind: "toolCall" as const,
      lifecycle: "streaming" as const,
      terminalAt: null,
      toolState: "inProgress" as const,
      payload: {
        presentation: {
          kind: "other" as const,
          rawKind: null,
          label: "Run tool",
          summary: null,
        },
        inputSummary: "Checking current state",
      },
    } as unknown as Extract<AcpItem, { kind: "toolCall" }>;
    const nextPage = {
      ...page,
      projectionRevision: "2" as never,
      turns: [
        {
          ...page.turns[0],
          revision: "2" as never,
          updatedAt: 3,
          items: [plan, working],
        },
      ],
    } as AcpChatPage;
    rerender(
      <AcpChatPresentation
        page={nextPage}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
      />,
    );
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(within(region).queryByLabelText("Current activity")).toBeNull();
    fireEvent.click(disclosure);
    expect(within(region).getByText("Run tool")).toBeVisible();
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

    expect(screen.getByText("Thinking")).toBeVisible();
    expect(screen.queryByText("In progress")).toBeNull();
    expect(screen.getByText("Search workspace")).not.toBeVisible();
    const details = container.querySelector("details");
    expect(details).not.toBeNull();
    details!.open = true;
    expect(screen.getByText("Search workspace")).toBeVisible();
  });
});
