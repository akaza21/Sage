import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem } from "../../../domain/acp";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { TurnActivityItem } from "./TurnActivityItem";
import { page as chatPage } from "./acpProjection.fixture";

afterEach(() => {
  cleanup();
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

it("renders context compaction as a compact shared notice", () => {
  const { container } = render(
    <TurnActivityItem item={item("compactionNotice", {})} />,
  );
  expect(screen.getByText("Context automatically compacted")).toBeVisible();
  expect(
    container.querySelector('[data-activity-kind="compactionNotice"]'),
  ).not.toBeNull();
  expect(screen.queryByRole("button", { name: /tool/i })).toBeNull();
});

it.each([
  ["started", "Compacting context…"],
  ["streaming", "Compacting context…"],
  ["completed", "Context automatically compacted"],
  ["failed", "Context compaction failed"],
  ["cancelled", "Context compaction cancelled"],
] as const)("renders shared %s compaction copy", (lifecycle, copy) => {
  render(
    <TurnActivityItem item={{ ...item("compactionNotice", {}), lifecycle }} />,
  );
  expect(screen.getByRole("status").textContent).toBe(copy);
});

it("does not invent compaction outcome copy for an unsupported lifecycle", () => {
  const { container } = render(
    <TurnActivityItem
      item={{ ...item("compactionNotice", {}), lifecycle: "unsupported" }}
    />,
  );
  expect(screen.queryByRole("status")).toBeNull();
  expect(
    container.querySelector('[data-activity-kind="compactionNotice"]'),
  ).toBeNull();
});

describe("TurnActivityItem", () => {
  it.each([
    ["browserMcpUnavailable", "Browser tools unavailable"],
    ["clientFileWriteRefused", "File write unavailable"],
    ["pi_retry", "Agent notice"],
    ["futureUnknownNotice42", "Agent notice"],
  ])(
    "gives notice %s a human heading without losing its message",
    (code, heading) => {
      render(
        <TurnActivityItem
          item={item("error", {
            code,
            message: "You can continue chatting.",
            recoverable: false,
          })}
        />,
      );
      const notice = screen.getByRole("alert");
      expect(within(notice).getByText(heading)).toBeVisible();
      expect(notice).toHaveTextContent("You can continue chatting.");
      expect(notice).not.toHaveTextContent(code);
    },
  );

  it("shows a live Thinking shimmer without rendering reasoning text", () => {
    const thought = item("thoughtSummary", { summary: "" });
    thought.lifecycle = "streaming";
    thought.terminalAt = null;
    const { container, rerender } = render(<TurnActivityItem item={thought} />);
    expect(screen.getByText("Thinking")).toBeVisible();
    expect(container.querySelector("details")).toBeNull();

    rerender(
      <TurnActivityItem
        item={{
          ...thought,
          payload: { summary: `Earlier line\n**${"x".repeat(300)}**\n\n` },
        }}
      />,
    );
    expect(screen.getByText("Thinking")).toHaveClass("acp-live-shimmer");
    expect(screen.queryByText("x".repeat(256))).toBeNull();
    expect(screen.queryByText("Earlier line")).toBeNull();
    expect(container.querySelector(".acp-markdown")).toBeNull();
    expect(
      container.querySelectorAll('[data-activity-kind="thought"]'),
    ).toHaveLength(1);

    rerender(
      <TurnActivityItem
        item={{
          ...thought,
          payload: {
            summary: "Earlier line\n**Review** `src/main.ts` and *tests*\n\n",
          },
        }}
      />,
    );
    expect(screen.getByText("Thinking")).toBeVisible();
    expect(screen.queryByText("Review src/main.ts and tests")).toBeNull();
    expect(screen.queryByText("x".repeat(256))).toBeNull();
  });

  it.each([
    ["### **Plan**", "Plan"],
    ["- *item*", "item"],
    ["1. `step`", "step"],
    ["2) next step", "next step"],
    ["> quote", "quote"],
    ["### Plan\n```ts\n\n", "Plan"],
    ["```ts\nconst value = 1;\n```", "const value = 1;"],
    ["```ts\n```\n\n", "Thinking"],
  ])("keeps the live thinking label stable for %j", (summary) => {
    const thought = item("thoughtSummary", { summary });
    thought.lifecycle = "streaming";
    thought.terminalAt = null;
    render(<TurnActivityItem item={thought} />);
    expect(screen.getByText("Thinking")).toHaveClass(
      "acp-thought",
      "acp-live-shimmer",
    );
  });

  it.each(["completed", "failed", "cancelled", "unsupported"] as const)(
    "removes thinking when the item is %s",
    (lifecycle) => {
      const thought = item("thoughtSummary", {
        summary: "Checking the files.",
      });
      thought.lifecycle = "streaming";
      const { container, rerender } = render(
        <TurnActivityItem item={thought} />,
      );
      expect(screen.getByText("Thinking")).toBeVisible();
      rerender(<TurnActivityItem item={{ ...thought, lifecycle }} />);
      expect(container).toBeEmptyDOMElement();
    },
  );

  it.each(["codex", "claude-code", "pi"])(
    "shows normalized %s reasoning only while the turn is live",
    (bindingId) => {
      const page = chatPage("running");
      page.sessions = page.sessions.map((session) => ({
        ...session,
        bindingId: bindingId as never,
      }));
      page.turns[0].items = [
        {
          ...item("thoughtSummary", { summary: "Reasoning in progress" }),
          lifecycle: "streaming",
          terminalAt: null,
        },
      ];
      const props = {
        hasEarlier: false,
        loadingEarlier: false,
        historyError: null,
        projectRoots: [],
        retentionIncomplete: false,
      };
      const { rerender } = render(
        <AcpChatPresentation page={page} {...props} />,
      );
      expect(screen.getByText("Thinking")).toHaveClass("acp-live-shimmer");
      const completed = {
        ...page,
        turns: [
          { ...page.turns[0], state: "completed" as const, terminalAt: 20 },
        ],
      };
      rerender(<AcpChatPresentation page={completed} {...props} />);
      const disclosure = document.querySelector(
        ".chat-transcript-work > summary",
      );
      if (disclosure) fireEvent.click(disclosure);
      expect(screen.queryByText("Thinking")).toBeNull();
      expect(screen.queryByText("Reasoning in progress")).toBeNull();
    },
  );

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

  it("does not revive completed thinking while a tool waits for approval", () => {
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
    expect(container).toBeEmptyDOMElement();
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

  it("does not duplicate working tools under live thinking", () => {
    const thought = item("thoughtSummary", { summary: "Checking the files." });
    thought.lifecycle = "streaming";
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
    expect(screen.queryByText("Search workspace")).toBeNull();
    expect(container.querySelector("details")).toBeNull();
  });
});
