import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { readFile } from "node:fs/promises";
import type { AcpItem, ToolPresentation } from "../../../domain/acp";
import { ChatTranscriptFooter } from "../ChatTranscriptFooter";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { TurnActivityItem } from "./TurnActivityItem";
import { page, textItem } from "./acpProjection.fixture";
import { parityTool } from "./activityParity.fixture";
import { CompactToolItem } from "./CompactToolItem";
import { ChatTranscriptTurnSummary } from "../ChatTranscriptFooter";
import { ToolActivityRollup } from "./ToolActivityRollupView";

afterEach(cleanup);

it("removes dead partial chrome and reveals rollup chevrons only on interaction", async () => {
  const css = await readFile("src/features/chat/acp/acp-chat.css", "utf8");
  const detailCss = await readFile(
    "src/features/chat/acp/acp-activity-details.css",
    "utf8",
  );
  const polishCss = await readFile(
    "src/features/chat/chat-transcript-polish.css",
    "utf8",
  );
  expect(css).not.toContain(".acp-message__partial");
  expect(detailCss).toMatch(
    /\.acp-tool-rollup__chevron\s*\{[^}]*opacity:\s*0/s,
  );
  expect(polishCss).toMatch(
    /\.chat-transcript-work__groups \.acp-tool-rollup__chevron\s*\{[^}]*opacity:\s*0/s,
  );
  expect(polishCss).toMatch(
    /\.acp-tool-rollup\[open\]\s*> summary\s+\.acp-tool-rollup__chevron/,
  );
});

it("opens Worked and its latest read details in one commit without replacing visible rows", () => {
  const projection = page("completed");
  projection.turns[0].items = [parityTool("read"), textItem()];
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
  const worked = container.querySelector<HTMLDetailsElement>(
    "details.chat-transcript-work",
  )!;
  const read = container.querySelector<HTMLDetailsElement>(".acp-tool-rollup")!;
  fireEvent.click(worked.querySelector("summary")!);
  expect(worked).toHaveAttribute("open");
  expect(container.querySelector(".acp-tool-rollup")).toBe(read);
  expect(read).toHaveAttribute("open");
  expect(screen.getByRole("group", { name: "read" })).toBeVisible();
  expect(screen.getByText("fixture output")).toBeVisible();
  fireEvent.click(worked.querySelector("summary")!);
  expect(container.querySelector(".acp-tool-rollup")).toBe(read);
  expect(read).not.toHaveAttribute("open");
  fireEvent.click(worked.querySelector("summary")!);
  expect(container.querySelector(".acp-tool-rollup")).toBe(read);
  expect(screen.getByText("fixture output")).toBeVisible();
});

it.each<ToolPresentation>([
  {
    kind: "search",
    label: "Search files",
    querySummary: "needle",
    resources: [],
    matchCount: 1,
    truncated: false,
  },
  {
    kind: "fetch",
    label: "Fetch page",
    url: "https://example.com",
    summary: "Page fetched",
  },
  {
    kind: "mcp",
    label: "Inspect",
    serverLabel: "Fixture",
    toolLabel: "Inspect",
    argumentsSummary: null,
    resultSummary: "Inspected",
  },
])(
  "opens the latest $kind rollup synchronously, with or without an earlier compact row",
  (presentation) => {
    for (const withRead of [false, true]) {
      const projection = page("completed");
      const latestTool = parityTool("read", 2);
      latestTool.payload.presentation = presentation;
      projection.turns[0].items = [
        ...(withRead ? [parityTool("read")] : []),
        latestTool,
        textItem(),
      ];
      const { container, unmount } = render(
        <AcpChatPresentation
          page={projection}
          hasEarlier={false}
          loadingEarlier={false}
          historyError={null}
          projectRoots={[]}
          retentionIncomplete={false}
        />,
      );
      const rows = [
        ...container.querySelectorAll<HTMLDetailsElement>(".acp-tool-rollup"),
      ];
      const latest = rows.at(-1)!;
      fireEvent.click(
        container.querySelector(".chat-transcript-work > summary")!,
      );
      expect(latest).toHaveAttribute("open");
      expect(latest.querySelector('[data-acp-item-id="tool-2"]')).toBeVisible();
      if (withRead) expect(rows[0]).not.toHaveAttribute("open");
      expect(
        container.querySelectorAll(".acp-tool-rollup")[rows.length - 1],
      ).toBe(latest);
      unmount();
    }
  },
);

it("keeps no padded activity group for a closed browser-only Worked", () => {
  const projection = page("completed");
  projection.turns[0].items = [textItem()];
  projection.turns[0].browserOperationsCollapsed = 1;
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
  expect(container.querySelector(".chat-transcript-work__groups")).toBeNull();
});

