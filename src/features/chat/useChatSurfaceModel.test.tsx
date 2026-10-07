import { renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { capabilitySetFixture } from "../../domain/acpProjection.fixture";
import type { DiffLineCommentContext } from "../../domain/attachedContext";
import type { HarnessDefinition } from "../../domain/harness";
import type { FormEvent } from "react";

const { currentNonterminalTurn } = vi.hoisted(() => ({
  currentNonterminalTurn: vi.fn<
    (page: AcpChatPage | null) => TurnProjection | null
  >(() => null),
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
        name: "Harness A",
        transport: "acp",
        bindingId: "binding-a",
        installState: "available",
        source: "builtIn",
      },
    ] satisfies readonly Pick<
      HarnessDefinition,
      | "id"
      | "name"
      | "transport"
      | "bindingId"
      | "installState"
      | "source"
      | "signIn"
    >[],
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
      end: vi.fn(async () => undefined),
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

it("hides usage from the previous session after harness selection diverges", () => {
  const usagePage = structuredClone(page);
  usagePage.turns = [
    {
      id: "turn-a",
      sessionId: "session-a",
      usage: { inputTokens: 25, outputTokens: 5 },
    } as TurnProjection,
  ];
  const args = {
    harnesses: [
      {
        id: "harness-a",
        name: "Harness A",
        transport: "acp",
        bindingId: "binding-a",
        installState: "available",
        source: "builtIn",
      },
      {
        id: "harness-b",
        name: "Harness B",
        transport: "acp",
        bindingId: "binding-b",
        installState: "available",
        source: "builtIn",
      },
    ] satisfies readonly Pick<
      HarnessDefinition,
      | "id"
      | "name"
      | "transport"
      | "bindingId"
      | "installState"
      | "source"
      | "signIn"
    >[],
    selectedHarnessId: "harness-a",
    selectedModelId: null,
    page: usagePage,
    discoveryPhase: "ready" as const,
    composerText: "",
    attachedContext: [],
    sending: false,
    chatError: null,
    projectTransitioning: false,
    focused: true,
    acpActions: {
      busy: false,
      error: null,
      cancellableTurnId: null,
      submit: vi.fn(),
      cancel: vi.fn(),
      close: vi.fn(),
      end: vi.fn(),
    },
    sendPrompt: vi.fn(),
  };
  const { result, rerender } = renderHook((next) => useChatSurfaceModel(next), {
    initialProps: args,
  });
  expect(result.current.usage?.inputTokens).toBe(25);
  rerender({ ...args, selectedHarnessId: "harness-b" });
  expect(result.current.usage).toBeNull();
});

it("keeps native End available while cancellation is negotiated or a turn is active", () => {
  const activeTurn: TurnProjection = {
    id: "turn-active" as TurnProjection["id"],
    sessionId: "session-a" as TurnProjection["sessionId"],
    runtimeEpoch: "1" as TurnProjection["runtimeEpoch"],
    ordinal: 1,
    state: "running",
    revision: "1" as TurnProjection["revision"],
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    items: [],
    activities: [],
    browserOperations: [],
    browserOperationsCollapsed: 0,
  };
  currentNonterminalTurn.mockReturnValueOnce(activeTurn);
  const endEligiblePage = structuredClone(page);
  endEligiblePage.sessions[0].capabilities = capabilitySetFixture({
    closeSession: false,
    cancel: true,
  });
  const { result } = renderHook(() =>
    useChatSurfaceModel({
      harnesses: [
        {
          id: "harness-a",
          name: "Harness A",
          transport: "acp",
          bindingId: "binding-a",
          installState: "available",
          source: "builtIn",
        },
      ],
      selectedHarnessId: "harness-a",
      selectedModelId: null,
      page: endEligiblePage,
      discoveryPhase: "ready",
      composerText: "",
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
        end: vi.fn(async () => undefined),
      },
      sendPrompt: vi.fn(async () => undefined),
    }),
  );
  expect(result.current.canEndSession).toBe(true);
});

it("blocks legacy submit truthfully while preserving visible attachments", () => {
  const sendPrompt = vi.fn(async () => undefined);
  const args = {
    harnesses: [
      {
        id: "legacy",
        name: "Legacy",
        transport: "legacySimulator",
        installState: "available",
        source: "builtIn",
        bindingId: null,
      },
    ] satisfies readonly Pick<
      HarnessDefinition,
      | "id"
      | "name"
      | "transport"
      | "bindingId"
      | "installState"
      | "source"
      | "signIn"
    >[],
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
      end: vi.fn(async () => undefined),
    },
    sendPrompt,
  };
  const { result, rerender } = renderHook((next) => useChatSurfaceModel(next), {
    initialProps: args,
  });
  expect(result.current.composerEnabled).toBe(false);
  expect(result.current.disabledReason).toBe(
    "Attached context requires an ACP harness. Remove it or select an ACP harness.",
  );
  result.current.submit({ preventDefault: vi.fn() } as unknown as FormEvent);
  expect(sendPrompt).not.toHaveBeenCalled();
  rerender({ ...args, attachedContext: [] });
  result.current.submit({ preventDefault: vi.fn() } as unknown as FormEvent);
  expect(sendPrompt).toHaveBeenCalledOnce();
});
