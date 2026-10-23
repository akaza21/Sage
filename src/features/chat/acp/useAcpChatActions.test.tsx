import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../../app/acpProjectionRegistry";
import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useSessionAccessModeStore } from "../../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpChatPage,
  CuratedBindingId,
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { DEFAULT_SESSION_ACCESS_MODE } from "../../../domain/sessionAccessMode";
import { getWorkbenchPorts } from "../../../ports/workbenchRuntime";
import { AcpDesktopError, acpDesktop } from "../../../services/acpDesktop";
import { useAcpChatActions } from "./useAcpChatActions";
import { ChatHarnessControls } from "../ChatHarnessControls";

const projectPort = getWorkbenchPorts().project;

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
    sessionConfig: {
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: {
        currentModeId: "review",
        availableModes: [
          { id: "review", name: "Review" },
          { id: "apply", name: "Apply" },
        ],
      },
      configOptions: [],
    },
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
  useSessionAccessModeStore.setState({
    modes: { fixture: DEFAULT_SESSION_ACCESS_MODE },
    ready: true,
    error: null,
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialWorkbench, true);
});

describe("useAcpChatActions per-Chat ownership", () => {
  it("shows a refusal when a prompt has no authoritative Chat context", async () => {
    useAcpProjectionStore.getState().clear();
    const send = vi.spyOn(acpDesktop, "sendTurn");
    const { result } = renderHook(() => useAcpChatActions("chat-a"));

    await act(async () => {
      expect(await result.current.submit("PONG", [], bindingId, null)).toBe(
        "rejected",
      );
    });

    expect(send).not.toHaveBeenCalled();
    expect(result.current.error).toMatch(/Chat.*load|Chat.*ready/i);
  });

  it("sends the Add menu mode through the scoped wire request and keeps its failure separate", async () => {
    const setSessionMode = vi
      .spyOn(acpDesktop, "setSessionMode")
      .mockRejectedValue(
        new AcpDesktopError({
          kind: "conflict",
          resource: "configRevision",
          message: "Mode update conflicted",
        }),
      );
    const { result } = renderHook(() => useAcpChatActions());
    render(
      <ChatHarnessControls
        availableBinding
        canPrompt
        discoveryPhase="ready"
        modelLabelScope="chat-a"
        sessionHarnessId="fixture"
        selectedHarnessId="fixture"
        sessionModelId={null}
        selectedModelId={null}
        projectName={null}
        turnBusy={false}
        sessionConfig={page().sessionConfig ?? null}
        modePending={false}
        modeError={null}
        focused
        onSetSessionMode={result.current.setSessionMode}
        onSetSessionConfigOption={result.current.setSessionConfigOption}
        onSelectHarness={vi.fn()}
        onSelectModel={vi.fn()}
        onModeRemoved={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitemradio", { name: "Apply" }));
    });
    expect(setSessionMode).toHaveBeenCalledExactlyOnceWith({
      projectId: "project-a",
      projectGeneration: "0",
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "1",
      modeId: "apply",
      expectedProjectionRevision: "1",
      expectedConfigRevision: "1",
      clientIntentId: expect.any(String),
    });

    expect(result.current.error).toBeNull();
    expect(result.current.modeError).toBe("Mode update conflicted");
    expect(result.current.modeChanging).toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      chatError: null,
      chatUiById: {
        "chat-a": {
          error: null,
          modeError: "Mode update conflicted",
          modeErrorOwner: { sessionId: "session-a", runtimeEpoch: "1" },
        },
      },
    });
  });

  it("keeps a config-option mutation pending and reports its refusal beside config controls", async () => {
    const pending = deferred<never>();
    vi.spyOn(acpDesktop, "setSessionConfigOption").mockReturnValue(
      pending.promise,
    );
    const configPage = page();
    configPage.sessionConfig = {
      ...configPage.sessionConfig!,
      configOptions: [
        {
          id: "fast",
          name: "Fast mode",
          type: "boolean",
          currentValue: false,
        },
      ],
    };
    const scope = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: "chat-a" as SageChatId,
    };
    useAcpProjectionStore.getState().applyAuthoritativePage(scope, configPage);
    const { result } = renderHook(() => useAcpChatActions());

    const change = result.current.setSessionConfigOption("fast", {
      type: "boolean",
      value: true,
    });
    await waitFor(() => expect(result.current.modeChanging).toBe(true));

    pending.reject(
      new AcpDesktopError({
        kind: "rejected",
        message: "Fast mode is unavailable for this session.",
      }),
    );
    await expect(change).resolves.toBe("rejected");
    expect(result.current.error).toBeNull();
    expect(result.current.modeError).toBe(
      "Fast mode is unavailable for this session.",
    );
  });

  it("does not leak a mode error across a runtime epoch change", async () => {
    useWorkbenchStore.setState({
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          sending: false,
          error: null,
          errorKind: null,
          modeError: "Old runtime rejected the mode",
          modeErrorOwner: { sessionId: "session-a", runtimeEpoch: "1" },
          modeChanging: false,
          requestGeneration: 0,
        },
      },
    });
    const { result } = renderHook(() => useAcpChatActions());
    expect(result.current.modeError).toBe("Old runtime rejected the mode");

    const scope = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: "chat-a" as SageChatId,
    };
    useAcpProjectionStore.getState().applyAuthoritativePage(scope, {
      ...page(),
      projectionRevision: "2" as never,
      sessionConfig: {
        ...page().sessionConfig!,
        runtimeEpoch: "2",
        configRevision: "2",
      },
    });

    await waitFor(() => expect(result.current.modeError).toBeNull());
  });

  it("does not turn a rapid repeat Send into a queued prompt", async () => {
    const pending = deferred<Awaited<ReturnType<typeof acpDesktop.sendTurn>>>();
    const send = vi
      .spyOn(acpDesktop, "sendTurn")
      .mockReturnValue(pending.promise);
    useWorkbenchStore.getState().setComposerText("one");
    const { result } = renderHook(() => useAcpChatActions());

    const first = result.current.submit("one", [], bindingId, null);
    const repeated = result.current.submit("two", [], bindingId, null);
    await waitFor(() => expect(send).toHaveBeenCalledOnce());
    await expect(repeated).resolves.toBe("rejected");
    expect(send).toHaveBeenCalledWith(
      expect.objectContaining({ prompt: "one" }),
    );

    pending.resolve({
      projection: { ...page(), projectionRevision: "2" } as AcpChatPage,
    });
    await expect(first).resolves.toBe("applied");
    expect(send).toHaveBeenCalledOnce();
  });

  it("restores a background failure into only the captured Chat composer", async () => {
    const pending = deferred<Awaited<ReturnType<typeof acpDesktop.sendTurn>>>();
    const send = vi
      .spyOn(acpDesktop, "sendTurn")
      .mockReturnValue(pending.promise);
    vi.spyOn(getWorkbenchPorts().chat, "activateChat").mockImplementation(
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
      const action = result.current.submit("Retry ACP A", [], bindingId, null);
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
    vi.spyOn(getWorkbenchPorts().chat, "activateChat").mockImplementation(
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
      const action = result.current.submit(
        "Dispatch from A",
        [],
        bindingId,
        null,
      );
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
    vi.spyOn(projectPort, "getBootstrapSnapshot").mockResolvedValue({
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
      await result.current.submit("Retry bounded failure", [], bindingId, null);
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

  it("submits a bound Chat that is not the active tab", async () => {
    const send = vi.spyOn(acpDesktop, "sendTurn").mockResolvedValue({
      projection: {
        ...page(),
        chatId: "chat-b",
        projectionRevision: "2",
      } as AcpChatPage,
    });
    const scopeB = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: "chat-b" as SageChatId,
    };
    const storeB = acpProjectionStoreFor("chat-b");
    storeB.getState().activateScope(scopeB);
    const intent = storeB.getState().beginPageLoad(null);
    storeB.getState().completePageLoad(intent, {
      ...page(),
      chatId: "chat-b",
    } as AcpChatPage);
    const { result } = renderHook(() => useAcpChatActions("chat-b"));
    await act(async () => {
      await result.current.submit("From B", [], bindingId, null);
    });
    expect(send).toHaveBeenCalledWith(
      expect.objectContaining({ chatId: "chat-b", prompt: "From B" }),
    );
  });

  it("does not show a sign-in error kind after the bound Chat changes", async () => {
    vi.spyOn(acpDesktop, "sendTurn").mockRejectedValue(
      new AcpDesktopError({
        kind: "signInRequired",
        message: "Sign in before sending a prompt.",
      }),
    );
    const scopeA = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: "chat-a" as SageChatId,
    };
    const storeA = acpProjectionStoreFor("chat-a");
    storeA.getState().activateScope(scopeA);
    const intentA = storeA.getState().beginPageLoad(null);
    storeA.getState().completePageLoad(intentA, page());
    const scopeB = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: "chat-b" as SageChatId,
    };
    const storeB = acpProjectionStoreFor("chat-b");
    storeB.getState().activateScope(scopeB);
    const intent = storeB.getState().beginPageLoad(null);
    storeB.getState().completePageLoad(intent, {
      ...page(),
      chatId: "chat-b",
    } as AcpChatPage);
    const { result, rerender } = renderHook(
      ({ chatId }) => useAcpChatActions(chatId),
      { initialProps: { chatId: "chat-a" } },
    );

    await act(async () => {
      await result.current.submit("Need access", [], bindingId, null);
    });
    expect(result.current.errorKind).toBe("signInRequired");

    rerender({ chatId: "chat-b" });
    expect(result.current.errorKind).toBeNull();
  });
});