it("keeps one leading spinner when a read group mixes running and failed calls", () => {
  const { container } = render(
    <ToolActivityRollup
      interactive
      rollup={{
        kind: "read",
        commandActivity: "ordinary",
        items: [
          parityTool("read", 1, "failed"),
          parityTool("read", 2, "inProgress"),
        ],
      }}
    />,
  );
  const lead = container.querySelector(".acp-tool-rollup__summary-copy")!;
  expect(lead.querySelectorAll("svg")).toHaveLength(1);
  expect(lead.querySelector(".acp-tool-running")).not.toBeNull();
});

it("never auto-opens a completed turn for an unresolved retained permission", () => {
  const projection = page("completed");
  projection.turns[0].items = [
    {
      ...parityTool(),
      kind: "permissionRequest",
      toolState: "waitingForApproval",
      payload: { state: "received" },
    } as unknown as AcpItem,
  ];
  const { container } = render(
    <ChatTranscriptTurnSummary turn={projection.turns[0]} hasActivities>
      <p>Retained tool work</p>
    </ChatTranscriptTurnSummary>,
  );
  expect(container.querySelector("details")).not.toHaveAttribute("open");
});

it("keeps a completed Ran row visible and collapsed while Worked is closed", () => {
  const projection = page("completed");
  projection.turns[0].items = [parityTool(), textItem()];
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
  expect(container.querySelector(".chat-transcript-work")).not.toHaveAttribute(
    "open",
  );
  const row = screen.getByText("Ran git status --short");
  expect(row).toBeVisible();
  const disclosure = row.closest("details")!;
  expect(disclosure).not.toHaveAttribute("open");
  expect(screen.queryByText("Shell")).toBeNull();
  disclosure.open = true;
  fireEvent(disclosure, new Event("toggle"));
  expect(screen.getByText("Shell")).toBeVisible();
  expect(container.querySelector(".chat-transcript-work")).not.toHaveAttribute(
    "open",
  );
});

it("adds no empty tool row group beside a closed Worked without tool calls", () => {
  const projection = page("completed");
  const prose = { ...textItem(), id: "item-prose" } as AcpItem;
  if (prose.kind === "text") prose.payload.text = "Checked the preview first.";
  projection.turns[0].items = [prose, textItem()];
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
  const worked = container.querySelector("details.chat-transcript-work");
  expect(worked).not.toBeNull();
  expect(worked).not.toHaveAttribute("open");
  expect(container.querySelector(".chat-transcript-work__groups")).toBeNull();
});

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
    expect(screen.queryByText("Partial answer retained")).toBeNull();
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
  expect(work.open).toBe(false);
  expect(container.querySelector(".acp-tool-command-frame")).toBeNull();
  work.open = true;
  fireEvent(work, new Event("toggle"));
  const rows =
    container.querySelectorAll<HTMLDetailsElement>(".acp-tool-rollup");
  expect([...rows].map((row) => row.open)).toEqual([false, true]);
  rows[0].open = true;
  fireEvent(rows[0], new Event("toggle"));
  expect(rows[0].open).toBe(true);
});

it.each(["execute", "read", "edit"] as const)(
  "uses only the spinner as the leading glyph for a running %s row",
  (kind) => {
    const projection = page("running");
    projection.turns[0].items = [parityTool(kind, 1, "inProgress")];
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
    const lead = container.querySelector(".acp-tool-rollup__summary-copy")!;
    expect(lead.querySelectorAll("svg")).toHaveLength(1);
    expect(lead.querySelector(".acp-tool-running")).not.toBeNull();
  },
);

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

it("lists individually collapsed Ran command rows inside expanded Worked", () => {
  const projection = page("completed");
  projection.turns[0].items = [
    parityTool(),
    parityTool("execute", 2),
    textItem(),
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
  const worked = container.querySelector<HTMLDetailsElement>(
    ".chat-transcript-work",
  )!;
  worked.open = true;
  fireEvent(worked, new Event("toggle"));
  const commands = container.querySelectorAll<HTMLDetailsElement>(
    ".acp-tool-rollup__items > details",
  );
  expect(commands).toHaveLength(2);
  for (const command of commands) {
    expect(command.open).toBe(false);
    expect(command.querySelector("summary")).toHaveTextContent(
      "Ran git status --short",
    );
  }
  expect(container.querySelectorAll(".acp-tool-command-frame")).toHaveLength(0);
  commands[0].open = true;
  fireEvent(commands[0], new Event("toggle"));
  expect(container.querySelectorAll(".acp-tool-command-frame")).toHaveLength(1);
});
