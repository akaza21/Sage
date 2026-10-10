import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page, textItem } from "./acp/acpProjection.fixture";
import * as acpTurnState from "./acp/acpTurnState";
import { ChatTranscript } from "./ChatTranscript";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function renderTranscript(
  acpPage: AcpChatPage | null,
  messages: readonly ChatMessage[] = [],
) {
  const onRetry = vi.fn();
  const transcript = (page: AcpChatPage | null) => (
    <ChatTranscript
      transcriptRef={{ current: null }}
      contentRef={{ current: null }}
      endRef={{ current: null }}
      onScroll={vi.fn()}
      projectTransitioning={false}
      useNativeAcp
      page={page}
      focused
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
      showJumpToLatest={false}
      onJumpToLatest={vi.fn()}
    />
  );
  const result = render(transcript(acpPage));
  await settleLazyModules();
  return {
    ...result,
    onRetry,
    async rerenderTranscript(next: AcpChatPage) {
      result.rerender(transcript(next));
      await settleLazyModules();
    },
  };
}

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

describe("ChatTranscript answer actions", () => {
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
    "does not place a prior answer's actions beneath a %s turn",
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
      expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
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
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
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
      expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
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
    "offers Retry after stopping before the first answer token (%s, prior answer: %s)",
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
        expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
        if (nextState !== state)
          expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
      }
      const retry = screen.getByRole("button", { name: "Retry" });
      expect(retry).toBeVisible();
      expect(retry.closest(".chat-transcript-actions")).not.toBeNull();
      fireEvent.click(retry);
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
