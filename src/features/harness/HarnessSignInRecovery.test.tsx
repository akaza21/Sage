import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { acpHarnessDesktop } from "../../services/acpHarnessDesktop";
import { codexAuthStatusDesktop } from "../../services/codexAuthStatusDesktop";
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
  vi.spyOn(codexAuthStatusDesktop, "getAuth").mockImplementation(
    async (scope) => ({ ...scope, phase: "signedOut", revision: "1" }),
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

it("refocusing after an external sign-in refreshes models and makes sending available", async () => {
  install();
  vi.useFakeTimers();
  render(<ComposerHarnessSignInNotice harnessId="account" />);
  await act(async () => {
    await refreshHarnessSignIn("account");
  });
  expect(sendDisabledReason()).toBe("Finish signing in before sending.");
  vi.mocked(codexAuthStatusDesktop.getAuth).mockImplementation(
    async (scope) => ({ ...scope, phase: "signedIn", revision: "2" }),
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_001);
    fireEvent.focus(window);
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(sendDisabledReason()).toBeNull();
  expect(useWorkbenchStore.getState().selectedModelId).toBe("model");
  expect(screen.queryByText("Account harness isn't signed in")).toBeNull();
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

it.each(["project", "harness", "request"])(
  "ignores an observed sign-in after the %s scope changes",
  async (changed) => {
    install();
    let complete!: () => void;
    vi.mocked(codexAuthStatusDesktop.getAuth).mockImplementationOnce(
      (scope) =>
        new Promise((resolve) => {
          complete = () =>
            resolve({ ...scope, phase: "signedIn", revision: "2" });
        }),
    );
    const observing = refreshHarnessSignIn("account");
    if (changed === "project")
      useWorkbenchStore.setState({ nativeTransitionEpoch: 2 });
    if (changed === "harness")
      useWorkbenchStore.setState({ selectedHarnessId: null });
    if (changed === "request")
      useWorkbenchStore.setState({ harnessRequestGeneration: 2 });
    complete();
    await observing;
    expect(useWorkbenchStore.getState().harnessDiscoveryPhase).toBe(
      "signInRequired",
    );
    expect(useWorkbenchStore.getState().selectedModelId).toBeNull();
  },
);

it("can cancel and retry a sidebar login while a Chat owns another harness", async () => {
  install();
  useWorkbenchStore.setState({
    selectedHarnessId: "other",
    harnessDiscoveryPhase: "ready",
    harnessAccount: null,
  });
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
  fireEvent.click(
    await screen.findByRole("button", { name: "Account: sign-in needed" }),
  );
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Sign in to Account harness" }),
  );
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
  vi.mocked(acpHarnessDesktop.discoverCodex).mockImplementationOnce(
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
