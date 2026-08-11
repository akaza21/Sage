import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpChatPage,
  CuratedBindingId,
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { AcpDesktopError, acpDesktop } from "../../../services/acpDesktop";
import { desktop } from "../../../services/desktop";
import { useAcpChatActions } from "./useAcpChatActions";

const bindingId = "binding-a" as CuratedBindingId;
const initialWorkbench = useWorkbenchStore.getState();

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

function page(): AcpChatPage {
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "A",
    projectionRevision: "1",
    currentSessionId: "session-a",
    sessions: [
      {
        id: "session-a",
        ordinal: 0,
        bindingId,
        modelId: null,
        state: "active",
        residency: "hot",
        runtimeEpoch: "1",
        nextRuntimeEpoch: null,
        capabilities: { prompt: true },
        revision: "1",
        projectionRevision: "1",
        legacy: false,
      },
    ],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

beforeEach(() => {
  useAcpProjectionStore.getState().clear();
  useWorkbenchStore.setState({
    phase: "ready",
    error: null,
    project: { id: "project-a", name: "A", roots: [] },
    tabs: [
      { id: "chat-a", kind: "chat", title: "A" },
      { id: "chat-b", kind: "chat", title: "B" },
    ] as never,
    archivedChats: [],
    activeTabId: "chat-a",
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    chatOwnershipEpoch: 1,
    chatUiById: {},
    chatError: null,
    composerText: "",
    sending: false,
  });
  const scope = {
    projectId: "project-a" as SageProjectId,
    projectGeneration: "0" as ProjectGeneration,
    chatId: "chat-a" as SageChatId,
  };
  useAcpProjectionStore.getState().activateScope(scope);
  const intent = useAcpProjectionStore.getState().beginPageLoad(null);
  useAcpProjectionStore.getState().completePageLoad(intent, page());
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
  useWorkbenchStore.setState(initialWorkbench, true);
});

describe("useAcpChatActions per-Chat ownership", () => {
  it("restores a background failure into only the captured Chat composer", async () => {
    const pending = deferred<Awaited<ReturnType<typeof acpDesktop.sendTurn>>>();
    const send = vi
      .spyOn(acpDesktop, "sendTurn")
      .mockReturnValue(pending.promise);
    vi.spyOn(desktop, "activateChat").mockImplementation(
      async (layout, expectedTransitionEpoch) => ({
        committed: true,
        transitionEpoch: expectedTransitionEpoch + 1,
        snapshot: {
          transitionEpoch: expectedTransitionEpoch + 1,
          projects: [],
          project: { id: "project-a", name: "A", roots: [] },
          tabs: layout.tabs,
          archivedChats: [],
          activeTabId: layout.activeTabId,
          selectedRootId: null,
          selectedResource: null,
          harnesses: [],
          sessionId: null,
          selectedHarnessId: null,
          selectedModelId: null,
          messages: [],
          repositories: { roots: [], truncated: false, elapsedMs: 0 },
          turnChanges: [],
          recoveredBuffers: [],
          recoveryNotice: null,
        },
        recoveryError: null,
      }),
    );
    useWorkbenchStore.getState().setComposerText("Retry ACP A");
    const { result } = renderHook(() => useAcpChatActions());

    let outcome!: Awaited<ReturnType<typeof result.current.submit>>;
    await act(async () => {
      const action = result.current.submit("Retry ACP A", bindingId, null);
      await waitFor(() => expect(send).toHaveBeenCalledOnce());
      expect(useWorkbenchStore.getState()).toMatchObject({
        composerText: "",
        sending: true,
      });
      await useWorkbenchStore.getState().activateChat("chat-b");
      useWorkbenchStore.getState().setComposerText("Draft B");
      pending.reject(
        new AcpDesktopError({
          kind: "conflict",
          resource: "projectionRevision",
          message: "ACP A failed in background",
        }),
      );
      outcome = await action;
    });

    expect(outcome).toBe("rejected");
    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "Draft B",
      sending: false,
      chatError: null,
      chatUiById: {
        "chat-a": {
          composerText: "Retry ACP A",
          sending: false,
          error: "ACP A failed in background",
        },
        "chat-b": { composerText: "Draft B", error: null },
      },
    });
  });

  it("keeps a successful background dispatch owned by its source Chat", async () => {
    const pending = deferred<Awaited<ReturnType<typeof acpDesktop.sendTurn>>>();
    const send = vi
      .spyOn(acpDesktop, "sendTurn")
      .mockReturnValue(pending.promise);
    vi.spyOn(desktop, "activateChat").mockImplementation(
      async (layout, expectedTransitionEpoch) => ({
        committed: true,
        transitionEpoch: expectedTransitionEpoch + 1,
        snapshot: {
          transitionEpoch: expectedTransitionEpoch + 1,
          projects: [],
          project: { id: "project-a", name: "A", roots: [] },
          tabs: layout.tabs,
          archivedChats: [],
          activeTabId: layout.activeTabId,
          selectedRootId: null,
          selectedResource: null,
          harnesses: [],
          sessionId: null,
          selectedHarnessId: null,
          selectedModelId: null,
          messages: [],
          repositories: { roots: [], truncated: false, elapsedMs: 0 },
          turnChanges: [],
          recoveredBuffers: [],
          recoveryNotice: null,
        },
        recoveryError: null,
      }),
    );
    useWorkbenchStore.getState().setComposerText("Dispatch from A");
    const { result } = renderHook(() => useAcpChatActions());

    let outcome!: Awaited<ReturnType<typeof result.current.submit>>;
    await act(async () => {
      const action = result.current.submit("Dispatch from A", bindingId, null);
      await waitFor(() => expect(send).toHaveBeenCalledOnce());
      await useWorkbenchStore.getState().activateChat("chat-b");
      useWorkbenchStore.getState().setComposerText("Draft B");
      pending.resolve({
        projection: {
          ...page(),
          projectionRevision: "2",
        } as AcpChatPage,
      });
      outcome = await action;
    });

    expect(outcome).toBe("ignoredStale");
    expect(send).toHaveBeenCalledWith(
      expect.objectContaining({ chatId: "chat-a", prompt: "Dispatch from A" }),
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: "chat-b",
      composerText: "Draft B",
      sending: false,
      chatError: null,
      chatUiById: {
        "chat-a": { composerText: "", sending: false, error: null },
        "chat-b": { composerText: "Draft B", error: null },
      },
    });
  });

  it("bounds a large action error in the owner map and clears it on Project reset", async () => {
    const oversizedError = "é".repeat(4 * 1024);
    vi.spyOn(acpDesktop, "sendTurn").mockRejectedValue(
      new AcpDesktopError({
        kind: "conflict",
        resource: "projectionRevision",
        message: oversizedError,
      }),
    );
    vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue({
      transitionEpoch: 1,
      projects: [],
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        { id: "chat-a", kind: "chat", title: "A" },
        { id: "chat-b", kind: "chat", title: "B" },
      ] as never,
      archivedChats: [],
      activeTabId: "chat-a",
      selectedRootId: null,
      selectedResource: null,
      harnesses: [],
      sessionId: null,
      selectedHarnessId: null,
      selectedModelId: null,
      messages: [],
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      turnChanges: [],
      recoveredBuffers: [],
      recoveryNotice: null,
    });
    useWorkbenchStore.getState().setComposerText("Retry bounded failure");
    const { result } = renderHook(() => useAcpChatActions());

    await act(async () => {
      await result.current.submit("Retry bounded failure", bindingId, null);
    });

    const retained = useWorkbenchStore.getState().chatUiById["chat-a"].error;
    expect(new TextEncoder().encode(retained ?? "")).toHaveLength(2 * 1024);
    await act(async () => {
      await useWorkbenchStore.getState().initialize();
    });
    expect(useWorkbenchStore.getState()).toMatchObject({
      chatError: null,
      chatUiById: { "chat-a": { error: null } },
    });
  });
});
