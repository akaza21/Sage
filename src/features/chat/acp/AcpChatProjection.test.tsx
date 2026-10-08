import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  AcpChatPage,
  AcpItem,
  HarnessActivityProjection,
  SagePermissionRequestId,
} from "../../../domain/acp";
import {
  browseSessionForOwner,
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "../../preview/harnessBrowse";
import type { HarnessBrowseOwner } from "../../preview/harnessBrowse";
import { MAX_AGENT_EDIT_SUMMARY_FILES } from "./agentEditSummary";
import { AcpChatProjection } from "./AcpChatProjection";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { page, textItem } from "./acpProjection.fixture";
import { settleLazyModules } from "../../../test/settleLazyModules";
import createdFileEvent from "./createdFileEvent.fixture.json";
import { browserOperationFixture } from "../../../domain/browserOperation.fixture";
import { ChatTranscriptTurnSummary } from "../ChatTranscriptFooter";

async function renderProjection(ui: Parameters<typeof render>[0]) {
  const result = render(ui);
  await settleLazyModules();
  return result;
}

const browseOwner = {
  projectId: "project-1",
  projectGeneration: "1",
  transitionEpoch: 1,
  chatId: "chat-1",
  sessionId: "session-1",
  runtimeEpoch: "2",
} as HarnessBrowseOwner;

beforeEach(() => setHarnessBrowseOwner(browseOwner, [browseOwner.chatId]));

afterEach(() => {
  cleanup();
  clearBrowseSessions();
});

it("renders durable browser actions separately from harness tool rows", async () => {
  const base = page("completed");
  const browserPage: AcpChatPage = {
    ...base,
    turns: [
      {
        ...base.turns[0],
        browserOperations: [browserOperationFixture],
        browserOperationsCollapsed: 3,
      },
    ],
  };
  await renderProjection(<AcpChatProjection page={browserPage} active />);
  const browserActions = screen.getByRole("region", {
    name: "Browser actions",
  });
  expect(browserActions.closest(".chat-transcript-work")).toBeInTheDocument();
  fireEvent.click(screen.getByText(/^Worked for /));
  expect(browserActions).toBeVisible();
  expect(screen.getByText("+3 earlier browser actions")).toBeInTheDocument();
});

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

function editItem(
  id: string,
  ordinal: number,
  rootId: string,
  summary: string,
  overrides: Partial<AcpItem> = {},
  additions: number | null = null,
  deletions: number | null = null,
): AcpItem {
  return {
    id,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "2",
    ordinal,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: ordinal,
    updatedAt: ordinal,
    terminalAt: ordinal,
    kind: "fileChangeDiff",
    toolState: null,
    payload: {
      changes: [
        {
          resource: {
            kind: "resource",
            resource: { rootId, relativePath: "src/main.ts" },
          },
          operation: "updated",
          originalResource: null,
          summary,
          additions,
          deletions,
          pairState: "available",
          snapshotRevision: "1",
          oldBytes: 4,
          newBytes: 4,
          contentDigest: "a".repeat(64),
        },
      ],
      truncated: false,
    },
    ...overrides,
  } as AcpItem;
}

describe("AcpChatProjection", () => {
  it("renders the Codex added-file projection as a created file card", async () => {
    const projection = page("completed");
    projection.sessions = [{ id: "session-1", ordinal: 0 } as never];
    const file = editItem("file-added", 1, "fixture-root", "unused");
    if (file.kind !== "fileChangeDiff") throw new Error("fixture");
    file.payload.changes = [
      {
        ...file.payload.changes[0],
        resource: { kind: "resource", resource: createdFileEvent.resource },
        operation: createdFileEvent.expected.operation as "created",
        summary: createdFileEvent.expected.summary,
        additions: null,
        deletions: null,
        pairState: "unavailable",
        snapshotRevision: "0",
        oldBytes: null,
        newBytes: null,
        contentDigest: null,
      },
    ];
    projection.turns[0].items = [file];

    await renderProjection(<AcpChatProjection page={projection} active />);

    const card = screen.getByRole("region", { name: "Files edited in turn 1" });
    fireEvent.click(within(card).getByRole("button", { name: /file changed/ }));
    expect(
      within(card).getByText(createdFileEvent.expected.summary),
    ).toBeVisible();
    expect(within(card).getByText("new.ts")).toBeVisible();
    expect(within(card).queryByText(/Path does not exist/)).toBeNull();
  });

  it("opens a Codex summary-only created file from its changed-file card", async () => {
    const projection = page("completed");
    projection.sessions = [{ id: "session-1", ordinal: 0 } as never];
    const file = editItem("file-added", 1, "fixture-root", "unused");
    if (file.kind !== "fileChangeDiff") throw new Error("fixture");
    file.payload.changes = [
      {
        ...file.payload.changes[0],
        resource: { kind: "resource", resource: createdFileEvent.resource },
        operation: "created",
        pairState: "unavailable",
        snapshotRevision: "0",
        oldBytes: null,
        newBytes: null,
        contentDigest: null,
      },
    ];
    projection.turns[0].items = [file];
    const openResource = vi.fn();
    await renderProjection(
      <AcpChatProjection
        page={projection}
        active
        projectRoots={[{ id: "fixture-root", name: "Project", ordinal: 0 }]}
        onOpenResource={openResource}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /file changed/ }));

    fireEvent.click(
      screen.getByRole("button", { name: "Open file Project / src/new.ts" }),
    );
    expect(openResource).toHaveBeenCalledWith(
      createdFileEvent.resource,
      "file",
      { projectId: "project-1", chatId: "chat-1", sessionId: "session-1" },
    );
  });
  it("badges an assistant message with the Sage avatar beside its role label", async () => {
    await renderProjection(
      <AcpChatProjection page={page("completed")} active />,
    );

    const message = screen
      .getByText("Normalized answer")
      .closest(".acp-message");
    const avatar = message?.querySelector(".acp-message__avatar");

    expect(avatar).not.toBeNull();
    expect(avatar?.getAttribute("aria-hidden")).toBe("true");
    expect(avatar?.textContent).toBe("S");
  });

  it("leaves a user message without an empty avatar placeholder", async () => {
    await renderProjection(
      <AcpChatProjection page={page("completed")} active />,
    );

    const messages = document.querySelectorAll(".acp-message--user");

    for (const message of messages) {
      expect(message.querySelector(".acp-message__avatar")).toBeNull();
    }
  });

  it("keeps reported usage out of the transcript", () => {
    const base = page("running");
    const next = {
      ...base,
      turns: [
        {
          ...base.turns[0],
          usage: {
            inputTokens: 1_234,
            outputTokens: 0,
            totalTokens: 1_234,
          },
          items: [textItem()],
        },
      ],
    } as AcpChatPage;

    render(<AcpChatProjection page={next} active />);

    expect(screen.queryByLabelText("Usage")).toBeNull();
    expect(screen.queryByText("1,234 tokens")).toBeNull();
  });

  it("keeps retained Turn status accessible without replaying it", async () => {
    render(<AcpChatProjection page={page("waitingUser")} active />);
    const status = await screen.findByRole("status", {
      name: "Waiting for user",
    });
    expect(status).toHaveTextContent("Waiting for user");
    expect(status).toHaveAttribute("aria-live", "off");
    expect(status).toHaveAttribute("aria-atomic", "true");
  });

  it("renders lifecycle state and normalized items", () => {
    const { container } = render(
      <AcpChatProjection page={page("waitingModel")} active />,
    );
    expect(
      screen.getByRole("status", { name: "Waiting for model" }),
    ).toHaveTextContent("Waiting for model");
    expect(screen.getByText("Normalized answer")).toBeInTheDocument();
    expect(screen.getByText("09:05", { selector: "time" })).toHaveAttribute(
      "dateTime",
      "2026-08-31T09:05:00.000Z",
    );
    expect(container.querySelector(".acp-chat-projection")).toMatchObject({
      dataset: {
        projectId: "project-1",
        chatId: "chat-1",
        currentSessionId: "session-1",
      },
    });
    expect(container.querySelector(".acp-turn")).toMatchObject({
      dataset: {
        turnId: "turn-1",
        sessionId: "session-1",
        runtimeEpoch: "2",
        turnOrdinal: "1",
      },
    });
    expect(container.querySelector(".acp-message--assistant")).toMatchObject({
      dataset: { itemId: "item-text", messageRole: "assistant" },
    });
  });

  it("keeps a date boundary above the user bubble rather than between the bubble and Work", async () => {
    const next = page("completed");
    next.turns[0].items = [
      {
        ...textItem(),
        id: "user-prompt" as AcpItem["id"],
        payload: { role: "user", text: "Please run the tests" },
      },
      activityItem("test-command", "command-activity", {
        origin: "primary",
        activityId: null,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: 0,
            outputTail: "all tests passed",
            outputTruncated: false,
            label: "Run tests",
            summary: "npm test",
            resources: [],
            activity: "declaredTest",
          },
          inputSummary: null,
        },
      }),
      textItem(),
      editItem("change", 3, "root-one", "updated"),
    ];

    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );

    const turn = container.querySelector<HTMLElement>(".acp-turn");
    const prompt = turn?.querySelector(".acp-message--user");
    const day = turn?.querySelector(".chat-transcript-day");
    const work = turn?.querySelector<HTMLDetailsElement>(
      ".chat-transcript-work",
    );
    const answer = turn?.querySelector(".acp-message--assistant");
    const changedFiles = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    const rollup = work?.querySelector<HTMLDetailsElement>(".acp-tool-rollup");

    expect(day).toBeNull();
    expect(turn?.previousElementSibling).toHaveClass("chat-transcript-day");
    expect(prompt).not.toBeNull();
    expect(work).not.toBeNull();
    expect(answer).not.toBeNull();
    expect(changedFiles).not.toBeNull();
    expect(
      prompt!.compareDocumentPosition(work!) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      work!.compareDocumentPosition(answer!) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      answer!.compareDocumentPosition(changedFiles) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(container.querySelectorAll(".acp-tool-rollup")).toHaveLength(1);
    expect(turn?.querySelectorAll(".acp-message--user")).toHaveLength(1);
    expect(within(turn!).getAllByText("Ran tests")).toHaveLength(1);
    expect(work).not.toHaveAttribute("open");
    expect(rollup).not.toBeNull();
    expect(rollup).not.toHaveAttribute("open");

    fireEvent.click(work!.querySelector("summary")!);
    expect(work).toHaveAttribute("open");
    expect(within(work!).getByText("Ran tests")).toBeVisible();
    rollup!.open = true;
    fireEvent(rollup!, new Event("toggle"));
    expect(
      await within(rollup!).findByLabelText("Command line"),
    ).toHaveTextContent("npm test");
    fireEvent.click(work!.querySelector("summary")!);
    expect(work).not.toHaveAttribute("open");
  });

  it("shows day labels only when the calendar day changes between turns", async () => {
    const yesterday = new Date();
    yesterday.setDate(yesterday.getDate() - 1);
    yesterday.setHours(14, 8, 0, 0);
    const today = new Date();
    today.setHours(9, 5, 0, 0);
    const base = page("completed");
    const moments = [
      yesterday.getTime(),
      yesterday.getTime() + 60_000,
      today.getTime(),
    ];
    const turns = moments.map((createdAt, index) => ({
      ...base.turns[0],
      id: `turn-${index + 1}` as AcpChatPage["turns"][number]["id"],
      ordinal: index + 1,
      createdAt,
      items: [
        {
          ...textItem(),
          id: `prompt-${index + 1}` as AcpItem["id"],
          createdAt,
          payload: { role: "user" as const, text: `Prompt ${index + 1}` },
        },
      ],
    })) as AcpChatPage["turns"];
    const { container } = await renderProjection(
      <AcpChatProjection page={{ ...base, turns }} active />,
    );
    const renderedTurns = container.querySelectorAll(".acp-turn");
    expect(container.querySelectorAll(".chat-transcript-day")).toHaveLength(2);
    expect(renderedTurns[0]?.previousElementSibling).toHaveClass(
      "chat-transcript-day",
    );
    expect(renderedTurns[1]?.previousElementSibling).toBe(renderedTurns[0]);
    expect(renderedTurns[2]?.previousElementSibling).toHaveClass(
      "chat-transcript-day",
    );
    expect(renderedTurns[2]?.querySelector(".chat-transcript-day")).toBeNull();
  });

  it("shows a day label after local midnight without a projection update", () => {
    vi.useFakeTimers();
    try {
      const beforeMidnight = new Date(2026, 9, 2, 23, 59, 59);
      vi.setSystemTime(beforeMidnight);
      const chatPage = page("completed");
      const morning = new Date(2026, 9, 2, 9, 0).getTime();
      chatPage.turns[0].createdAt = morning;
      chatPage.turns[0].items = [];
      const { container, unmount } = render(
        <AcpChatPresentation
          page={chatPage}
          hasEarlier={false}
          loadingEarlier={false}
          historyError={null}
          projectRoots={[]}
          retentionIncomplete={false}
        />,
      );
      expect(container.querySelector(".chat-transcript-day")).toBeNull();
      act(() => vi.advanceTimersByTime(2_000));
      expect(container.querySelector(".chat-transcript-day")).not.toBeNull();
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("omits a date boundary for an unrepresentable turn timestamp", async () => {
    const base = page("completed");
    base.turns[0].createdAt = Number.MAX_VALUE;
    const { container } = await renderProjection(
      <AcpChatProjection page={base} active />,
    );
    expect(container.querySelector(".acp-turn")).not.toBeNull();
    expect(container.querySelector(".chat-transcript-day")).toBeNull();
  });

  it("keeps mixed tool details in ordinal order inside the Worked disclosure", async () => {
    const next = page("completed");
    next.turns[0].items = [
      activityItem("later-read", "activity", {
        origin: "primary",
        activityId: null,
        ordinal: 4,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "read",
            label: "Read source",
            resources: [
              {
                kind: "resource",
                resource: { rootId: "root-a", relativePath: "src/main.ts" },
              },
            ],
          },
          inputSummary: null,
        },
      }),
      textItem(),
      activityItem("earlier-command", "activity", {
        origin: "primary",
        activityId: null,
        ordinal: 2,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: 0,
            outputTail: "done",
            outputTruncated: false,
            label: "Run checks",
            summary: "npm run check",
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      }),
    ];

    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const work = container.querySelector(".chat-transcript-work");
    expect(work).not.toBeNull();
    expect(
      [...work!.querySelectorAll(".acp-tool-rollup__label")].map(
        (label) => label.textContent,
      ),
    ).toEqual(["Ran commands, read a file"]);
    const rollup =
      container.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    expect(rollup).not.toBeNull();
    rollup!.open = true;
    fireEvent(rollup!, new Event("toggle"));
    expect(
      [...rollup!.querySelectorAll(".acp-tool-card")].map((card) =>
        card.getAttribute("data-acp-item-id"),
      ),
    ).toEqual(["earlier-command", "later-read"]);
  });

  it("summarises adjacent mixed completed tools in one chronological row", async () => {
    const next = page("completed");
    const file = (
      id: string,
      ordinal: number,
      kind: "edit" | "read",
      path: string,
    ) =>
      activityItem(id, "activity", {
        origin: "primary",
        activityId: null,
        ordinal,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind,
            label: path,
            resources: [
              {
                kind: "resource",
                resource: { rootId: "root-a", relativePath: path },
              },
            ],
          },
          inputSummary: null,
        },
      });
    next.turns[0].items = [
      file("read-two", 3, "read", "src/b.ts"),
      file("edit", 1, "edit", "src/a.ts"),
      activityItem("command", "activity", {
        origin: "primary",
        activityId: null,
        ordinal: 4,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: 0,
            outputTail: null,
            outputTruncated: false,
            label: "Run command",
            summary: "npm test",
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      }),
      file("read-one", 2, "read", "src/a.ts"),
    ];

    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const work = container.querySelector(".chat-transcript-work");
    expect(
      [...work!.querySelectorAll(".acp-tool-rollup__label")].map((row) =>
        row.textContent?.trim(),
      ),
    ).toEqual(["Edited a file, read files, ran commands"]);
  });

  it("interleaves running assistant prose and activity rows in transcript order", async () => {
    const next = page("running");
    const prose = (id: string, ordinal: number, text: string) =>
      activityItem(id, "activity", {
        origin: "primary",
        activityId: null,
        ordinal,
        kind: "text",
        payload: { role: "assistant", text },
      });
    const command = (id: string, ordinal: number) =>
      activityItem(id, "activity", {
        origin: "primary",
        activityId: null,
        ordinal,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: 0,
            outputTail: null,
            outputTruncated: false,
            label: "Run command",
            summary: "gh --version",
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      });
    next.turns[0].items = [
      command("second-command", 4),
      prose("first-prose", 1, "First progress note"),
      prose("second-prose", 3, "Second progress note"),
      command("first-command", 2),
    ];

    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const work = container.querySelector(".chat-transcript-work");
    expect(work).not.toBeInstanceOf(HTMLDetailsElement);
    expect(work?.querySelector(":scope > summary")).toBeNull();
    expect(
      work?.querySelector(".chat-transcript-work__heading"),
    ).toHaveTextContent(/^Working for /);
    expect(
      [
        ...work!.querySelectorAll(
          ".acp-message--assistant .acp-markdown, .acp-tool-rollup__label",
        ),
      ].map((row) => row.textContent?.trim()),
    ).toEqual([
      "First progress note",
      "Ran commands",
      "Second progress note",
      "Ran commands",
    ]);
  });

  it("keeps an intervening error between running activity rows", async () => {
    const next = page("running");
    const command = (id: string, ordinal: number) =>
      activityItem(id, "activity", {
        origin: "primary",
        activityId: null,
        ordinal,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: 0,
            outputTail: null,
            outputTruncated: false,
            label: "Run command",
            summary: "gh --version",
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      });
    next.turns[0].items = [
      command("later-command", 3),
      activityItem("intervening-error", "activity", {
        origin: "primary",
        activityId: null,
        ordinal: 2,
        kind: "error",
        payload: {
          code: "TOOL_RETRY",
          message: "Retrying command",
          recoverable: true,
        },
      }),
      command("first-command", 1),
    ];

    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const work = container.querySelector(".chat-transcript-work");
    expect(
      [...work!.querySelectorAll(".acp-tool-rollup__label, .acp-error")].map(
        (row) => row.textContent?.trim(),
      ),
    ).toEqual([
      "Ran commands",
      "TOOL_RETRYRetry availableRetrying command",
      "Ran commands",
    ]);
  });

  it("keeps completed progress in Worked and the final answer visible", async () => {
    const next = page("completed");
    next.turns[0].items = [
      {
        ...textItem(),
        id: "final-answer" as AcpItem["id"],
        ordinal: 4,
        payload: { role: "assistant", text: "Final answer" },
      },
      activityItem("command", "activity", {
        origin: "primary",
        activityId: null,
        ordinal: 3,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: 0,
            outputTail: null,
            outputTruncated: false,
            label: "Run command",
            summary: "gh --version",
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      }),
      {
        ...textItem(),
        id: "progress-note" as AcpItem["id"],
        ordinal: 2,
        payload: { role: "assistant", text: "Progress note" },
      },
    ];

    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const work = container.querySelector(".chat-transcript-work");
    expect(work).not.toHaveAttribute("open");
    expect(work?.querySelector(".acp-markdown")).toHaveTextContent(
      "Progress note",
    );
    expect(work?.querySelector(".acp-tool-rollup__label")).toHaveTextContent(
      "Ran commands",
    );
    expect(work).not.toHaveTextContent("Final answer");
    expect(
      container.querySelector(".acp-turn__items > .acp-message--assistant"),
    ).toHaveTextContent("Final answer");
  });

  it.each([
    "completed",
    "interrupted",
    "failed",
    "cancelled",
    "deliveryUnknown",
  ] as const)(
    "keeps no terminal thinking rows after a %s turn",
    async (state) => {
      const next = page(state);
      next.turns[0].items = [
        {
          ...textItem(),
          id: "final-answer" as AcpItem["id"],
          ordinal: 3,
          lifecycle: "completed",
          payload: { role: "assistant", text: "Final answer" },
        },
        {
          ...textItem(),
          id: "closed-thought" as AcpItem["id"],
          ordinal: 2,
          kind: "thoughtSummary",
          lifecycle: "cancelled",
          terminalAt: 4_000,
          payload: { summary: "Checked the file changes" },
        } as AcpItem,
      ];

      const { container } = await renderProjection(
        <AcpChatProjection page={next} active />,
      );
      expect(container.querySelector(".acp-thought")).toBeNull();
      expect(screen.queryByText("Checked the file changes")).toBeNull();
      expect(
        container.querySelector(".acp-turn__items > .acp-message--assistant"),
      ).toHaveTextContent("Final answer");
    },
  );

  it("collapses trailing progress but leaves only the last assistant answer visible", async () => {
    const next = page("completed");
    next.turns[0].items = [
      {
        ...textItem(),
        id: "final-answer" as AcpItem["id"],
        ordinal: 3,
        payload: { role: "assistant", text: "Final answer" },
      },
      activityItem("command", "activity", {
        origin: "primary",
        activityId: null,
        ordinal: 1,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: 0,
            outputTail: null,
            outputTruncated: false,
            label: "Run command",
            summary: "gh --version",
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      }),
      {
        ...textItem(),
        id: "trailing-progress" as AcpItem["id"],
        ordinal: 2,
        payload: { role: "assistant", text: "Trailing progress" },
      },
    ];

    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const work = container.querySelector(".chat-transcript-work");
    expect(work).not.toHaveAttribute("open");
    expect(work).toHaveTextContent("Trailing progress");
    expect(work).not.toHaveTextContent("Final answer");
    expect(
      container.querySelector(".acp-turn__items > .acp-message--assistant"),
    ).toHaveTextContent("Final answer");
  });

  it("places a decided permission in the Worked activity, not the answer flow", async () => {
    const next = page("completed");
    next.turns[0].items = [
      activityItem("decided-permission", "activity", {
        origin: "primary",
        activityId: null,
        kind: "permissionRequest",
        toolState: "completed",
        payload: {
          requestId: "request-1" as SagePermissionRequestId,
          state: "resolvedAllowed",
          label: "Read requested",
          summary: "Allow reading this file",
        },
      }),
      textItem(),
    ];
    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const permission = container.querySelector(".acp-permission--settled");
    expect(permission).not.toBeNull();
    expect(permission?.closest(".chat-transcript-work")).not.toBeNull();
  });

  it("keeps Working time ticking without projection events and freezes on settlement", () => {
    vi.useFakeTimers();
    const clearIntervalSpy = vi.spyOn(window, "clearInterval");
    try {
      vi.setSystemTime(4_000);
      const running = page("running").turns[0];
      running.createdAt = 1_000;
      const { rerender, unmount } = render(
        <ChatTranscriptTurnSummary turn={running} hasActivities />,
      );
      expect(screen.getByText("Working for 3s")).toBeInTheDocument();
      act(() => vi.advanceTimersByTime(2_000));
      expect(screen.getByText("Working for 5s")).toBeInTheDocument();
      const settled = {
        ...running,
        state: "completed" as const,
        terminalAt: 6_000,
      };
      rerender(<ChatTranscriptTurnSummary turn={settled} hasActivities />);
      expect(screen.getByText("Worked for 5s")).toBeInTheDocument();
      expect(clearIntervalSpy).toHaveBeenCalled();
      act(() => vi.advanceTimersByTime(2_000));
      expect(screen.getByText("Worked for 5s")).toBeInTheDocument();
      unmount();
      const active = render(
        <ChatTranscriptTurnSummary turn={running} hasActivities />,
      );
      const clearCount = clearIntervalSpy.mock.calls.length;
      active.unmount();
      expect(clearIntervalSpy.mock.calls.length).toBeGreaterThan(clearCount);
    } finally {
      clearIntervalSpy.mockRestore();
      vi.useRealTimers();
    }
  });

  it("keeps running activity rows visible and closes them after settlement", () => {
    const running = page("running").turns[0];
    const { rerender, container } = render(
      <ChatTranscriptTurnSummary turn={running} hasActivities>
        <p>Running npm test</p>
      </ChatTranscriptTurnSummary>,
    );
    const work = container.querySelector(".chat-transcript-work");
    expect(work).not.toBeInstanceOf(HTMLDetailsElement);
    expect(screen.getByText("Running npm test")).toBeVisible();
    const settled = {
      ...running,
      state: "completed" as const,
      terminalAt: running.createdAt + 5_000,
    };
    rerender(
      <ChatTranscriptTurnSummary turn={settled} hasActivities>
        <p>Running npm test</p>
      </ChatTranscriptTurnSummary>,
    );
    expect(work).not.toHaveAttribute("open");
  });

  it("renders prompt image attachments above the user bubble", async () => {
    const next = page("completed");
    const user = {
      ...textItem(),
      id: "user-prompt" as AcpItem["id"],
      ordinal: 1,
      payload: { role: "user" as const, text: "Look at this" },
    } as AcpItem;
    const image = activityItem("prompt-image", "activity", {
      origin: "primary",
      activityId: null,
      ordinal: 2,
      kind: "image",
      toolState: null,
      payload: {
        mediaType: "image/png",
        source: "user",
        dataUrl:
          "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=",
      },
    });
    next.turns[0].items = [user, image, { ...textItem(), ordinal: 3 }];
    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const attachment = container.querySelector(".acp-image");
    const bubble = container.querySelector(".acp-message--user");
    const work = container.querySelector(".chat-transcript-work");
    expect(attachment).not.toBeNull();
    expect(
      screen.getByRole("img", { name: "Image attached by you" }),
    ).toBeInTheDocument();
    expect(attachment?.closest(".acp-turn__attachments")).not.toBeNull();
    expect(bubble).not.toBeNull();
    expect(
      attachment!.compareDocumentPosition(bubble!) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      bubble!.compareDocumentPosition(work!) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("keeps an agent image before assistant text in the agent flow", async () => {
    const next = page("completed");
    const user = {
      ...textItem(),
      id: "user-prompt" as AcpItem["id"],
      ordinal: 1,
      payload: { role: "user" as const, text: "Make an image" },
    } as AcpItem;
    const image = activityItem("generated-image", "activity", {
      origin: "primary",
      activityId: null,
      ordinal: 2,
      kind: "image",
      toolState: null,
      payload: {
        mediaType: "image/png",
        source: "agent",
        dataUrl:
          "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=",
      },
    });
    next.turns[0].items = [user, image, { ...textItem(), ordinal: 3 }];
    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const bubble = container.querySelector(".acp-message--user")!;
    const generated = container.querySelector(".acp-image")!;
    expect(generated.closest(".acp-turn__attachments")).toBeNull();
    expect(
      bubble.compareDocumentPosition(generated) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      screen.getByRole("img", { name: "Image provided by the agent" }),
    ).toBeInTheDocument();
  });

  it("renders an admitted png image item", async () => {
    const next = page("completed");
    const png =
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";
    next.turns[0].items = [
      {
        id: "item-image",
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "2",
        ordinal: 2,
        origin: "primary",
        activityId: null,
        lifecycle: "completed",
        revision: "1",
        createdAt: 2,
        updatedAt: 2,
        terminalAt: 2,
        kind: "image",
        toolState: null,
        payload: {
          mediaType: "image/png",
          dataUrl: `data:image/png;base64,${png}`,
        },
      } as AcpItem,
    ];
    render(<AcpChatProjection page={next} active />);
    const image = await screen.findByRole("img", {
      name: "Image provided by the agent",
    });
    expect(image).toHaveAttribute("src", `data:image/png;base64,${png}`);
    expect(
      screen.getByRole("img", { name: "Image provided by the agent" }),
    ).toBe(image);
  });

  it("does not render an img for svg, oversize, or http image payloads", () => {
    for (const payload of [
      {
        mediaType: "image/svg+xml",
        dataUrl: "data:image/svg+xml;base64,PHN2Zy8+",
      },
      {
        mediaType: "image/png",
        dataUrl: `data:image/png;base64,${"A".repeat(262_144)}`,
      },
      {
        mediaType: "image/png",
        dataUrl: "https://example.com/x.png",
      },
    ]) {
      const next = page("completed");
      next.turns[0].items = [
        {
          id: "item-rejected-image",
          kind: "image",
          origin: "primary",
          toolState: null,
          payload,
        } as AcpItem,
      ];
      const { container, unmount } = render(
        <AcpChatProjection page={next} active />,
      );
      expect(container.querySelector("img.acp-image__src")).toBeNull();
      unmount();
    }
  });

  it("renders mermaid fences as a copyable diagram instead of a raw fence only", async () => {
    const next = page("completed");
    const item = textItem();
    item.payload.text = "```mermaid\ngraph TD\nA --> B\n```";
    next.turns[0].items = [item];
    await renderProjection(<AcpChatProjection page={next} active />);
    expect(
      await screen.findByRole("img", { name: "Mermaid diagram" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "Copy diagram source" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/```mermaid/)).toBeNull();
  });

  it("opens an admitted harness browse URL without a click", () => {
    const next = page("running");
    next.turns[0].items = [
      {
        id: "item-browse",
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "2",
        ordinal: 2,
        origin: "primary",
        activityId: null,
        lifecycle: "streaming",
        revision: "1",
        createdAt: 2,
        updatedAt: 2,
        terminalAt: null,
        kind: "toolCall",
        toolState: "inProgress",
        payload: {
          presentation: {
            kind: "fetch",
            url: null,
            label: "_sage/browse",
            summary: "http://127.0.0.1:5173/app",
          },
          inputSummary: null,
        },
      } as AcpItem,
    ];
    render(<AcpChatProjection page={next} active />);
    expect(browseSessionForOwner(browseOwner)?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
  });

  it("does not open Preview from a non-curated web tool", () => {
    const next = page("running");
    next.turns[0].items = [
      {
        id: "item-web-search",
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "2",
        ordinal: 2,
        origin: "primary",
        activityId: null,
        lifecycle: "completed",
        revision: "1",
        createdAt: 2,
        updatedAt: 2,
        terminalAt: 2,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "fetch",
            url: null,
            label: "Web search",
            summary: "http://127.0.0.1:5173/returned-by-search",
          },
          inputSummary: null,
          outputSummary: null,
        },
      } as AcpItem,
    ];
    render(<AcpChatProjection page={next} active />);
    expect(browseSessionForOwner(browseOwner)).toBeNull();
  });

  it("keeps cancellation out of the transcript projection", () => {
    render(<AcpChatProjection page={page("completed")} active />);
    expect(screen.getByRole("status", { name: "Completed" })).toHaveTextContent(
      "Completed",
    );
    expect(screen.queryByRole("button", { name: /cancel turn/i })).toBeNull();
  });

  it("fully unmounts an inactive heavy projection", () => {
    const { container } = render(
      <AcpChatProjection page={page("running")} active={false} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("keeps one announcement pair when two projections are mounted", () => {
    const focusedPage = page("completed");
    focusedPage.title = "Focused chat";
    const unfocusedPage = page("completed");
    unfocusedPage.chatId = "chat-2" as AcpChatPage["chatId"];
    unfocusedPage.title = "Unfocused chat";
    unfocusedPage.turns[0].items = [
      {
        ...textItem(),
        payload: { role: "assistant", text: "Unfocused answer" },
      },
    ];
    render(
      <>
        <AcpChatProjection page={focusedPage} active focused />
        <AcpChatProjection page={unfocusedPage} active focused={false} />
      </>,
    );
    expect(document.querySelectorAll("[data-acp-announcement]")).toHaveLength(
      2,
    );
    expect(
      document.querySelectorAll('[data-acp-announcement="polite"]'),
    ).toHaveLength(1);
    expect(
      document.querySelectorAll('[data-acp-announcement="assertive"]'),
    ).toHaveLength(1);
    expect(screen.getByText("Normalized answer")).toBeInTheDocument();
    expect(screen.getByText("Unfocused answer")).toBeInTheDocument();
  });

  it("does not dispatch browse from an unfocused projection", () => {
    const next = page("running");
    next.turns[0].items = [
      textItem(),
      {
        id: "item-browse",
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "2",
        ordinal: 2,
        origin: "primary",
        activityId: null,
        lifecycle: "streaming",
        revision: "1",
        createdAt: 2,
        updatedAt: 2,
        terminalAt: null,
        kind: "toolCall",
        toolState: "inProgress",
        payload: {
          presentation: {
            kind: "fetch",
            url: null,
            label: "_sage/browse",
            summary: "http://127.0.0.1:5173/app",
          },
          inputSummary: null,
        },
      } as AcpItem,
    ];
    render(<AcpChatProjection page={next} active focused={false} />);
    expect(browseSessionForOwner(browseOwner)).toBeNull();
    expect(screen.getByText("Normalized answer")).toBeInTheDocument();
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

  it("renders one root-qualified edit summary and opens the existing per-file Diff target", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      textItem(),
      editItem("left-old", 1, "root-left", "old left event"),
      editItem("right", 2, "root-right", "right event"),
      editItem("left-new", 3, "root-left", "latest left event"),
    ];
    const openResource = vi.fn();

    await renderProjection(
      <AcpChatProjection
        page={edited}
        active
        projectRoots={[
          { id: "root-left", name: "Client", ordinal: 0 },
          { id: "root-right", name: "Server", ordinal: 1 },
        ]}
        onOpenResource={openResource}
      />,
    );

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    fireEvent.click(
      within(summary).getByRole("button", { name: /files changed/ }),
    );
    expect(
      within(summary).getAllByRole("button", { name: /open diff/i }),
    ).toHaveLength(2);
    expect(within(summary).queryByText("old left event")).toBeNull();
    const server = within(summary).getByRole("button", {
      name: "Open diff Server / src/main.ts",
    });
    fireEvent.click(server);
    expect(openResource).toHaveBeenCalledWith(
      { rootId: "root-right", relativePath: "src/main.ts" },
      "diff",
      { projectId: "project-1", chatId: "chat-1", sessionId: "session-1" },
      expect.objectContaining({
        itemId: "right",
        turnId: "turn-1",
        runtimeEpoch: "2",
        snapshotRevision: "1",
        contentDigest: "a".repeat(64),
      }),
    );
  });

  it("turn_edit_summary_shows_bounded_addition_deletion_chips", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      editItem("counted", 1, "root-one", "counted", {}, 2, 1),
      editItem("unknown", 2, "root-two", "unknown"),
    ];

    await renderProjection(
      <AcpChatProjection
        page={edited}
        active
        projectRoots={[
          { id: "root-one", name: "One", ordinal: 0 },
          { id: "root-two", name: "Two", ordinal: 1 },
        ]}
      />,
    );

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    fireEvent.click(
      within(summary).getByRole("button", { name: /files changed/ }),
    );
    expect(within(summary).getAllByText("main.ts")).toHaveLength(2);
    expect(summary.querySelector(".file-kind-icon--typescript")).not.toBeNull();
    expect(within(summary).getAllByText("+2")).toHaveLength(2);
    expect(within(summary).getAllByText("-1")).toHaveLength(2);
    expect(within(summary).getAllByText("—")).toHaveLength(2);
    expect(
      within(summary).getByRole("button", { name: "Mark reviewed" }),
    ).toBeEnabled();
    expect(
      within(summary).getByRole("button", { name: "Needs follow-up" }),
    ).toBeEnabled();
    expect(summary).toHaveTextContent(
      "This records review status only. It does not apply or revert files.",
    );
    expect(
      within(summary).queryByRole("button", { name: "Accept" }),
    ).toBeNull();
    expect(
      within(summary).queryByRole("button", { name: "Discard" }),
    ).toBeNull();
  });

  it("selecting_a_changed_file_opens_the_existing_per_file_diff", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [editItem("change", 1, "root-one", "updated")];
    const openResource = vi.fn();

    await renderProjection(
      <AcpChatProjection
        page={edited}
        active
        projectRoots={[{ id: "root-one", name: "One", ordinal: 0 }]}
        onOpenResource={openResource}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /file changed/ }));

    fireEvent.click(
      screen.getByRole("button", { name: "Open diff One / src/main.ts" }),
    );
    expect(openResource).toHaveBeenCalledWith(
      { rootId: "root-one", relativePath: "src/main.ts" },
      "diff",
      { projectId: "project-1", chatId: "chat-1", sessionId: "session-1" },
      expect.objectContaining({
        itemId: "change",
        rootId: "root-one",
        snapshotRevision: "1",
      }),
    );
    expect(openResource).not.toHaveBeenCalledWith(expect.anything(), "review");
  });

  it("exposes a sole admitted root identity but fails closed for an ambiguous project root", () => {
    const soleRoot = page("completed");
    const { rerender } = render(
      <AcpChatProjection
        page={soleRoot}
        active
        projectRoots={[{ id: "root-one", name: "One", ordinal: 0 }]}
      />,
    );
    expect(screen.getByLabelText(/Agent chat/)).toHaveAttribute(
      "data-expected-resource-root-id",
      "root-one",
    );

    rerender(
      <AcpChatProjection
        page={soleRoot}
        active
        projectRoots={[
          { id: "root-one", name: "One", ordinal: 0 },
          { id: "root-two", name: "Two", ordinal: 1 },
        ]}
      />,
    );
    expect(screen.getByLabelText(/Agent chat/)).not.toHaveAttribute(
      "data-expected-resource-root-id",
    );
  });

  it("disambiguates duplicate root names in visible and accessible Diff labels", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      editItem("left", 1, "root-left", "left event"),
      editItem("right", 2, "root-right", "right event"),
    ];
    const openResource = vi.fn();

    await renderProjection(
      <AcpChatProjection
        page={edited}
        active
        projectRoots={[
          { id: "root-right", name: "Workspace", ordinal: 1 },
          { id: "root-left", name: "Workspace", ordinal: 0 },
        ]}
        onOpenResource={openResource}
      />,
    );

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    fireEvent.click(
      within(summary).getByRole("button", { name: /files changed/ }),
    );
    expect(within(summary).getByText("Workspace (root 1)")).toBeVisible();
    expect(within(summary).getByText("Workspace (root 2)")).toBeVisible();
    const left = within(summary).getByRole("button", {
      name: "Open diff Workspace (root 1) / src/main.ts",
    });
    const right = within(summary).getByRole("button", {
      name: "Open diff Workspace (root 2) / src/main.ts",
    });

    fireEvent.click(left);
    fireEvent.click(right);
    expect(openResource).toHaveBeenNthCalledWith(
      1,
      { rootId: "root-left", relativePath: "src/main.ts" },
      "diff",
      { projectId: "project-1", chatId: "chat-1", sessionId: "session-1" },
      expect.objectContaining({ rootId: "root-left", snapshotRevision: "1" }),
    );
    expect(openResource).toHaveBeenNthCalledWith(
      2,
      { rootId: "root-right", relativePath: "src/main.ts" },
      "diff",
      { projectId: "project-1", chatId: "chat-1", sessionId: "session-1" },
      expect.objectContaining({ rootId: "root-right", snapshotRevision: "1" }),
    );
  });

  it("discloses same-Turn paging until the full Turn has merged", async () => {
    const latest = page("completed");
    latest.sessions = [{ id: "session-1", ordinal: 0 } as never];
    latest.nextCursor = "same-turn-cursor" as never;
    latest.turns[0].items = [editItem("newer", 2, "root-a", "newer event")];

    const { rerender } = await renderProjection(
      <AcpChatProjection page={latest} active />,
    );

    let summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(within(summary).getByText("Showing 1 changed file")).toBeVisible();
    fireEvent.click(
      within(summary).getByRole("button", { name: /Showing 1 changed file/ }),
    );
    expect(
      screen.getByText(/earlier history has not been loaded/i),
    ).toBeVisible();

    const merged = {
      ...latest,
      nextCursor: null,
      turns: [
        {
          ...latest.turns[0],
          items: [
            editItem("older", 1, "root-b", "older event"),
            ...latest.turns[0].items,
          ],
        },
      ],
    };
    rerender(<AcpChatProjection page={merged} active />);
    await settleLazyModules();

    summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(within(summary).getByText("2 files changed")).toBeVisible();
    expect(
      screen.queryByText(/earlier history has not been loaded/i),
    ).toBeNull();
  });

  it("discloses frontend retention eviction without claiming a complete count", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      editItem("retained", 2, "root-a", "retained event"),
    ];

    await renderProjection(
      <AcpChatProjection page={edited} active retentionIncomplete />,
    );

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(within(summary).getByText("Showing 1 changed file")).toBeVisible();
    fireEvent.click(
      within(summary).getByRole("button", { name: /Showing 1 changed file/ }),
    );
    expect(
      screen.getByText(/discarded to stay within memory limits/i),
    ).toBeVisible();
  });

  it("discloses the rendered-file cap and rejected stale entries without enabling them", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    const items: AcpItem[] = Array.from(
      { length: MAX_AGENT_EDIT_SUMMARY_FILES + 1 },
      (_, index) => {
        const item = editItem(
          `extra-${index}`,
          index,
          "root-extra",
          `extra ${index}`,
        );
        if (item.kind === "fileChangeDiff") {
          item.payload = {
            ...item.payload,
            changes: [
              {
                ...item.payload.changes[0],
                resource: {
                  kind: "resource",
                  resource: {
                    rootId: "root-extra",
                    relativePath: `src/extra-${index}.ts`,
                  },
                },
              },
            ],
          };
        }
        return item;
      },
    );
    items.push(
      editItem("stale", 100, "root-stale", "stale", {
        chatId: "chat-other" as never,
      }),
    );
    edited.turns[0].items = items;

    await renderProjection(<AcpChatProjection page={edited} active />);

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    fireEvent.click(
      within(summary).getByRole("button", { name: /Showing 32 changed files/ }),
    );
    expect(within(summary).getAllByRole("listitem")).toHaveLength(
      MAX_AGENT_EDIT_SUMMARY_FILES,
    );
    expect(
      within(summary).getByText(/1 additional changed files not shown/i),
    ).toBeInTheDocument();
    expect(
      within(summary).getByText(
        /unsafe or stale change records were not shown/i,
      ),
    ).toBeInTheDocument();
    expect(within(summary).queryByText("stale")).toBeNull();
  });

  it("keeps unavailable and wholly stale edit records noninteractive", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    const unavailable = editItem("unavailable", 1, "root-a", "unavailable");
    if (unavailable.kind === "fileChangeDiff") {
      unavailable.payload = {
        changes: [
          {
            resource: {
              kind: "unavailable",
              reason: "foreign",
              label: "Path is outside this Project",
            },
            operation: "unknown",
            originalResource: null,
            summary: "No safe Project resource",
            additions: null,
            deletions: null,
          },
        ],
        truncated: false,
      };
    }
    const stale = editItem("stale-only", 2, "root-a", "stale", {
      chatId: "chat-other" as never,
    });
    edited.turns[0].items = [unavailable, stale];
    const openResource = vi.fn();
    const { rerender } = await renderProjection(
      <AcpChatProjection page={edited} active onOpenResource={openResource} />,
    );

    let summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(within(summary).queryByRole("button")).toBeNull();
    expect(
      within(summary).getByText(/outside this Project/i),
    ).toBeInTheDocument();

    const refreshed = structuredClone(edited);
    refreshed.turns[0].items = [stale];
    refreshed.turns[0].revision = "2" as never;
    rerender(
      <AcpChatProjection
        page={refreshed}
        active
        onOpenResource={openResource}
      />,
    );
    summary = screen.getByRole("region", { name: "Files edited in turn 1" });
    expect(within(summary).queryByRole("list")).toBeNull();
    expect(
      within(summary).getByText(
        /unsafe or stale change records were not shown/i,
      ),
    ).toBeInTheDocument();
    expect(openResource).not.toHaveBeenCalled();
  });

  it("names a deleted unavailable file without offering an open action", async () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    const deleted = editItem("deleted", 1, "root-a", "deleted");
    if (deleted.kind === "fileChangeDiff") {
      deleted.payload = {
        changes: [
          {
            resource: {
              kind: "unavailable",
              reason: "missing",
              label: "Path does not exist",
              displayResource: {
                rootId: "root-a",
                relativePath: "cp8-pane-probe.md",
              },
            },
            operation: "deleted",
            originalResource: null,
            summary: "Deleted file",
            additions: null,
            deletions: null,
          },
        ],
        truncated: false,
      };
    }
    const legacy = editItem("legacy-deleted", 2, "root-a", "legacy-deleted");
    if (legacy.kind === "fileChangeDiff") {
      legacy.payload = {
        changes: [
          {
            resource: {
              kind: "unavailable",
              reason: "missing",
              label: "Path does not exist",
            },
            operation: "deleted",
            originalResource: null,
            summary: "Earlier deletion",
            additions: null,
            deletions: null,
          },
        ],
        truncated: false,
      };
    }
    edited.turns[0].items = [deleted, legacy];
    const openResource = vi.fn();
    await renderProjection(
      <AcpChatProjection page={edited} active onOpenResource={openResource} />,
    );

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(
      within(summary).getByText("cp8-pane-probe.md · Deleted"),
    ).toBeInTheDocument();
    expect(
      within(summary).getByText("Path does not exist · unavailable"),
    ).toBeInTheDocument();
    expect(within(summary).queryByRole("button")).toBeNull();
    expect(openResource).not.toHaveBeenCalled();
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
            kind: "read",
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
    const { rerender } = render(
      <AcpChatProjection page={nested} active onOpenResource={openResource} />,
    );

    const activityGroup = screen.getByRole("group", {
      name: "Harness activity: Root activity",
    });
    expect(activityGroup).toHaveAttribute(
      "data-sage-activity-id",
      "activity-root",
    );
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
      { projectId: "project-1", chatId: "chat-1", sessionId: "session-1" },
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

    rootNode!.focus();
    fireEvent.keyDown(rootNode!, { key: "ArrowRight" });
    const restoredChildNode = tree.querySelector<HTMLElement>(
      '[data-sage-id="nested-unknown"]',
    );
    const nestedDisclosure = within(restoredChildNode!).getByText(
      "Activity details",
    );
    nestedDisclosure.focus();
    expect(nestedDisclosure).toHaveFocus();
    const refreshed = structuredClone(nested);
    refreshed.turns[0].items = refreshed.turns[0].items.filter(
      (item) => item.id !== "nested-unknown",
    );
    refreshed.turns[0].activities = [refreshed.turns[0].activities[0]];
    refreshed.turns[0].revision = "2" as never;
    rerender(
      <AcpChatProjection
        page={refreshed}
        active
        onOpenResource={openResource}
      />,
    );
    expect(activityDisclosure).toHaveFocus();
  });

  it("keeps historical-session resource and browse rows non-interactive", () => {
    const historical = page("completed");
    historical.currentSessionId = "session-new" as never;
    historical.turns[0].items = [
      activityItem("historical-file", "historical", {
        origin: "primary",
        activityId: null,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "read",
            label: "Read source",
            resources: [
              {
                kind: "resource",
                resource: { rootId: "root-a", relativePath: "src/main.ts" },
              },
            ],
          },
          inputSummary: null,
        },
      }),
      activityItem("historical-web", "historical", {
        origin: "primary",
        activityId: null,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "fetch",
            url: null,
            label: "_sage/browse",
            summary: "http://127.0.0.1:5173/old",
          },
          inputSummary: null,
        },
      }),
    ];
    const openResource = vi.fn();

    render(
      <AcpChatProjection
        page={historical}
        active
        onOpenResource={openResource}
      />,
    );

    const readDisclosure = document.querySelector<HTMLDetailsElement>(
      '.acp-tool-rollup[data-activity-kind="read"]',
    );
    expect(readDisclosure).not.toBeNull();
    if (!readDisclosure)
      throw new Error("Expected the read activity disclosure");
    readDisclosure.open = true;
    fireEvent(readDisclosure, new Event("toggle"));

    expect(screen.getByText("main.ts")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open file src/main.ts" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Open in Sage browser" }),
    ).toBeNull();
    expect(openResource).not.toHaveBeenCalled();
  });

  it("shows only the latest live thinking line", async () => {
    const next = page("running");
    next.turns[0].items = [
      {
        ...textItem(),
        id: "new-thought",
        ordinal: 2,
        kind: "thoughtSummary",
        lifecycle: "streaming",
        payload: { summary: "Current thinking" },
      } as AcpItem,
      {
        ...textItem(),
        id: "old-thought",
        ordinal: 1,
        kind: "thoughtSummary",
        lifecycle: "streaming",
        payload: { summary: "Earlier thinking" },
      } as AcpItem,
    ];
    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    expect(screen.getByText("Current thinking")).toBeVisible();
    expect(screen.queryByText("Earlier thinking")).toBeNull();
    expect(container.querySelectorAll(".acp-thought")).toHaveLength(1);
  });

  it("shows live thinking text only until the turn completes", async () => {
    const next = page("running");
    next.turns[0].items = [
      {
        id: "thought",
        kind: "thoughtSummary",
        origin: "primary",
        lifecycle: "streaming",
        createdAt: 1_000,
        terminalAt: null,
        payload: { summary: "Checking the workspace before editing." },
      } as AcpItem,
    ];
    const { rerender } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    const live = screen.getAllByText(
      "Checking the workspace before editing.",
    )[0];
    expect(live).toBeVisible();
    expect(live).toHaveClass("acp-thought");

    const finished = structuredClone(next);
    finished.turns[0].state = "completed";
    finished.turns[0].revision = "4" as never;
    finished.turns[0].items[0].lifecycle = "completed";
    finished.turns[0].items[0].terminalAt = 4_000;
    finished.turns[0].items[0].revision = "2" as never;
    rerender(<AcpChatProjection page={finished} active />);
    expect(
      screen.queryAllByText("Checking the workspace before editing."),
    ).toHaveLength(0);
    expect(document.querySelector(".acp-thought")).toBeNull();
  });

  it.each(["interrupted", "cancelled"] as const)(
    "marks a partial assistant answer after a %s user stop",
    async (state) => {
      const next = page(state, { kind: "cancelledByUser" });
      next.turns[0].items = [
        {
          ...textItem(),
          lifecycle: "cancelled",
          payload: { role: "assistant", text: "Partial answer" },
        } as AcpItem,
      ];
      await renderProjection(<AcpChatProjection page={next} active />);
      const answer = screen.getByText("Partial answer");
      expect(answer.closest(".acp-message")).toHaveAttribute(
        "data-answer-state",
        "partial",
      );
      expect(
        screen.getByText(state === "cancelled" ? "Cancelled" : "Interrupted"),
      ).toBeVisible();
      expect(screen.getByText(/· partial answer/)).toBeVisible();
      expect(screen.queryByText("Completed")).toBeNull();
    },
  );

  it("does not mark a completed assistant answer as partial on an interrupted turn", async () => {
    const next = page("interrupted");
    next.turns[0].items = [
      {
        ...textItem(),
        lifecycle: "completed",
        payload: { role: "assistant", text: "Completed answer" },
      } as AcpItem,
    ];
    await renderProjection(<AcpChatProjection page={next} active />);
    expect(
      screen.getByText("Completed answer").closest(".acp-message"),
    ).not.toHaveAttribute("data-answer-state");
    expect(screen.queryByText(/partial answer/)).toBeNull();
  });

  it("shows a failed turn's message and the existing retry action together", async () => {
    const next = page("failed", {
      kind: "protocolError",
      message: "The harness connection closed.",
    });
    next.turns[0].items = [
      {
        ...textItem(),
        id: "prompt",
        payload: { role: "user", text: "Try again" },
      } as AcpItem,
    ];
    const onRetry = vi.fn();
    await renderProjection(
      <AcpChatProjection page={next} active onRetry={onRetry} />,
    );
    const failure = screen.getByRole("group", { name: "Turn failed" });
    expect(
      within(failure).getByText("The harness connection closed."),
    ).toBeVisible();
    fireEvent.click(within(failure).getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("does not offer a failed-turn retry without that turn's current-session prompt", async () => {
    const next = page("failed", {
      kind: "protocolError",
      message: "Connection lost",
    });
    next.turns[0].items = [];
    next.turns = [
      {
        ...page("completed").turns[0],
        id: "prior" as never,
        ordinal: 0,
        items: [
          {
            ...textItem(),
            id: "old-prompt",
            payload: { role: "user", text: "Old request" },
          } as AcpItem,
        ],
      },
      ...next.turns,
    ];
    const { rerender } = await renderProjection(
      <AcpChatProjection page={next} active onRetry={vi.fn()} />,
    );
    expect(
      within(screen.getByRole("group", { name: "Turn failed" })).queryByRole(
        "button",
        { name: "Retry" },
      ),
    ).toBeNull();

    const noSession = structuredClone(next);
    noSession.currentSessionId = null;
    noSession.turns[1].items = [
      {
        ...textItem(),
        id: "latest-prompt",
        payload: { role: "user", text: "Latest request" },
      } as AcpItem,
    ];
    noSession.turns[1].revision = "4" as never;
    rerender(<AcpChatProjection page={noSession} active onRetry={vi.fn()} />);
    expect(
      within(screen.getByRole("group", { name: "Turn failed" })).queryByRole(
        "button",
        { name: "Retry" },
      ),
    ).toBeNull();
  });

  it("does not synthesize thinking for Pi tool-only turns", async () => {
    const next = page("running");
    next.turns[0].items = [
      activityItem("tool", "activity", {
        origin: "primary",
        lifecycle: "streaming",
        kind: "toolCall",
        toolState: "inProgress",
        payload: {
          presentation: {
            kind: "execute",
            cwd: null,
            exitCode: null,
            outputTail: null,
            outputTruncated: false,
            label: "Run tool",
            summary: null,
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      }),
    ];
    const { container } = await renderProjection(
      <AcpChatProjection page={next} active />,
    );
    expect(
      container.querySelector('[data-activity-kind="execute"]'),
    ).not.toBeNull();
    expect(container.querySelector(".acp-thought")).toBeNull();
    expect(screen.queryByText("Thinking")).toBeNull();
    expect(screen.queryByText(/^Thought/)).toBeNull();
  });
});
