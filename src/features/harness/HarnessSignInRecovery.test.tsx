import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useWorkbenchStore } from "../../app/store";
import type { StartAcpSessionRequest } from "../../domain/acp";
import { agentSessionFixture } from "../../domain/acpProjection.fixture";
import { acpDesktop } from "../../services/acpDesktop";
import { acpHarnessDesktop } from "../../services/acpHarnessDesktop";
import { desktop } from "../../services/desktop";
import {
  ComposerHarnessSignInNotice,
  HarnessAccountMenu,
} from "./HarnessSignInStatus";
import { HarnessDiscoveryNotice } from "../chat/HarnessDiscoveryNotice";
import { composerDisabledReason } from "../chat/composerDisabledReason";
import {
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
} from "./harnessSignIn";

vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  resetHarnessSignInForTest();
  clearMocks();
  useWorkbenchStore.setState(initial, true);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

function install() {
  useWorkbenchStore.setState({
    project: {
      id: "project",
      name: "Project",
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
        name: "Account harness",
        description: "",
        source: "builtIn",
        installState: "available",
        transport: "acp",
        bindingId: "account-binding",
        accountModelDiscovery: true,
      },
    ],
    selectedHarnessId: "account",
    harnessDiscoveryPhase: "signInRequired",
    harnessAccount: "signedOut",
  });
  vi.spyOn(acpHarnessDesktop, "discoverCodex").mockImplementation(
    async (scope) =>
      ({
        ...scope,
        rootId: "root",
        runtimeEpoch: "1",
        account: "managedChatgpt",
        models: [{ id: "model", name: "Model", isDefault: true }],
        defaultModelId: "model",
      }) as never,
  );
}

function sendDisabledReason() {
  return composerDisabledReason({
    actionBusy: false,
    activeTurn: false,
    availableBinding: true,
    canPrompt: true,
    chatLoadFailed: false,
    harnessAvailable: true,
    harnessDiscoveryPhase: useWorkbenchStore.getState().harnessDiscoveryPhase,
    projectTransitioning: false,
    selectedHarness: true,
    useNativeAcp: true,
  });
}

it.each([1_000, 60_001])(
  "refocusing after %i ms refreshes signed-out discovery and makes sending available",
  async (elapsed) => {
    install();
    vi.useFakeTimers();
    render(<ComposerHarnessSignInNotice harnessId="account" />);
    expect(sendDisabledReason()).toBe("Finish signing in before sending.");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(elapsed);
      fireEvent.focus(window);
      fireEvent.focus(window);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(sendDisabledReason()).toBeNull();
    expect(useWorkbenchStore.getState().selectedModelId).toBe("model");
    expect(screen.queryByText("Account harness isn't signed in")).toBeNull();
  },
);

it("starts a session with the discovered model after refocusing beyond the TTL", async () => {
  install();
  vi.useFakeTimers();
  vi.mocked(acpHarnessDesktop.discoverCodex).mockRestore();
  let catalog: string[] = [];
  mockIPC((command, args) => {
    const { request } = args as { request: StartAcpSessionRequest };
    if (command === "get_codex_discovery") {
      catalog = ["model"];
      return {
        ...request,
        rootId: "root",
        runtimeEpoch: "1",
        account: "managedChatgpt",
        models: [{ id: "model", name: "Model", isDefault: true }],
        defaultModelId: "model",
      };
    }
    if (command === "get_codex_auth") {
      // Reproduce the old native observation's catalog invalidation.
      catalog = [];
      return { ...request, phase: "signedIn", revision: "2" };
    }
    if (command !== "start_acp_session") throw new Error(command);
    if (!catalog.includes(request.modelId!))
      throw { kind: "invalidScope", message: "Model is absent from catalog." };
    const session = agentSessionFixture({
      bindingId: request.bindingId,
      modelId: request.modelId,
    });
    return {
      projection: {
        projectId: request.projectId,
        chatId: request.chatId,
        title: "Chat",
        projectionRevision: "1",
        currentSessionId: session.id,
        sessions: [session],
        turns: [],
        nextCursor: null,
      },
    };
  });
  await useWorkbenchStore.getState().retryHarnessDiscovery();
  render(<ComposerHarnessSignInNotice harnessId="account" />);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(sendDisabledReason()).toBeNull();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_001);
    fireEvent.focus(window);
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(sendDisabledReason()).toBeNull();
  expect(useWorkbenchStore.getState().selectedModelId).toBe("model");
  expect(screen.queryByText("Account harness isn't signed in")).toBeNull();
  const state = useWorkbenchStore.getState();
  const started = await acpDesktop.startSession({
    projectId: state.project!.id,
    projectGeneration: String(state.nativeTransitionEpoch),
    chatId: "chat",
    expectedProjectionRevision: "0",
    clientIntentId: "start-after-refocus",
    bindingId: "account-binding",
    modelId: state.selectedModelId,
  } as StartAcpSessionRequest);
  expect(started.projection.sessions[0].modelId).toBe("model");
});

it("can cancel an unfinished browser login and successfully retry", async () => {
  install();
  let abandon!: () => void;
  vi.spyOn(acpHarnessDesktop, "startCodexAuth")
    .mockImplementationOnce(
      (scope) =>
        new Promise((resolve) => {
          abandon = () =>
            resolve({ ...scope, phase: "signedOut", revision: "2" });
        }),
    )
    .mockImplementationOnce(async (scope) => ({
      ...scope,
      phase: "signedIn",
      revision: "3",
    }));
  vi.spyOn(acpHarnessDesktop, "cancelCodexAuth").mockImplementation(
    async (scope) => {
      if (
        scope.projectId !== "project" ||
        scope.bindingId !== "account-binding"
      )
        throw new Error("Foreign login");
      abandon();
      return { ...scope, phase: "waiting", revision: "2" };
    },
  );
  render(
    <>
      <ComposerHarnessSignInNotice harnessId="account" />
      <HarnessDiscoveryNotice />
    </>,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Sign in to Account harness" }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(
    await screen.findByRole("button", { name: "Sign in to Account harness" }),
  ).toBeEnabled();
  expect(sendDisabledReason()).toBe("Finish signing in before sending.");
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Sign in to Account harness" }),
    );
  });
  expect(sendDisabledReason()).toBeNull();
  expect(useWorkbenchStore.getState().selectedModelId).toBe("model");
});

