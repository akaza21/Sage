import { renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import type { AcpChatPage } from "../../domain/acp";
import type { DiffLineCommentContext } from "../../domain/attachedContext";
import type { FormEvent } from "react";

const { currentNonterminalTurn } = vi.hoisted(() => ({
  currentNonterminalTurn: vi.fn(() => null),
}));

vi.mock("./acp/currentNonterminalTurn", () => ({ currentNonterminalTurn }));

import { useChatSurfaceModel } from "./useChatSurfaceModel";

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
      capabilities: { closeSession: true, prompt: true },
    },
  ],
} as unknown as AcpChatPage;

it("does not rescan page turn eligibility for an unrelated composer edit", () => {
  const args = {
    harnesses: [
      {
        id: "harness-a",
        transport: "acp",
        bindingId: "binding-a",
        installState: "available",
        source: "builtIn",
      },
    ],
    selectedHarnessId: "harness-a",
    selectedModelId: null,
    page,
    discoveryPhase: "ready" as const,
    composerText: "first",
    attachedContext: [],
    sending: false,
    chatError: null,
    projectTransitioning: false,
    focused: true,
    acpActions: {
      busy: false,
      error: null,
      cancellableTurnId: null,
      submit: vi.fn(async () => undefined),
      cancel: vi.fn(async () => undefined),
      close: vi.fn(async () => undefined),
    },
    sendPrompt: vi.fn(async () => undefined),
  };
  const { rerender } = renderHook((next) => useChatSurfaceModel(next), {
    initialProps: args,
  });
  expect(currentNonterminalTurn).toHaveBeenCalledTimes(1);
  rerender({ ...args, composerText: "second" });
  expect(currentNonterminalTurn).toHaveBeenCalledTimes(1);
  rerender({ ...args, page: structuredClone(page) });
  expect(currentNonterminalTurn).toHaveBeenCalledTimes(2);
});

it("blocks legacy submit truthfully while preserving visible attachments", () => {
  const sendPrompt = vi.fn(async () => undefined);
  const args = {
    harnesses: [
      {
        id: "legacy",
        transport: "stdio",
        installState: "available",
        source: "builtIn",
      },
    ],
    selectedHarnessId: "legacy",
    selectedModelId: null,
    page: null,
    discoveryPhase: "ready" as const,
    composerText: "Keep this draft",
    attachedContext: [{ kind: "diffLineComment" } as DiffLineCommentContext],
    sending: false,
    chatError: null,
    projectTransitioning: false,
    focused: true,
    acpActions: {
      busy: false,
      error: null,
      cancellableTurnId: null,
      submit: vi.fn(async () => undefined),
      cancel: vi.fn(async () => undefined),
      close: vi.fn(async () => undefined),
    },
    sendPrompt,
  };
  const { result, rerender } = renderHook((next) => useChatSurfaceModel(next), {
    initialProps: args,
  });
  expect(result.current.composerEnabled).toBe(false);
  expect(result.current.disabledReason).toBe(
    "Attached diff comments require an ACP harness. Remove them or select an ACP harness.",
  );
  result.current.submit({ preventDefault: vi.fn() } as unknown as FormEvent);
  expect(sendPrompt).not.toHaveBeenCalled();
  rerender({ ...args, attachedContext: [] });
  result.current.submit({ preventDefault: vi.fn() } as unknown as FormEvent);
  expect(sendPrompt).toHaveBeenCalledOnce();
});
