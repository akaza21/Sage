import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page, textItem } from "./acp/acpProjection.fixture";
import * as acpTurnState from "./acp/acpTurnState";
import { ChatTranscript } from "./ChatTranscript";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function renderTranscript(
  acpPage: AcpChatPage,
  messages: readonly ChatMessage[] = [],
) {
  const onRetry = vi.fn();
  const result = render(
    <ChatTranscript
      transcriptRef={{ current: null }}
      contentRef={{ current: null }}
      endRef={{ current: null }}
      onScroll={vi.fn()}
      projectTransitioning={false}
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
      messages={messages}
      developmentSimulator={false}
      onRetry={onRetry}
      showJumpToLatest={false}
      onJumpToLatest={vi.fn()}
    />,
  );
  await settleLazyModules();
  return { ...result, onRetry };
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

  it.each(["running", "waitingApproval", "cancelling"] as const)(
    "does not place a prior answer's actions beneath a %s turn",
    async (state) => {
      const next = retryPage();
      next.turns = [
        ...next.turns,
        {
          ...page(state).turns[0],
          id: "latest-turn" as never,
          ordinal: 2,
          items: [],
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
  it.each(["completed", "failed"] as const)(
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
