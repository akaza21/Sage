import {
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
  TurnProjection,
} from "../../../domain/acp";
import {
  browseSessionForOwner,
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "../../preview/harnessBrowse";
import type { HarnessBrowseOwner } from "../../preview/harnessBrowse";
import { MAX_AGENT_EDIT_SUMMARY_FILES } from "./agentEditSummary";
import { AcpChatProjection } from "./AcpChatProjection";

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

function textItem(): Extract<AcpItem, { kind: "text" }> {
  return {
    id: "item-text",
    kind: "text",
    origin: "primary",
    createdAt: Date.UTC(2026, 7, 31, 9, 5),
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
    sessions: [{ id: "session-1", runtimeEpoch: "2" }] as never,
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
  it("replaces only a revised immutable Turn row", async () => {
    const chatMarkdown = vi.fn(({ text }: { text: string }) => (
      <span>{text}</span>
    ));
    const turnEditSummary = vi.fn(() => null);
    vi.resetModules();
    vi.doMock("./ChatMarkdown", () => ({ ChatMarkdown: chatMarkdown }));
    vi.doMock("./TurnEditSummary", () => ({
      TurnEditSummary: turnEditSummary,
    }));
    const { AcpChatProjection: MemoProjection } =
      await import("./AcpChatProjection");
    const initial = page("completed");
    initial.turns = Array.from({ length: 14 }, (_, index) => {
      const turn = structuredClone(initial.turns[0]);
      turn.id = `turn-${index + 1}` as never;
      turn.ordinal = index + 1;
      turn.revision = "1" as never;
      turn.items = [
        {
          ...textItem(),
          id: `item-${index + 1}` as never,
          turnId: turn.id,
          revision: "1" as never,
          payload: { role: "assistant", text: `Retained row ${index + 1}` },
        } as AcpItem,
        {
          ...textItem(),
          id: `retained-item-${index + 1}` as never,
          turnId: turn.id,
          revision: "1" as never,
          payload: {
            role: "assistant",
            text: `Unchanged sibling ${index + 1}`,
          },
        } as AcpItem,
      ];
      return turn;
    });
    const { container, rerender } = render(
      <MemoProjection page={initial} active />,
    );
    await screen.findByText("Retained row 14");
    chatMarkdown.mockClear();
    turnEditSummary.mockClear();
    const before = Array.from(
      container.querySelectorAll<HTMLElement>(".acp-turn"),
    );
    const revised = structuredClone(initial);
    const changed = revised.turns[6]!;
    changed.revision = "2" as never;
    changed.items = changed.items.map((item, index) =>
      index === 0
        ? ({
            ...item,
            revision: "2" as never,
            payload: { role: "assistant", text: "Revised row 7" },
          } as AcpItem)
        : item,
    );
    rerender(<MemoProjection page={revised} active />);
    await screen.findByText("Revised row 7");
    const after = Array.from(
      container.querySelectorAll<HTMLElement>(".acp-turn"),
    );
    expect(chatMarkdown).toHaveBeenCalledOnce();
    expect(turnEditSummary).toHaveBeenCalledOnce();
    expect(after[0]).toBe(before[0]);
    expect(after[6]).toBe(before[6]);
    expect(after[6]).toHaveTextContent("Revised row 7");
    expect(after[13]).toBe(before[13]);
    vi.doUnmock("./ChatMarkdown");
    vi.doUnmock("./TurnEditSummary");
    vi.resetModules();
  });

  it("keeps one tool card when an admitted result names its call", async () => {
    const next = page("completed");
    const call = activityItem("call", "activity", {
      origin: "primary",
      activityId: null,
      kind: "toolCall",
      lifecycle: "streaming",
      terminalAt: null,
      toolState: "inProgress",
      payload: {
        presentation: {
          kind: "generic",
          label: "Issue tracker lookup",
          summary: "Read the admitted issue summary",
        },
        inputSummary: "Issue 42",
      },
    });
    const thought = activityItem("thought-linked-tool", "activity", {
      origin: "primary",
      activityId: null,
      kind: "thoughtSummary",
      toolState: null,
      payload: { summary: "Checking issue state" },
    });
    const result = activityItem("result", "activity", {
      origin: "primary",
      activityId: null,
      kind: "toolUpdateResult",
      toolState: "completed",
      payload: {
        callItemId: "call" as AcpItem["id"],
        presentation: {
          kind: "generic",
          label: "Issue tracker lookup",
          summary: "Read the admitted issue summary",
        },
        outputSummary: "Issue 42 is ready.",
      },
    });
    next.turns[0].items = [thought, call];

    const { container, rerender } = render(
      <AcpChatProjection page={next} active />,
    );
    await screen.findByRole("group", { name: "Issue tracker lookup" });
    const card = container.querySelector('[data-acp-item-id="call"]');
    expect(container.querySelectorAll(".acp-tool-card")).toHaveLength(1);
    expect(card).not.toHaveAttribute("data-tool-result-id");
    expect(card).toHaveAttribute("data-activity-state", "inProgress");
    expect(container.querySelector(".acp-thought__working")).not.toBeNull();

    const revised = structuredClone(next);
    revised.turns[0].revision = "4" as never;
    revised.turns[0].items = [thought, call, result];
    rerender(<AcpChatProjection page={revised} active />);
    expect(container.querySelectorAll(".acp-tool-card")).toHaveLength(1);
    expect(container.querySelector('[data-acp-item-id="call"]')).toBe(card);
    expect(card).toHaveAttribute("data-tool-result-id", "result");
    expect(card).toHaveAttribute("data-activity-state", "completed");
    expect(card).toHaveTextContent("Issue 42 is ready.");
    expect(container.querySelector(".acp-thought__working")).toBeNull();
  });

  it("keeps a terminal call state and call presentation over a stale result", async () => {
    const next = page("completed");
    const call = activityItem("terminal-call", "activity", {
      origin: "primary",
      activityId: null,
      kind: "toolCall",
      lifecycle: "failed",
      toolState: "failed",
      payload: {
        presentation: {
          kind: "web",
          label: "Browse preview",
          summary: "Opened http://127.0.0.1:5173/from-call",
        },
        inputSummary: "http://127.0.0.1:5173/from-input",
      },
    });
    const stale = activityItem("stale-result", "activity", {
      origin: "primary",
      activityId: null,
      kind: "toolUpdateResult",
      lifecycle: "streaming",
      terminalAt: null,
      toolState: "inProgress",
      payload: {
        callItemId: "terminal-call" as AcpItem["id"],
        presentation: {
          kind: "generic",
          label: "Wrong replacement",
          summary: "Wrong result presentation",
        },
        outputSummary: "Stale result detail",
      },
    });
    next.turns[0].items = [call, stale];

    const before = structuredClone(next);
    before.turns[0].items = [call];
    const { container, rerender } = render(
      <AcpChatProjection page={before} active />,
    );
    const card = await screen.findByRole("group", { name: "Browse preview" });
    const revised = structuredClone(next);
    revised.turns[0].revision = "4" as never;
    rerender(<AcpChatProjection page={revised} active />);
    expect(container.querySelectorAll(".acp-tool-card")).toHaveLength(1);
    expect(container.querySelector('[data-acp-item-id="terminal-call"]')).toBe(
      card,
    );
    expect(card).toHaveAttribute("data-activity-kind", "web");
    expect(card).toHaveAttribute("data-activity-state", "failed");
    expect(container.querySelector(".acp-thought__working")).toBeNull();
    expect(card).toHaveTextContent("from-call");
    expect(screen.queryByText("Wrong replacement")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Open in Sage browser" }),
    ).toBeVisible();
  });

  it("does not hide activity anchored to a linked result", async () => {
    const next = page("completed");
    const call = activityItem("tree-call", "activity-root", {
      kind: "toolCall",
      toolState: "inProgress",
      payload: {
        presentation: {
          kind: "generic",
          label: "Root call",
          summary: null,
        },
        inputSummary: null,
      },
    });
    const result = activityItem("tree-result", "activity-root", {
      kind: "toolUpdateResult",
      toolState: "completed",
      payload: {
        callItemId: "tree-call" as AcpItem["id"],
        presentation: {
          kind: "generic",
          label: "Root result",
          summary: null,
        },
        outputSummary: "done",
      },
    });
    const child = activityItem("tree-child", "activity-child", {
      kind: "unsupported",
      toolState: null,
      payload: {
        category: "extension.future",
        summary: "Retained child",
        truncated: false,
      },
    });
    next.turns[0].items = [call, result, child];
    next.turns[0].activities = [
      activity("activity-root", null, "item-text", 1),
      activity("activity-child", "activity-root", "tree-result", 2),
    ];

    render(<AcpChatProjection page={next} active />);
    const group = await screen.findByRole("group", {
      name: "Harness activity: Root activity",
    });
    fireEvent.click(
      within(group).getByText("Root activity").closest("summary")!,
    );
    const resultNode = group.querySelector<HTMLElement>(
      '[data-sage-id="tree-result"]',
    )!;
    resultNode.focus();
    fireEvent.keyDown(resultNode, { key: "ArrowRight" });
    expect(
      within(group)
        .getAllByRole("treeitem")
        .find((item) => item.getAttribute("aria-level") === "2"),
    ).toHaveTextContent("Unsupported activity");
  });

  it("does not guess a call/result association without an admitted call ID", async () => {
    const next = page("completed");
    const presentation = {
      kind: "generic" as const,
      label: "Same label",
      summary: "Same summary",
    };
    next.turns[0].items = [
      activityItem("unlinked-call", "activity", {
        origin: "primary",
        activityId: null,
        kind: "toolCall",
        toolState: "inProgress",
        payload: { presentation, inputSummary: null },
      }),
      activityItem("unlinked-result", "activity", {
        origin: "primary",
        activityId: null,
        kind: "toolUpdateResult",
        toolState: "completed",
        payload: {
          callItemId: null,
          presentation,
          outputSummary: "Retained result",
        },
      }),
    ];

    const { container } = render(<AcpChatProjection page={next} active />);
    await screen.findByText("Tool activity · 2 items");
    const rollup =
      container.querySelector<HTMLDetailsElement>(".acp-tool-rollup");
    if (!rollup) throw new Error("Expected the tool rollup disclosure");
    rollup.open = true;
    fireEvent(rollup, new Event("toggle"));
    expect(container.querySelectorAll(".acp-tool-card")).toHaveLength(2);
    expect(
      [...container.querySelectorAll(".acp-tool-card")].map((card) =>
        card.getAttribute("data-acp-item-id"),
      ),
    ).toEqual(["unlinked-call", "unlinked-result"]);
  });

  it("keeps unlinked tool results in their original turn position", async () => {
    const next = page("completed");
    const presentation = {
      kind: "generic" as const,
      label: "Ordered activity",
      summary: null,
    };
    next.turns[0].items = [
      activityItem("call-one", "activity", {
        origin: "primary",
        activityId: null,
        kind: "toolCall",
        toolState: "completed",
        payload: { presentation, inputSummary: null },
      }),
      textItem(),
      activityItem("result-one", "activity", {
        origin: "primary",
        activityId: null,
        kind: "toolUpdateResult",
        toolState: "completed",
        payload: {
          callItemId: null,
          presentation,
          outputSummary: "Retained result",
        },
      }),
    ];

    const { container } = render(<AcpChatProjection page={next} active />);
    await screen.findAllByText("Ordered activity");
    expect(
      [...container.querySelectorAll("[data-acp-item-id], .acp-message")].map(
        (element) =>
          element.getAttribute("data-acp-item-id") ??
          element.getAttribute("data-message-role"),
      ),
    ).toEqual(["call-one", "assistant", "result-one"]);
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
    render(<AcpChatProjection page={next} active />);
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
            kind: "web",
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
        kind: "toolUpdateResult",
        toolState: "completed",
        payload: {
          callItemId: "item-web-search" as AcpItem["id"],
          presentation: {
            kind: "web",
            label: "Web search",
            summary: "http://127.0.0.1:5173/returned-by-search",
          },
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
            kind: "web",
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

  it("renders one root-qualified edit summary and opens the existing per-file Diff target", () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      textItem(),
      editItem("left-old", 1, "root-left", "old left event"),
      editItem("right", 2, "root-right", "right event"),
      editItem("left-new", 3, "root-left", "latest left event"),
    ];
    const openResource = vi.fn();

    render(
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

  it("turn_edit_summary_shows_bounded_addition_deletion_chips", () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      editItem("counted", 1, "root-one", "counted", {}, 2, 1),
      editItem("unknown", 2, "root-two", "unknown"),
    ];

    render(
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

  it("selecting_a_changed_file_opens_the_existing_per_file_diff", () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [editItem("change", 1, "root-one", "updated")];
    const openResource = vi.fn();

    render(
      <AcpChatProjection
        page={edited}
        active
        projectRoots={[{ id: "root-one", name: "One", ordinal: 0 }]}
        onOpenResource={openResource}
      />,
    );

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

  it("disambiguates duplicate root names in visible and accessible Diff labels", () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      editItem("left", 1, "root-left", "left event"),
      editItem("right", 2, "root-right", "right event"),
    ];
    const openResource = vi.fn();

    render(
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

  it("discloses same-Turn paging until the full Turn has merged", () => {
    const latest = page("completed");
    latest.sessions = [{ id: "session-1", ordinal: 0 } as never];
    latest.nextCursor = "same-turn-cursor" as never;
    latest.turns[0].items = [editItem("newer", 2, "root-a", "newer event")];

    const { rerender } = render(<AcpChatProjection page={latest} active />);

    let summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(within(summary).getByText("Showing 1 edited file")).toBeVisible();
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

    summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(within(summary).getByText("Edited 2 files")).toBeVisible();
    expect(
      screen.queryByText(/earlier history has not been loaded/i),
    ).toBeNull();
  });

  it("discloses frontend retention eviction without claiming a complete count", () => {
    const edited = page("completed");
    edited.sessions = [{ id: "session-1", ordinal: 0 } as never];
    edited.turns[0].items = [
      editItem("retained", 2, "root-a", "retained event"),
    ];

    render(<AcpChatProjection page={edited} active retentionIncomplete />);

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
    expect(within(summary).getByText("Showing 1 edited file")).toBeVisible();
    expect(
      screen.getByText(/discarded to stay within memory limits/i),
    ).toBeVisible();
  });

  it("discloses the rendered-file cap and rejected stale entries without enabling them", () => {
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

    render(<AcpChatProjection page={edited} active />);

    const summary = screen.getByRole("region", {
      name: "Files edited in turn 1",
    });
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

  it("keeps unavailable and wholly stale edit records noninteractive", () => {
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
    const { rerender } = render(
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
            kind: "file",
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
            kind: "web",
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

    expect(screen.getByText("main.ts")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open file src/main.ts" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Open in Sage browser" }),
    ).toBeNull();
    expect(openResource).not.toHaveBeenCalled();
  });

  it("renders a collapsible Thought only from thoughtSummary using the session clock", async () => {
    const { syncAgentRailClock } =
      await import("../../../workbench/agentRailViewPrefs");
    syncAgentRailClock(19_000);
    const next = page("running");
    next.turns[0].items = [
      {
        id: "thought",
        kind: "thoughtSummary",
        origin: "primary",
        createdAt: 1_000,
        payload: { summary: "Checking the workspace" },
      } as AcpItem,
    ];
    render(<AcpChatProjection page={next} active />);
    const thought = screen.getByText("Thought 18s");
    expect(thought.closest("details")).not.toHaveAttribute("open");
    fireEvent.click(thought);
    expect(screen.getByText("Checking the workspace")).toBeVisible();
  });

  it("does not synthesize a Thought for tool-only turns and caps working rows", () => {
    const next = page("running");
    const tools = Array.from({ length: 65 }, (_, index) =>
      activityItem(`tool-${index}`, "activity", {
        kind: "toolCall",
        toolState: "inProgress",
        payload: {
          presentation: {
            kind: "command",
            label: `Tool ${index}`,
            summary: null,
            resources: [],
            activity: "ordinary",
          },
          inputSummary: null,
        },
      }),
    );
    next.turns[0].items = tools;
    const { container, rerender } = render(
      <AcpChatProjection page={next} active />,
    );
    expect(screen.queryByText(/^Thought/)).toBeNull();

    const refreshed = structuredClone(next);
    refreshed.turns[0].items = [
      {
        id: "thought",
        kind: "thoughtSummary",
        origin: "primary",
        createdAt: 0,
        payload: { summary: "Working" },
      } as AcpItem,
      ...tools,
    ];
    refreshed.turns[0].revision = "2" as never;
    rerender(<AcpChatProjection page={refreshed} active />);
    expect(container.querySelectorAll(".acp-thought__working li")).toHaveLength(
      64,
    );
  });

  it("shows an existing search summary as a nested working status line", () => {
    const next = page("running");
    next.turns[0].items = [
      {
        id: "thought",
        kind: "thoughtSummary",
        origin: "primary",
        createdAt: 0,
        payload: { summary: "Looking" },
      } as AcpItem,
      activityItem("search", "activity", {
        kind: "toolCall",
        toolState: "inProgress",
        payload: {
          presentation: {
            kind: "search",
            label: "Search workspace",
            querySummary: "needle",
            resources: [],
          },
          inputSummary: null,
        },
      }),
    ];
    render(<AcpChatProjection page={next} active />);
    fireEvent.click(screen.getByText(/^Thought/));
    expect(screen.getByText("Search workspace")).toBeVisible();
    expect(screen.getByText("Searching… needle")).toBeVisible();
  });
});
