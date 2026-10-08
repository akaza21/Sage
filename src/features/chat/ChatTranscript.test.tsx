import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../../domain/acp";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page, textItem } from "./acp/acpProjection.fixture";
import { ChatTranscript } from "./ChatTranscript";

afterEach(cleanup);

async function renderTranscript(acpPage: AcpChatPage) {
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
      messages={[]}
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

describe("ChatTranscript Retry", () => {
  it.each(["no session", "historical session", "no prompt"])(
    "omits footer Retry with %s even when a callback is supplied",
    async (scenario) => {
      const next = retryPage();
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
