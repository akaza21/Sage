import { act, renderHook } from "@testing-library/react";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { FormEvent } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { MAX_ATTACHED_CONTEXTS_PER_TURN } from "../../domain/attachedContext";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { MAX_TRANSCRIPT_QUOTE_BYTES } from "../../domain/transcriptQuote";
import { MAX_EFFECTIVE_PROMPT_BYTES } from "./composerDisabledReason";

const { currentNonterminalTurn } = vi.hoisted(() => ({
  currentNonterminalTurn: vi.fn<
    (page: AcpChatPage | null) => TurnProjection | null
  >(() => null),
}));
vi.mock(import("./acp/currentNonterminalTurn"), async (importOriginal) => ({
  ...(await importOriginal()),
  currentNonterminalTurn,
}));

import { useChatSurfaceModel } from "./useChatSurfaceModel";

const initialAccess = useSessionAccessModeStore.getState();
beforeEach(() =>
  useSessionAccessModeStore.setState({
    ready: true,
    modes: { "binding-a": "readOnly" },
  }),
);
afterEach(() => {
  currentNonterminalTurn.mockReset();
  useSessionAccessModeStore.setState(initialAccess, true);
});

const page = {
  projectId: "project-a",
  chatId: "chat-a",
  currentSessionId: "session-a",
  turns: [],
  sessions: [
    {
      id: "session-a",
      legacy: false,
      runtimeEpoch: "1",
      bindingId: "binding-a",
      modelId: null,
      state: "ready",
      capabilities: { closeSession: true, prompt: true, promptResources: true },
    },
  ],
} as unknown as AcpChatPage;

function model(
  overrides: Partial<Parameters<typeof useChatSurfaceModel>[0]>,
  stagedRevision?: string,
) {
  const submit = vi.fn(async () => undefined);
  const queuePrompt = vi.fn(async () => true);
  const view = renderHook(() =>
    useChatSurfaceModel({
      harnesses: [
        {
          id: "harness-a",
          name: "Harness A",
          transport: "acp",
          bindingId: "binding-a",
          installState: "available",
          source: "builtIn",
          accessModes: [
            {
              id: "readOnly",
              label: "Read-only",
              description: "Read workspace files.",
              fallback: true,
            },
          ],
        },
      ],
      selectedHarnessId: "harness-a",
      selectedModelId: null,
      page,
      discoveryPhase: "ready",
      composerText: "Explain",
      attachedContext: [],
      sending: false,
      chatError: null,
      projectTransitioning: false,
      focused: true,
      acpActions: {
        busy: false,
        error: null,
        cancellableTurnId: null,
        submit,
        cancel: vi.fn(),
        close: vi.fn(),
        end: vi.fn(),
      },
      sendPrompt: vi.fn(),
      queuePrompt,
      ...overrides,
    }),
  );
  const event = { preventDefault: vi.fn() } as unknown as FormEvent;
  act(() => view.result.current.submit(event, stagedRevision));
  return { result: view.result.current, submit, queuePrompt };
}

it("surfaces the native prompt limit for a full draft whose annotations escape heavily", () => {
  // Every input is at its own cap: a full composer and the most annotations,
  // each quoting text whose JSON escaping doubles it (paths, JSON, logs).
  const quotes = Array.from(
    { length: MAX_ATTACHED_CONTEXTS_PER_TURN },
    (_, index) => ({
      kind: "transcriptQuote" as const,
      ordinal: index + 1,
      sourceTurnId: "turn-a",
      sourceItemId: `item-${index}`,
      text: "\\".repeat(MAX_TRANSCRIPT_QUOTE_BYTES),
      comment: '"'.repeat(MAX_TRANSCRIPT_QUOTE_BYTES),
      startOffset: 0,
      endOffset: MAX_TRANSCRIPT_QUOTE_BYTES,
    }),
  );
  const { result, submit } = model({
    composerText: "a".repeat(MAX_CHAT_COMPOSER_BYTES),
    attachedContext: quotes,
  });
  expect(result.sendBlocked).toBe(true);
  expect(result.disabledReason).toMatch(
    new RegExp(
      `^This message and its attached context are too large to send \\(\\d+ of ${MAX_EFFECTIVE_PROMPT_BYTES / 1000} KB\\)`,
    ),
  );
  expect(submit).not.toHaveBeenCalled();
});

it("sends staged pasted text with an empty message, naming its revision", () => {
  const { submit } = model({ composerText: "" }, "rev-1");
  expect(submit).toHaveBeenCalledWith("", [], "binding-a", null, {
    revision: "rev-1",
  });
});

it("names no staged batch for a message sent without one", () => {
  const { submit } = model({});
  expect(submit).toHaveBeenCalledWith("Explain", [], "binding-a", null);
});
