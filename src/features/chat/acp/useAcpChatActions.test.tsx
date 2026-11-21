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
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

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
  RuntimeEpoch,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import { DEFAULT_SESSION_ACCESS_MODE } from "../../../domain/sessionAccessMode";
import { getWorkbenchPorts } from "../../../ports/workbenchRuntime";
import { AcpDesktopError, acpDesktop } from "../../../services/acpDesktop";
import { acpHarnessDesktop } from "../../../services/acpHarnessDesktop";
import { useAcpChatActions } from "./useAcpChatActions";
import { clearChatSignInError } from "../chatSignInRecovery";
import { ChatHarnessControls } from "../ChatHarnessControls";
import { ComposerNoticeSurface } from "../ChatSignInNotices";
import {
  HarnessAccountMenu,
  HarnessSignInRecovery,
} from "../../harness/HarnessSignInStatus";
import {
  harnessSignInStatus,
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
  subscribeHarnessSignIn,
} from "../../harness/harnessSignIn";

let nativeTime = 100;

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
  clearMocks();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  resetHarnessSignInForTest();
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialWorkbench, true);
});

describe("useAcpChatActions per-Chat ownership", () => {
  it("retains a typed binding refusal across remount and clears its reason with the error", async () => {
    mockIPC((command) => {
      if (command === "send_acp_turn")
        return Promise.reject({
          kind: "unavailable",
          reason: "bindingUnavailable",
          message: "No admitted binding.",
        });
      throw new Error(command);
    });
    const first = renderHook(() => useAcpChatActions());
    await act(async () => {
      await first.result.current.submit(
        "Try this binding",
        [],
        bindingId,
        null,
      );
    });
    first.unmount();
    const reopened = renderHook(() => useAcpChatActions());
    expect(reopened.result.current.error).toBe("No admitted binding.");
    expect(reopened.result.current.errorUnavailableReason).toBe(
      "bindingUnavailable",
    );
    act(() => useWorkbenchStore.getState().clearActiveChatError());
    expect(reopened.result.current.errorUnavailableReason).toBeNull();
  });
  it.each(["read before delivery", "read after delivery"])(
    "keeps an untimestamped native refusal local (%s) until a fresh read succeeds",
    async (ordering) => {
      vi.spyOn(Date, "now").mockReturnValue(400);
      useWorkbenchStore.setState({
        project: {
          ...useWorkbenchStore.getState().project!,
          roots: [
            {
              id: "root",
              name: "Root",
              ordinal: 0,
              availability: "available",
              message: null,
            },
          ],
        },
        harnesses: [
          {
            id: "account",
            name: "Account",
            description: "",
            source: "builtIn",
            installState: "available",
            transport: "acp",
            bindingId,
            accountModelDiscovery: true,
          },
        ],
        selectedHarnessId: "account",
        harnessAccount: "managedChatgpt",
        harnessAccountReadStartedAt: 100,
        harnessDiscoveryPhase: "ready",
      });
      function Notices() {
        const actions = useAcpChatActions();
        return (
          <ComposerNoticeSurface
            workbenchError={null}
            chatError={actions.error}
            imageStatus={null}
            selectedHarnessId="account"
            discoveryPhase="ready"
            missingBindingNotice={false}
            chatAuth={{ page: page(), errorKind: actions.errorKind }}
          />
        );
      }
      render(<Notices />);
      render(<HarnessAccountMenu avatar="A" />);
      const otherChat = render(
        <HarnessSignInRecovery
          recovery={{
            harnessId: "account",
            scope: "other-chat-failed-turn",
            onSignedIn: () => undefined,
          }}
        />,
      );
      await waitFor(() =>
        expect(harnessSignInStatus("account").state).toBe("signedIn"),
      );
      const { result } = renderHook(() => useAcpChatActions());
      const refusal = deferred<never>();
      const entered = deferred<void>();
      mockIPC((command) => {
        if (command === "send_acp_turn") {
          entered.resolve();
          return refusal.promise;
        }
        throw new Error(command);
      });
      const read =
        deferred<Awaited<ReturnType<typeof acpHarnessDesktop.discoverCodex>>>();
      vi.spyOn(acpHarnessDesktop, "discoverCodex").mockImplementationOnce(
        () => read.promise,
      );
      let checking!: Promise<unknown>;
      let sending!: Promise<unknown>;
      act(() => {
        checking = refreshHarnessSignIn("account");
        sending = result.current.submit(
          "Expired credentials",
          [],
          bindingId,
          null,
        );
      });
      const account = {
        projectId: "project-a" as SageProjectId,
        projectGeneration: "0" as ProjectGeneration,
        bindingId,
        rootId: "root",
        runtimeEpoch: "1" as RuntimeEpoch,
        account: "managedChatgpt",
        accountReadStartedAt: 300,
        models: [],
        defaultModelId: null,
      } as Awaited<ReturnType<typeof acpHarnessDesktop.discoverCodex>>;
      const finishRead = async () => {
        read.resolve(account);
        await checking;
      };
      await act(async () => {
        await entered.promise;
        // Native persistence failed before discovery; IPC delivery can lag it.
        if (ordering === "read before delivery") await finishRead();
        refusal.reject({
          kind: "signInRequired",
          message: "Credentials expired.",
        });
        await sending;
        if (ordering === "read after delivery") await finishRead();
      });
      expect(result.current.errorKind).toBe("signInRequired");
      expect(await screen.findByText("Recheck Account sign-in")).toBeVisible();
      expect(harnessSignInStatus("account")).toMatchObject({
        state: "signedIn",
        evidenceAt: 300,
      });
      expect(
        screen.queryByLabelText("Harness sign-in needs attention"),
      ).toBeNull();
      expect(
        screen.getByRole("button", { name: "Sign in to Account" }),
      ).toBeEnabled();
      expect(otherChat.container).toBeEmptyDOMElement();
      vi.mocked(acpHarnessDesktop.discoverCodex).mockResolvedValueOnce(account);
      await act(async () => {
        await refreshHarnessSignIn("account");
      });
      expect(result.current.errorKind).toBe("signInRequired");
      expect(screen.getByText("Recheck Account sign-in")).toBeVisible();
      vi.mocked(acpHarnessDesktop.discoverCodex).mockResolvedValueOnce({
        ...account,
        accountReadStartedAt: 500,
      });
      fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
      await waitFor(() => expect(result.current.errorKind).toBeNull());
      expect(screen.queryByText("Recheck Account sign-in")).toBeNull();
    },
  );
  it.each([
    ["managedChatgpt", false],
    ["signedOut", false],
    ["managedChatgpt", true],
    ["signedOut", true],
  ] as const)(
    "keeps a failed Claude start's recovery after selecting a %s account harness (historical turn: %s)",
    async (account, historicalTurn) => {
      const projection = useAcpProjectionStore.getState();
      const emptyPage: AcpChatPage = {
        ...page(),
        currentSessionId: null,
        sessions: historicalTurn
          ? [
              {
                ...page().sessions[0],
                state: "closed",
                bindingId: "account-binding" as CuratedBindingId,
              },
            ]
          : [],
        turns: historicalTurn
          ? [
              {
                id: "old-turn",
                sessionId: "session-a",
                runtimeEpoch: "1",
                ordinal: 0,
                state: "failed",
                revision: "1",
                createdAt: 1,
                updatedAt: 2,
                terminalAt: 2,
                terminalReason: { kind: "authRequired" },
                items: [],
                activities: [],
                browserOperations: [],
                browserOperationsCollapsed: 0,
              } as unknown as AcpChatPage["turns"][number],
            ]
          : [],
      };
      const scope = projection.scope!;
      projection.clear();
      projection.activateScope(scope);
      projection.completePageLoad(projection.beginPageLoad(null), emptyPage);
      useWorkbenchStore.setState({
        harnesses: [
          {
            id: "cli",
            name: "Claude",
            description: "",
            source: "builtIn",
            installState: "available",
            transport: "acp",
            bindingId,
            accountModelDiscovery: false,
            signIn: { command: "", hint: "", statusCheck: "claudeCli" },
          },
          {
            id: "account",
            name: "Codex",
            description: "",
            source: "builtIn",
            installState: "available",
            transport: "acp",
            bindingId: "account-binding",
            accountModelDiscovery: true,
          },
        ],
        selectedHarnessId: "cli",
      });
      vi.spyOn(acpDesktop, "startSession").mockRejectedValue(
        new AcpDesktopError({
          kind: "signInRequired",
          terminalAt: ++nativeTime,
          message: "Claude refused start.",
        }),
      );
      const { result } = renderHook(() => useAcpChatActions());
      function Notices() {
        const selected = useWorkbenchStore((state) => state.selectedHarnessId);
        const actions = useAcpChatActions();
        return (
          <ComposerNoticeSurface
            workbenchError={null}
            chatError={actions.error}
            imageStatus={null}
            selectedHarnessId={selected}
            discoveryPhase="ready"
            missingBindingNotice={false}
            chatAuth={{ page: emptyPage, errorKind: actions.errorKind }}
          />
        );
      }
      render(<Notices />);
      await act(async () => {
        await result.current.submit("Hello", [], bindingId, null);
      });
      expect(await screen.findByText("Claude isn't signed in")).toBeVisible();
      act(() =>
        useWorkbenchStore.setState({
          selectedHarnessId: "account",
          harnessAccount: account,
          harnessAccountReadStartedAt: ++nativeTime,
          harnessDiscoveryPhase:
            account === "signedOut" ? "signInRequired" : "ready",
        }),
      );
      await waitFor(() =>
        expect(harnessSignInStatus("account").state).toBe(
          account === "signedOut" ? "signedOut" : "signedIn",
        ),
      );
      expect(result.current.errorKind).toBe("signInRequired");
      expect(
        screen.getByRole("button", { name: "Sign in to Claude" }),
      ).toBeVisible();
      expect(
        screen.queryByRole("button", { name: "Sign in to Codex" }),
      ).toBeNull();
      vi.spyOn(acpDesktop, "getClaudeAuthStatus").mockResolvedValue({
        loggedIn: true,
        accountReadStartedAt: ++nativeTime,
      });
      fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
      await waitFor(() => expect(result.current.errorKind).toBeNull());
      expect(result.current.error).toBeNull();
      expect(
        useWorkbenchStore.getState().chatUiById["chat-a"].dismissedAuthTurnId,
      ).toBeUndefined();
    },
  );
  it("keeps a newer sign-in when a refused command finishes settling", async () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "auth-fixture",
          name: "Auth fixture",
          description: "",
          installState: "available",
          source: "builtIn",
          transport: "acp",
          bindingId,
          accountModelDiscovery: true,
        },
      ],
      selectedHarnessId: "auth-fixture",
      harnessAccount: "managedChatgpt",
      harnessAccountReadStartedAt: ++nativeTime,
      harnessDiscoveryPhase: "ready",
    });
    let signedInElsewhere = false;
    let recovery: Promise<unknown> | undefined;
    vi.spyOn(acpHarnessDesktop, "discoverCodex").mockImplementation(
      async (scope) =>
        ({
          ...scope,
          rootId: null,
          account: "apiKey",
          accountReadStartedAt: ++nativeTime,
          models: [],
        }) as never,
    );
    const unsubscribe = subscribeHarnessSignIn(() => {
      if (
        !signedInElsewhere &&
        harnessSignInStatus("auth-fixture").state === "signedOut"
      ) {
        signedInElsewhere = true;
        recovery = refreshHarnessSignIn("auth-fixture");
      }
    });
    vi.spyOn(acpDesktop, "sendTurn").mockRejectedValue(
      new AcpDesktopError({
        kind: "signInRequired",
        terminalAt: ++nativeTime,
        message: "Sign in before sending.",
      }),
    );
    const { result } = renderHook(() => useAcpChatActions());
    await act(async () => {
      await result.current.submit("Hello", [], bindingId, null);
      await recovery;
    });
    expect(signedInElsewhere).toBe(true);
    expect(harnessSignInStatus("auth-fixture").state).toBe("signedIn");
    unsubscribe();
  });
  it.each([false, true])(
    "attributes a refused session start to the submitted binding (previous session: %s)",
    async (hasSession) => {
      const scope = useAcpProjectionStore.getState().scope!;
      useAcpProjectionStore.getState().clear();
      useAcpProjectionStore.getState().activateScope(scope);
      useAcpProjectionStore
        .getState()
        .completePageLoad(
          useAcpProjectionStore.getState().beginPageLoad(null),
          {
            ...page(),
            currentSessionId: hasSession ? page().currentSessionId : null,
            sessions: hasSession ? page().sessions : [],
          },
        );
      useWorkbenchStore.setState({
        harnesses: [bindingId, "new-binding"].map((binding, index) => ({
          id: `auth-${index}`,
          name: `Auth ${index}`,
          description: "",
          installState: "available",
          source: "builtIn",
          transport: "acp",
          bindingId: binding,
          accountModelDiscovery: false,
          signIn: { command: "", hint: "", statusCheck: "claudeCli" },
        })),
      });
      vi.spyOn(acpDesktop, "startSession").mockRejectedValue(
        new AcpDesktopError({
          kind: "signInRequired",
          terminalAt: ++nativeTime,
          message: "Sign in before starting.",
        }),
      );
      const { result } = renderHook(() => useAcpChatActions());
      await act(async () => {
        await result.current.submit(
          "Hello",
          [],
          "new-binding" as CuratedBindingId,
          null,
        );
      });
      expect(result.current.errorKind).toBe("signInRequired");
      expect(harnessSignInStatus("auth-1").state).toBe("signedOut");
      expect(harnessSignInStatus("auth-0").state).toBe("unknown");
    },
  );
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
    const apply = await screen.findByRole("menuitemradio", { name: "Apply" });
    await act(async () => {
      fireEvent.click(apply);
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

  it.each([
    ["a bound", "chat-a"],
    ["an unbound", null],
  ])("clears a sign-in refusal and its kind for %s Chat", async (_, chatId) => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "auth-fixture",
          name: "Auth fixture",
          description: "",
          installState: "available",
          source: "builtIn",
          transport: "acp",
          bindingId,
          accountModelDiscovery: false,
          signIn: { command: "", hint: "", statusCheck: "claudeCli" },
        },
      ],
    });
    vi.spyOn(acpDesktop, "sendTurn").mockRejectedValue(
      new AcpDesktopError({
        kind: "signInRequired",
        terminalAt: ++nativeTime,
        message: "Sign in before sending a prompt.",
      }),
    );
    if (chatId) {
      const store = acpProjectionStoreFor(chatId);
      store.getState().activateScope({
        projectId: "project-a" as SageProjectId,
        projectGeneration: "0" as ProjectGeneration,
        chatId: chatId as SageChatId,
      });
      store
        .getState()
        .completePageLoad(store.getState().beginPageLoad(null), page());
    }
    const { result } = renderHook(() => useAcpChatActions(chatId));

    await act(async () => {
      await result.current.submit("Need access", [], bindingId, null);
    });
    expect(result.current.errorKind).toBe("signInRequired");
    expect(harnessSignInStatus("auth-fixture").state).toBe("signedOut");

    const modeErrorOwner = { sessionId: "session-a", runtimeEpoch: "1" };
    act(() =>
      useWorkbenchStore.setState((state) => ({
        chatUiById: {
          ...state.chatUiById,
          "chat-a": {
            ...state.chatUiById["chat-a"],
            modeError: "A separate mode change was refused.",
            modeErrorOwner,
          },
        },
      })),
    );
    act(() => clearChatSignInError(chatId));
    expect(result.current.error).toBeNull();
    expect(result.current.errorKind).toBeNull();
    expect(useWorkbenchStore.getState().chatUiById["chat-a"]).toMatchObject({
      modeError: "A separate mode change was refused.",
      modeErrorOwner,
    });
  });

  it("does not show a sign-in error kind after the bound Chat changes", async () => {
    vi.spyOn(acpDesktop, "sendTurn").mockRejectedValue(
      new AcpDesktopError({
        kind: "signInRequired",
        terminalAt: ++nativeTime,
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