it("does not restore an old chat's sign-out after signing in from another chat", async () => {
  install();
  vi.spyOn(acpHarnessDesktop, "startCodexAuth").mockImplementation(
    async (scope) => ({ ...scope, phase: "signedIn", revision: "2" }),
  );
  const failedChat = () => (
    <ComposerHarnessSignInNotice
      harnessId="account"
      recovery={{
        harnessId: "account",
        scope: "chat-a-failed-turn",
        onSignedIn: () => undefined,
      }}
    />
  );
  const chatA = render(failedChat());
  expect(
    await screen.findByText("Account harness isn't signed in"),
  ).toBeVisible();
  chatA.unmount();
  const chatB = render(<ComposerHarnessSignInNotice harnessId="account" />);
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Sign in to Account harness" }),
    );
  });
  expect(screen.queryByText("Account harness isn't signed in")).toBeNull();
  chatB.unmount();
  render(failedChat());
  expect(screen.queryByText("Account harness isn't signed in")).toBeNull();
  expect(sendDisabledReason()).toBeNull();
});

it.each(["project", "harness", "request"])(
  "ignores a discovery retry's sign-in after the %s scope changes",
  async (changed) => {
    install();
    let complete!: () => void;
    vi.mocked(acpHarnessDesktop.discoverCodex).mockImplementationOnce(
      (scope) =>
        new Promise((resolve) => {
          complete = () =>
            resolve({
              ...scope,
              rootId: "root",
              account: "managedChatgpt",
              models: [
                { id: "stale-model", name: "Stale model", isDefault: true },
              ],
            } as never);
        }),
    );
    const observing = refreshHarnessSignIn("account");
    if (changed === "project")
      useWorkbenchStore.setState({ nativeTransitionEpoch: 2 });
    if (changed === "harness")
      useWorkbenchStore.setState({ selectedHarnessId: null });
    if (changed === "request")
      useWorkbenchStore.setState({ harnessRequestGeneration: 2 });
    useWorkbenchStore.setState({
      harnessDiscoveryPhase: "signInRequired",
      models: [],
      selectedModelId: null,
    });
    complete();
    await observing;
    expect(useWorkbenchStore.getState().harnessDiscoveryPhase).toBe(
      "signInRequired",
    );
    expect(useWorkbenchStore.getState().selectedModelId).toBeNull();
  },
);

it("can cancel the original sidebar login after selecting another harness and retry", async () => {
  install();
  useWorkbenchStore.setState((state) => ({
    harnesses: [
      ...state.harnesses,
      {
        ...state.harnesses[0],
        id: "other",
        name: "Other harness",
        transport: "acp",
        installState: "available",
        bindingId: "other-binding",
        accountModelDiscovery: false,
      },
    ],
  }));
  vi.spyOn(desktop, "listHarnessModels").mockResolvedValue([]);
  let abandon!: () => void;
  vi.spyOn(acpHarnessDesktop, "discoverCodex").mockImplementation(
    async (scope) =>
      ({ ...scope, rootId: "root", account: "signedOut", models: [] }) as never,
  );
  vi.spyOn(acpHarnessDesktop, "startCodexAuth").mockImplementation(
    (scope) =>
      new Promise((resolve) => {
        abandon = () =>
          resolve({ ...scope, phase: "signedOut", revision: "2" });
      }),
  );
  vi.spyOn(acpHarnessDesktop, "cancelCodexAuth").mockImplementation(
    async (scope) => {
      if (scope.bindingId !== "account-binding")
        throw new Error("Foreign login");
      abandon();
      return { ...scope, phase: "waiting", revision: "2" };
    },
  );
  render(
    <>
      <ComposerHarnessSignInNotice harnessId="other" />
      <HarnessAccountMenu avatar="W" />
    </>,
  );
  await act(async () => {
    await refreshHarnessSignIn("account");
  });
  fireEvent.click(
    await screen.findByRole("button", { name: "Account: sign-in needed" }),
  );
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Sign in to Account harness" }),
  );
  await screen.findByRole("menuitem", {
    name: "Cancel sign-in to Account harness",
  });
  await act(async () => {
    await useWorkbenchStore.getState().selectHarness("other");
    fireEvent.focus(window);
  });
  fireEvent.click(
    await screen.findByRole("menuitem", {
      name: "Cancel sign-in to Account harness",
    }),
  );
  expect(
    await screen.findByRole("menuitem", { name: "Sign in to Account harness" }),
  ).toBeEnabled();
  vi.mocked(acpHarnessDesktop.startCodexAuth).mockImplementationOnce(
    async (scope) => ({ ...scope, phase: "signedIn", revision: "3" }),
  );
  vi.mocked(acpHarnessDesktop.discoverCodex).mockImplementation(
    async (scope) =>
      ({
        ...scope,
        rootId: "root",
        account: "managedChatgpt",
        models: [{ id: "model", name: "Model", isDefault: true }],
      }) as never,
  );
  await act(async () => {
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Sign in to Account harness" }),
    );
  });
  expect(useWorkbenchStore.getState().harnessDiscoveryPhase).toBe("ready");
  expect(screen.queryByRole("menu")).toBeNull();
});
