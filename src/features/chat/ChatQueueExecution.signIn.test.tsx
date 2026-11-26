import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../../domain/acp";
import ChatQueueExecution from "./ChatQueueExecution";

const pageEndingWith = (turn: object) =>
  ({
    currentSessionId: "session-1",
    turns: [
      {
        id: "turn-1",
        sessionId: "session-1",
        terminalReason: { kind: "authRequired" },
      },
      { sessionId: "session-1", ...turn },
    ],
  }) as unknown as AcpChatPage;

afterEach(cleanup);

it("holds a queued prompt after a sign-in failure until a newer turn replaces it", () => {
  const submit = vi.fn(async () => "applied" as const);
  const updateQueuedPrompt = vi.fn();
  const props = {
    queuedPrompts: [
      {
        id: "queued-1",
        text: "next",
        attachedContext: [],
        bindingId: "binding-a",
        modelId: null,
        status: "queued" as const,
      },
    ],
    queuedTurnBarrier: null,
    knownTurnOrder: null,
    activeTurn: false,
    actionBusy: false,
    projectTransitioning: false,
    canPrompt: true,
    useNativeAcp: true,
    page: pageEndingWith({
      id: "turn-1b",
      terminalReason: { kind: "authRequired" },
    }),
    observedOrder: null,
    submit,
    updateQueuedPrompt,
    setQueuedTurnBarrier: vi.fn(),
  };
  const view = render(<ChatQueueExecution {...props} />);
  expect(submit).not.toHaveBeenCalled();
  expect(updateQueuedPrompt).not.toHaveBeenCalled();
  view.rerender(
    <ChatQueueExecution {...props} page={pageEndingWith({ id: "turn-2" })} />,
  );
  expect(submit).toHaveBeenCalledOnce();
  expect(updateQueuedPrompt).toHaveBeenCalledWith("queued-1", "sending");
});
