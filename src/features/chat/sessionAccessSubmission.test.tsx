import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { FormEvent } from "react";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useChatSurfaceModel } from "./useChatSurfaceModel";
import type { AcpChatPage } from "../../domain/acp";

const readOnly = {
  id: "readOnly" as const,
  label: "Read-only",
  description: "No writes.",
  fallback: true,
};
beforeEach(() =>
  useSessionAccessModeStore.setState({ modes: {}, ready: true, error: null }),
);
afterEach(cleanup);
function args(modes = [readOnly]) {
  return {
    harnesses: [
      {
        id: "selected",
        name: "Selected",
        transport: "acp" as const,
        bindingId: "binding-a",
        installState: "available" as const,
        source: "builtIn" as const,
        accessModes: modes,
      },
    ],
    selectedHarnessId: "selected",
    selectedModelId: null,
    page: {
      projectId: "p",
      chatId: "c",
      currentSessionId: "old",
      sessions: [
        {
          id: "old",
          bindingId: "binding-old",
          modelId: null,
          legacy: false,
          runtimeEpoch: "1",
          state: "active",
          capabilities: { prompt: true },
        },
      ],
      turns: [],
    } as unknown as AcpChatPage,
    discoveryPhase: "ready" as const,
    composerText: "inspect",
    attachedContext: [],
    sending: false,
    chatError: null,
    projectTransitioning: false,
    focused: true,
    acpActions: {
      busy: false,
      error: null,
      cancellableTurnId: null,
      submit: vi.fn(async () => {}),
      cancel: vi.fn(),
      close: vi.fn(),
      end: vi.fn(),
    },
    sendPrompt: vi.fn(),
  };
}
it("disables send and explains unsupported access before any dispatch", () => {
  const input = args([]);
  const { result } = renderHook(() => useChatSurfaceModel(input));
  expect(result.current.composerEnabled).toBe(false);
  expect(result.current.disabledReason).toBe(
    "Choose a supported access mode before sending.",
  );
  act(() => result.current.submit({ preventDefault() {} } as FormEvent));
  expect(input.acpActions.submit).not.toHaveBeenCalled();
});
it("uses the same binding for access and the next session when selection differs from the old session", () => {
  const input = args();
  const { result } = renderHook(() => useChatSurfaceModel(input));
  expect(result.current.accessBindingId).toBe("binding-a");
  act(() => result.current.submit({ preventDefault() {} } as FormEvent));
  expect(input.acpActions.submit).toHaveBeenCalledWith(
    "inspect",
    [],
    "binding-a",
    null,
  );
});

it("blocks queued sends when the selected binding has no supported access", () => {
  const input = args([]);
  input.page.turns = [
    { id: "live", sessionId: "old", state: "running", ordinal: 0 },
  ] as never;
  const queuePrompt = vi.fn();
  const { result } = renderHook(() =>
    useChatSurfaceModel({ ...input, queuePrompt }),
  );
  expect(result.current.queueAvailable).toBe(false);
  expect(result.current.canPrompt).toBe(false);
  act(() => result.current.submit({ preventDefault() {} } as FormEvent));
  expect(queuePrompt).not.toHaveBeenCalled();
});

it("switches access by next-send binding and preserves each explicit preference", () => {
  const codex = args().harnesses[0];
  const harnesses = [
    codex,
    {
      ...codex,
      id: "claude",
      bindingId: "claude-binding",
      accessModes: [
        {
          id: "ask" as const,
          label: "Ask each time",
          description: "Review requests.",
          fallback: true,
        },
      ],
    },
    { ...codex, id: "opencode", bindingId: "opencode-binding" },
  ];
  useSessionAccessModeStore.setState({
    modes: { "binding-a": "workspaceWrite" },
  });
  const input = { ...args(), harnesses };
  const { result, rerender } = renderHook((next) => useChatSurfaceModel(next), {
    initialProps: input,
  });
  rerender({ ...input, selectedHarnessId: "claude" });
  expect(result.current.accessBindingId).toBe("claude-binding");
  expect(result.current.composerEnabled).toBe(true);
  act(() => result.current.submit({ preventDefault() {} } as FormEvent));
  expect(input.acpActions.submit).toHaveBeenLastCalledWith(
    "inspect",
    [],
    "claude-binding",
    null,
  );
  rerender({ ...input, selectedHarnessId: "opencode" });
  expect(result.current.accessBindingId).toBe("opencode-binding");
  expect(result.current.composerEnabled).toBe(true);
  rerender(input);
  expect(useSessionAccessModeStore.getState().modes).toEqual({
    "binding-a": "workspaceWrite",
  });
});

it("refuses a send whose annotations would overflow the prompt, with a clear reason", () => {
  const input = args();
  const quote = (ordinal: number) => ({
    kind: "transcriptQuote" as const,
    ordinal,
    sourceTurnId: "t",
    sourceItemId: `i${ordinal}`,
    text: '"'.repeat(4000),
    comment: '"'.repeat(4000),
    startOffset: 0,
    endOffset: 4000,
  });
  const { result } = renderHook(() =>
    useChatSurfaceModel({
      ...input,
      composerText: "x".repeat(16_000),
      attachedContext: [1, 2, 3, 4, 5, 6, 7, 8].map(quote),
    }),
  );
  expect(result.current.disabledReason).toMatch(
    /^This message and its attached context are too large to send/,
  );
  act(() => result.current.submit({ preventDefault() {} } as FormEvent));
  expect(input.acpActions.submit).not.toHaveBeenCalled();
});
