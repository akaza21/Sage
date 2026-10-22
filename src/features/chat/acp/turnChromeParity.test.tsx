import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { AcpItem } from "../../../domain/acp";
import { ChatTranscriptFooter } from "../ChatTranscriptFooter";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { TurnActivityItem } from "./TurnActivityItem";
import { page, textItem } from "./acpProjection.fixture";
import { parityTool } from "./activityParity.fixture";
import { CompactToolItem } from "./CompactToolItem";

afterEach(cleanup);

it.each([
  ["cancelled", { kind: "cancelledByUser" }, "Cancelled by you"],
  ["interrupted", undefined, "Interrupted"],
] as const)(
  "places the %s notice after the stopped answer, never above the prompt",
  (state, reason, label) => {
    const projection = page(state, reason);
    projection.turns[0].items = [
      {
        ...textItem(),
        id: "prompt" as AcpItem["id"],
        ordinal: 1,
        payload: { role: "user", text: "User prompt" },
      },
      { ...textItem(), ordinal: 2, lifecycle: "cancelled" },
    ];
    render(
      <AcpChatPresentation
        page={projection}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
      />,
    );
    const notice = screen.getByRole("status", { name: label });
    const answer = screen.getByText("Normalized answer");
    expect(
      answer.compareDocumentPosition(notice) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      screen.getByText("User prompt").compareDocumentPosition(answer) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  },
);

it.each(["started", "completed", "failed", "cancelled"] as const)(
  "renders %s compaction as an icon-led activity row with its outcome",
  (lifecycle) => {
    const notice = {
      ...textItem(),
      kind: "compactionNotice",
      lifecycle,
      payload: {},
    } as AcpItem & { kind: "compactionNotice" };
    const { container } = render(<TurnActivityItem item={notice} />);
    const row = screen.getByRole("status");
    expect(row.firstElementChild?.tagName.toLowerCase()).toBe("svg");
    expect(row).toHaveAttribute("data-activity-state", lifecycle);
    expect(container.querySelector(".acp-compaction-notice")).toHaveClass(
      "acp-activity-row",
    );
  },
);

it("keeps a simple completed answer's Worked duration without an empty disclosure", () => {
  const projection = page("completed");
  projection.turns[0].createdAt = 1_000;
  projection.turns[0].terminalAt = 6_000;
  const { container } = render(
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(screen.getByText("Worked for 5s")).toBeVisible();
  expect(container.querySelector("details.chat-transcript-work")).toBeNull();
});

it.each(["completed"] as const)(
  "omits orphan retry-only actions for a %s tool-only turn",
  (state) => {
    const projection = page(state);
    projection.turns[0].items = [parityTool()];
    const { container } = render(
      <ChatTranscriptFooter
        useNativeAcp
        page={projection}
        messages={[]}
        onRetry={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(container.querySelector(".chat-transcript-actions")).toBeNull();
  },
);

it.each(["cancelled", "interrupted"] as const)(
  "keeps Retry with the %s notice before any prose",
  (state) => {
    const projection = page(state);
    projection.turns[0].items = [parityTool()];
    const retry = vi.fn();
    render(
      <AcpChatPresentation
        page={projection}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
        onRetry={retry}
      />,
    );
    const button = screen.getByRole("button", { name: "Retry" });
    expect(button.previousElementSibling?.className).toBe("acp-turn__header");
    fireEvent.click(button);
    expect(retry).toHaveBeenCalledOnce();
  },
);

it("opens only the latest compact rollup when Worked is opened", () => {
  const projection = page("completed");
  projection.turns[0].items = [
    parityTool("read", 1),
    { ...textItem(), id: "middle" as AcpItem["id"], ordinal: 3 },
    { ...parityTool("execute", 2), ordinal: 4 },
    { ...textItem(), ordinal: 5 },
  ];
  const { container } = render(
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  const work = container.querySelector<HTMLDetailsElement>(
    ".chat-transcript-work",
  )!;
  work.open = true;
  fireEvent(work, new Event("toggle"));
  const rows =
    container.querySelectorAll<HTMLDetailsElement>(".acp-tool-rollup");
  expect([...rows].map((row) => row.open)).toEqual([false, true]);
  rows[0].open = true;
  fireEvent(rows[0], new Event("toggle"));
  expect(rows[0].open).toBe(true);
});

it.each(["completed", "failed"] as const)(
  "puts the %s tool outcome after output in the card footer",
  (state) => {
    const { container } = render(
      <CompactToolItem item={parityTool("execute", 1, state)} />,
    );
    const frame = container.querySelector(".acp-tool-command-frame")!;
    expect(frame.lastElementChild).toHaveAttribute("role", "status");
    expect(frame.lastElementChild).toHaveTextContent(
      state === "completed" ? "Completed" : "Failed",
    );
  },
);
