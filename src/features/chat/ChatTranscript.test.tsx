import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page, textItem } from "./acp/acpProjection.fixture";
import * as acpTurnState from "./acp/acpTurnState";
import { ChatTranscript } from "./ChatTranscript";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";
import { snapshotPage } from "../../app/agentDiff.fixture";

afterEach(() => {
  cleanup();
  document
    .querySelectorAll(".chat-edit-summary-zone")
    .forEach((slot) => slot.remove());
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function renderTranscript(
  acpPage: AcpChatPage | null,
  messages: readonly ChatMessage[] = [],
  useNativeAcp = true,
  showJumpToLatest: boolean | "activity" = false,
) {
  const slot = document.createElement("div");
  slot.className = "chat-edit-summary-zone";
  document.body.append(slot);
  const onRetry = vi.fn();
  const onJumpToLatest = vi.fn();
  const transcript = (page: AcpChatPage | null, focused = true) => (
    <ChatTranscript
      editSummaryContainer={slot}
      transcriptRef={{ current: null }}
      contentRef={{ current: null }}
      endRef={{ current: null }}
      onScroll={vi.fn()}
      projectTransitioning={false}
      useNativeAcp={useNativeAcp}
      page={page}
      focused={focused}
      projectRoots={[]}
      retentionIncomplete={false}
      onOpenResource={vi.fn()}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      onLoadEarlier={vi.fn()}
      messages={messages}
      developmentSimulator={false}
      onRetry={onRetry}
      showJumpToLatest={showJumpToLatest}
      onJumpToLatest={onJumpToLatest}
    />
  );
  const result = render(transcript(acpPage));
  await settleLazyModules();
  return {
    ...result,
    onRetry,
    onJumpToLatest,
    async setFocused(focused: boolean) {
      result.rerender(transcript(acpPage, focused));
      await settleLazyModules();
    },
    async rerenderTranscript(next: AcpChatPage) {
      result.rerender(transcript(next));
      await settleLazyModules();
    },
  };
}

it("offers an icon-only, named jump button that follows the latest turn", async () => {
  const { onJumpToLatest } = await renderTranscript(
    page("completed"),
    [],
    true,
    true,
  );
  const jump = screen.getByRole("button", { name: "Jump to latest" });
  expect(jump).toHaveAttribute("title", "Jump to latest");
  expect(jump.textContent).toBe("");
  expect(jump.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  expect(jump.querySelector("svg")).toHaveAttribute("width", "24");
  expect(jump.querySelector("svg")).toHaveAttribute("height", "24");
  fireEvent.click(jump);
  expect(onJumpToLatest).toHaveBeenCalledOnce();
});

it("does not offer jump to latest while already following the transcript", async () => {
  await renderTranscript(page("completed"));
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
  const jump = document.querySelector<HTMLButtonElement>(".chat-follow-end");
  expect(jump).not.toBeNull();
  expect(jump).toHaveAttribute("hidden");
});

it("shows three bouncing dots and a new-activity name while follow is paused", async () => {
  const { onJumpToLatest } = await renderTranscript(
    page("running"),
    [],
    true,
    "activity",
  );
  const pill = screen.getByRole("button", {
    name: "New activity below. Jump to latest",
  });
  expect(pill).not.toHaveAttribute("hidden");
  expect(pill).toHaveAttribute("data-mode", "activity");
  expect(pill).toHaveAttribute("title", "New activity below. Jump to latest");
  expect(pill.textContent).toBe("");
  expect(pill.querySelectorAll('svg[aria-hidden="true"]')).toHaveLength(1);
  fireEvent.click(pill);
  expect(onJumpToLatest).toHaveBeenCalledOnce();
});

function retryPage() {
  const next = page("completed");
  next.turns[0].items = [
    {
      ...textItem(),
      id: "prompt" as never,
      payload: { role: "user", text: "Try again" },
    },
    ...next.turns[0].items,
  ];
  return next;
}

const footerCopies = () =>
  screen
    .queryAllByRole("button", { name: "Copy" })
    .filter((button) => !button.closest(".acp-turn"));

describe("ChatTranscript answer actions", () => {
  it("places isolated chat disclosures outside the scrolling transcript and resets them on owner change", async () => {
    const first = snapshotPage();
    const second = structuredClone(first);
    second.chatId = "chat-b" as never;
    second.turns[0].items = second.turns[0].items.map((item) => ({
      ...item,
      chatId: second.chatId,
    }));
    const firstView = await renderTranscript(first);
    await renderTranscript(second);
    const summaries = screen.getAllByRole("region", {
      name: "Files edited in this chat",
    });
    expect(summaries).toHaveLength(2);
    expect(summaries[0].closest(".message-list")).toBeNull();
    expect(summaries[0].parentElement).toHaveClass("chat-edit-summary-zone");
    const firstPill = within(summaries[0]).getByRole("button", {
      name: /1 file changed/,
    });
    const secondPill = within(summaries[1]).getByRole("button", {
      name: /1 file changed/,
    });
    fireEvent.click(firstPill);
    expect(firstPill).toHaveAttribute("aria-expanded", "true");
    expect(secondPill).toHaveAttribute("aria-expanded", "false");
    expect(within(summaries[1]).queryByRole("list")).toBeNull();
    await firstView.rerenderTranscript(second);
    for (const pill of screen.getAllByRole("button", {
      name: /1 file changed/,
    }))
      expect(pill).toHaveAttribute("aria-expanded", "false");
  });
  it.each([true, false])(
    "does not replay prior copy feedback when a Chat regains focus (ACP: %s)",
    async (native) => {
      vi.stubGlobal("navigator", {
        clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
      });
      const { container, setFocused } = await renderTranscript(
        native ? retryPage() : null,
        native
          ? []
          : [{ role: "assistant", text: "Legacy answer" } as ChatMessage],
        native,
      );
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
      await waitFor(() =>
        expect(
          container.querySelector('[data-acp-announcement="polite"]'),
        ).toHaveTextContent("Copied"),
      );
      await setFocused(false);
      await setFocused(true);
      await act(
        async () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
      );
      expect(
        container.querySelector('[data-acp-announcement="polite"]')
          ?.textContent,
      ).toBe(native ? "Completed" : "");
    },
  );

  it("does not defer a copy announcement until refocus when the clipboard finishes in an unfocused Chat", async () => {
    let completeCopy!: () => void;
    vi.stubGlobal("navigator", {
      clipboard: {
        writeText: () =>
          new Promise<void>((resolve) => {
            completeCopy = resolve;
          }),
      },
    });
    const { container, setFocused } = await renderTranscript(retryPage());
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await setFocused(false);
    await act(async () => completeCopy());
    await setFocused(true);
    await act(
      async () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
    expect(
      container.querySelector('[data-acp-announcement="polite"]'),
    ).toHaveTextContent("Completed");
  });
  it.each([false, true])(
    "announces answer copy feedback in the existing polite region (failed: %s)",
    async (failed) => {
      const writeText = failed
        ? vi.fn().mockRejectedValue(new Error("Clipboard denied"))
        : vi.fn().mockResolvedValue(undefined);
      vi.stubGlobal("navigator", { clipboard: { writeText } });
      const { container } = await renderTranscript(retryPage());
      const polite = container.querySelector(
        '[data-acp-announcement="polite"]',
      );
      expect(polite).toHaveAttribute("aria-live", "polite");
      expect(polite).toBeEmptyDOMElement();
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
      await waitFor(() =>
        expect(polite).toHaveTextContent(failed ? "Copy failed" : "Copied"),
      );
      expect(container.querySelectorAll('[aria-live="polite"]')).toHaveLength(
        1,
      );
      expect(writeText).toHaveBeenCalledExactlyOnceWith(
        textItem().payload.text,
      );
      const names: string[] = [];
      const observer = new MutationObserver(() => {
        const name = polite!.getAttribute("aria-label");
        if (name) names.push(name);
      });
      observer.observe(polite!, {
        attributes: true,
        attributeFilter: ["aria-label"],
      });
      fireEvent.click(
        screen.getByRole("button", { name: failed ? "Copy" : "Copied" }),
      );
      await waitFor(() =>
        expect(names).toContain(failed ? "Copy failed" : "Copied"),
      );
      observer.disconnect();
    },
  );
  it("announces Copy on an earlier answer like Copy on the latest", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const next = retryPage();
    next.turns = [
      ...next.turns,
      {
        ...page("completed").turns[0],
        id: "latest-turn" as never,
        ordinal: 2,
      },
    ];
    const { container } = await renderTranscript(next);
    const polite = container.querySelector('[data-acp-announcement="polite"]');
    const earlier = container.querySelector(
      `.acp-turn[data-turn-id="${next.turns[0].id}"]`,
    ) as HTMLElement;
    fireEvent.click(within(earlier).getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(polite).toHaveTextContent("Copied"));
  });
  it.each(["", " \n\t "])(
    "copies the settled answer before trailing blank assistant text %j",
    async (text) => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      vi.stubGlobal("navigator", { clipboard: { writeText } });
      const next = page("completed");
      next.turns[0].items = [
        {
          ...textItem(),
          payload: { role: "assistant", text: "The real answer" },
        },
        {
          ...textItem(),
          id: "blank-progress" as never,
          payload: { role: "assistant", text },
        },
      ];
      await renderTranscript(next);
      expect(screen.getByText("The real answer")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
      expect(writeText).toHaveBeenCalledExactlyOnceWith("The real answer");
    },
  );
  it.each(["", " \n\t "])(
    "offers no answer actions for blank settled assistant text %j",
    async (text) => {
      const next = page("completed");
      next.turns[0].items = [
        { ...textItem(), payload: { role: "assistant", text } },
      ];
      const { container } = await renderTranscript(next);
      expect(container.querySelector(".chat-transcript-actions")).toBeNull();
    },
  );
  it.each([
    "queued",
    "sending",
    "running",
    "waitingModel",
    "waitingTool",
    "waitingApproval",
    "waitingUser",
    "cancelling",
  ] as const)("hides answer actions beneath a %s turn", async (state) => {
    const next = retryPage();
    next.turns[0].state = state;
    const { container } = await renderTranscript(next);
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(container.querySelector(".chat-transcript-actions")).toBeNull();
  });

  it.each([
    "queued",
    "sending",
    "running",
    "waitingModel",
    "waitingTool",
    "waitingApproval",
    "waitingUser",
    "cancelling",
  ] as const)(
    "keeps a prior answer's Copy in its own Turn, not beneath a %s turn",
    async (state) => {
      const next = retryPage();
      next.turns = [
        ...next.turns,
        {
          ...page(state).turns[0],
          id: "latest-turn" as never,
          ordinal: 2,
          items: retryPage().turns[0].items.slice(0, 1),
        },
      ];
      await renderTranscript(next);
      const [copy] = screen.getAllByRole("button", { name: "Copy" });
      expect(copy.closest(".acp-turn")).toHaveAttribute(
        "data-turn-id",
        next.turns[0].id,
      );
      expect(footerCopies()).toEqual([]);
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    },
  );

  it.each(["completed", "cancelled", "interrupted", "failed"] as const)(
    "copies only the latest %s turn's answer",
    async (state) => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      vi.stubGlobal("navigator", { clipboard: { writeText } });
      const next = retryPage();
      const latest = page(state).turns[0];
      latest.id = "latest-turn" as never;
      latest.ordinal = 2;
      latest.items = [
        {
          ...textItem(),
          payload: {
            role: "assistant",
            text: "Latest partial or final answer",
          },
        },
      ];
      next.turns = [...next.turns, latest];
      await renderTranscript(next);
      const copies = footerCopies();
      expect(copies).toHaveLength(1);
      fireEvent.click(copies[0]);
      expect(writeText).toHaveBeenCalledExactlyOnceWith(
        "Latest partial or final answer",
      );
    },
  );

  it.each(["empty page", "empty latest answer", "prompt only"] as const)(
    "does not copy prior or legacy text with an ACP %s",
    async (scenario) => {
      const next = retryPage();
      if (scenario === "empty page") next.turns = [];
      else {
        const latest = page("completed").turns[0];
        latest.id = "latest-turn" as never;
        latest.ordinal = 2;
        latest.items =
          scenario === "prompt only"
            ? [{ ...textItem(), payload: { role: "user", text: "No answer" } }]
            : [];
        next.turns = [...next.turns, latest];
      }
      await renderTranscript(next, [
        {
          id: "legacy",
          role: "assistant",
          text: "Unrelated legacy answer",
          createdAt: 1,
        } as ChatMessage,
      ]);
      expect(footerCopies()).toEqual([]);
    },
  );
});

describe("ChatTranscript Retry", () => {
  it("offers no Retry in a native ACP chat whose page has not loaded", async () => {
    await renderTranscript(null);
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
  });

  it.each(["", "Legacy answer"])(
    "does not offer native footer Retry without a loaded ACP page (legacy text: %j)",
    (text) => {
      render(
        <ChatTranscriptFooter
          useNativeAcp
          page={null}
          messages={[{ role: "assistant", text } as ChatMessage]}
          onRetry={vi.fn()}
        />,
      );
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    },
  );

  it.each(["", " \n\t "])(
    "does not offer legacy footer Retry without answer text %j",
    (text) => {
      render(
        <ChatTranscriptFooter
          useNativeAcp={false}
          page={null}
          messages={[{ role: "assistant", text } as ChatMessage]}
          onRetry={vi.fn()}
        />,
      );
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
    },
  );

  it("keeps legacy footer Retry available with answer text", () => {
    const onRetry = vi.fn();
    render(
      <ChatTranscriptFooter
        useNativeAcp={false}
        page={null}
        messages={[{ role: "assistant", text: "Legacy answer" } as ChatMessage]}
        onRetry={onRetry}
      />,
    );
    expect(screen.getByRole("button", { name: "Copy" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it.each([
    ["cancelled", true],
    ["interrupted", true],
    ["cancelled", false],
    ["interrupted", false],
  ] as const)(
    "keeps Retry with the stopped notice before the first answer token (%s, prior answer: %s)",
    async (state, hasPriorAnswer) => {
      const initial = retryPage();
      if (!hasPriorAnswer) initial.turns = [];
      const { onRetry, rerenderTranscript } = await renderTranscript(initial);
      expect(screen.queryByRole("button", { name: "Copy" }) !== null).toBe(
        hasPriorAnswer,
      );
      const latest = retryPage().turns[0];
      latest.id = "latest-turn" as never;
      latest.ordinal = hasPriorAnswer ? 2 : 1;
      latest.items = latest.items.slice(0, 1);
      for (const nextState of ["running", "cancelling", state] as const) {
        await rerenderTranscript({
          ...initial,
          turns: [
            ...initial.turns,
            {
              ...latest,
              state: nextState,
              terminalAt: nextState === state ? 3 : null,
              ...(nextState === state
                ? { terminalReason: { kind: "cancelledByUser" as const } }
                : {}),
            },
          ],
        });
        expect(footerCopies()).toEqual([]);
        if (nextState !== state)
          expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
      }
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      expect(
        [...document.querySelectorAll(".chat-transcript-actions")].filter(
          (row) => !row.closest(".acp-turn"),
        ),
      ).toEqual([]);
      expect(onRetry).toHaveBeenCalledOnce();
    },
  );

  it.each(["completed", "cancelled", "interrupted", "failed"] as const)(
    "derives Retry eligibility once for a %s turn",
    async (state) => {
      const latestRetryPrompt = vi.spyOn(acpTurnState, "latestRetryPrompt");
      const next = retryPage();
      next.turns[0].state = state;
      await renderTranscript(next);
      expect(latestRetryPrompt).toHaveBeenCalledOnce();
      expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
    },
  );

  it.each([
    ["completed", "no session"],
    ["completed", "historical session"],
    ["completed", "no prompt"],
    ["failed", "no session"],
    ["failed", "historical session"],
    ["failed", "no prompt"],
    ["cancelled", "no session"],
    ["cancelled", "historical session"],
    ["cancelled", "no prompt"],
    ["interrupted", "no session"],
    ["interrupted", "historical session"],
    ["interrupted", "no prompt"],
  ] as const)(
    "omits Retry on a %s turn with %s even when a callback is supplied",
    async (state, scenario) => {
      const next = retryPage();
      next.turns[0].state = state;
      if (scenario === "no session") next.currentSessionId = null;
      if (scenario === "historical session")
        next.turns[0].sessionId = "prior-session" as never;
      if (scenario === "no prompt")
        next.turns[0].items = next.turns[0].items.slice(1);
      await renderTranscript(next);
      expect(screen.getByRole("button", { name: "Copy" })).toBeVisible();
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    },
  );

  it("offers a working footer Retry for the latest current-session prompt", async () => {
    const { onRetry } = await renderTranscript(retryPage());
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("keeps failed-turn Retry in the failure group, not the footer", async () => {
    const next = retryPage();
    next.turns[0].state = "failed";
    const { container } = await renderTranscript(next);
    const retry = screen.getByRole("button", { name: "Retry" });
    expect(retry.closest('[role="group"]')).toHaveAccessibleName("Turn failed");
    expect(
      container.querySelector(".chat-transcript-actions"),
    ).not.toContainElement(retry);
  });
});

describe("ChatTranscript side chats", () => {
  const SIDE = "side-00000000-0000-4000-8000-000000000001";
  function view(
    acpPage: AcpChatPage,
    project: { id: string; name: string },
    projectTransitioning: boolean,
    onRetry = vi.fn(),
  ) {
    return (
      <ChatTranscript
        transcriptRef={{ current: null }}
        contentRef={{ current: null }}
        endRef={{ current: null }}
        onScroll={vi.fn()}
        projectTransitioning={projectTransitioning}
        project={project}
        useNativeAcp
        page={acpPage}
        focused
        projectRoots={[]}
        retentionIncomplete={false}
        onOpenResource={vi.fn()}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        onLoadEarlier={vi.fn()}
        messages={[]}
        developmentSimulator={false}
        onRetry={onRetry}
        showJumpToLatest={false}
        onJumpToLatest={vi.fn()}
      />
    );
  }

  it("starts a fork with Continued from chat instead of the new-chat home", async () => {
    const fork = page("completed");
    fork.turns = [];
    const project = { id: "project-1", name: "Sage" };
    const { unmount } = render(view(fork, project, false));
    await settleLazyModules();
    expect(screen.getByRole("region", { name: "New chat" })).toBeVisible();
    unmount();
    render(
      view({ ...fork, forkedFrom: "chat-parent" as never }, project, false),
    );
    await settleLazyModules();
    expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
    expect(
      screen.getByText("Continued from an archived chat"),
    ).toBeInTheDocument();
  });

  it("offers no Retry in a side chat", async () => {
    const side = retryPage();
    side.chatId = SIDE as never;
    render(view(side, { id: "project-1", name: "Sage" }, false));
    await settleLazyModules();
    expect(screen.getByRole("button", { name: "Copy" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  it("shows Thinking, not a Working header, while a side chat has only started", async () => {
    const side = page("running");
    side.chatId = SIDE as never;
    side.turns[0].items = [
      {
        ...textItem(),
        id: "prompt" as never,
        payload: { role: "user", text: "Explain" },
      },
    ];
    const { container } = render(
      view(side, { id: "project-1", name: "Sage" }, false),
    );
    await settleLazyModules();
    expect(screen.getByText("Thinking")).toHaveClass("acp-live-shimmer");
    expect(container.textContent).not.toMatch(/Working/);
  });
});
