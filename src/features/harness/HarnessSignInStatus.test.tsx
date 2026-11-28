import { composerProblem } from "../chat/ComposerStatusNotices";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { acpDesktop } from "../../services/acpDesktop";
import { acpHarnessDesktop } from "../../services/acpHarnessDesktop";
import { desktop } from "../../services/desktop";
import type { HarnessDefinition } from "../../domain/harness";
import type * as AcpHarnessDesktopModule from "../../services/acpHarnessDesktop";
import { ComposerStatusNotices } from "../chat/ComposerStatusNotices";
import { HarnessAccountMenu } from "./HarnessSignInStatus";
import {
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
  harnessSignInStatus,
  reportHarnessSignInRequired,
} from "./harnessSignIn";

let nativeTime = 100;

let signedIn = false;
let statusFailure = false;
let loginFailure = false;
vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));
vi.mock("../../services/acpDesktop", () => ({
  acpDesktop: {
    getClaudeAuthStatus: async () => {
      if (statusFailure) throw new Error("offline");
      return { loggedIn: signedIn, accountReadStartedAt: ++nativeTime };
    },
    startClaudeAuthLogin: async () => {
      if (loginFailure) throw new Error("failed");
      signedIn = true;
    },
  },
}));
vi.mock("../../services/acpHarnessDesktop", async (importOriginal) => ({
  ...(await importOriginal<typeof AcpHarnessDesktopModule>()),
  acpHarnessDesktop: {
    discoverCodex: async (scope: object) => ({
      ...scope,
      rootId: "root",
      account: signedIn ? "managedChatgpt" : "signedOut",
      accountReadStartedAt: ++nativeTime,
      models: [],
    }),
    startCodexAuth: async () => {
      signedIn = true;
      return { phase: "signedIn" };
    },
  },
}));
const initial = useWorkbenchStore.getState();
const harnesses: HarnessDefinition[] = [
  {
    id: "cli-fixture",
    name: "Claude Code",
    source: "builtIn",
    description: "",
    installState: "available",
    transport: "acp",
    bindingId: "cli",
    accountModelDiscovery: false,
    signIn: { command: "", hint: "", statusCheck: "claudeCli" },
  },
  {
    id: "account-fixture",
    name: "Codex",
    source: "builtIn",
    description: "",
    installState: "available",
    transport: "acp",
    bindingId: "account",
    accountModelDiscovery: true,
  },
];
afterEach(() => {
  cleanup();
  resetHarnessSignInForTest();
  useWorkbenchStore.setState(initial, true);
  signedIn = false;
  statusFailure = false;
  loginFailure = false;
  vi.restoreAllMocks();
  vi.useRealTimers();
});
function install(id: string) {
  useWorkbenchStore.setState({
    harnesses,
    selectedHarnessId: id,
    project: {
      id: "workspace",
      name: "Workspace",
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
    harnessDiscoveryPhase: id === "account-fixture" ? "signInRequired" : "idle",
    harnessAccount: id === "account-fixture" ? "signedOut" : null,
    harnessAccountReadStartedAt: ++nativeTime,
  });
}
function notice(_id: string) {
  return (
    <ComposerStatusNotices
      workbenchError={null}
      chatError={null}
      imageStatus={null}
    />
  );
}
it("rechecks signed-out Claude on return within the cache TTL", async () => {
  install("cli-fixture");
  render(<HarnessAccountMenu avatar="W" />);
  expect(
    await screen.findByRole("button", { name: "Account: sign-in needed" }),
  ).toBeVisible();
  vi.useFakeTimers();
  signedIn = true;
  let complete!: (value: {
    loggedIn: boolean;
    accountReadStartedAt: number;
  }) => void;
  vi.spyOn(acpDesktop, "getClaudeAuthStatus")
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    )
    .mockImplementation(async () => ({
      loggedIn: false,
      accountReadStartedAt: ++nativeTime,
    }));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1_000);
    fireEvent.focus(window);
    complete({ loggedIn: true, accountReadStartedAt: ++nativeTime });
    fireEvent.focus(window);
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(harnessSignInStatus("cli-fixture").state).toBe("signedIn");
  expect(screen.queryByText("Claude Code isn't signed in")).toBeNull();
});
it("offers one sign-in row per harness and shows its focus recheck inline", async () => {
  install("account-fixture");
  await act(async () => {
    await refreshHarnessSignIn("cli-fixture");
  });
  render(<HarnessAccountMenu avatar="W" />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Account: sign-in needed" }),
  );
  await screen.findByRole("menu");
  expect(
    screen.getAllByRole("menuitem").map((item) => item.textContent),
  ).toEqual(["Claude CodeSign in", "CodexSign in"]);
  let complete!: (value: {
    loggedIn: boolean;
    accountReadStartedAt: number;
  }) => void;
  vi.spyOn(acpDesktop, "getClaudeAuthStatus").mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      }),
  );
  act(() => {
    fireEvent.focus(window);
  });
  const claude = screen.getByRole("menuitem", {
    name: "Sign in to Claude Code",
  });
  expect(claude).toHaveTextContent("Checking…");
  expect(claude).toBeDisabled();
  await act(async () => {
    complete({ loggedIn: true, accountReadStartedAt: ++nativeTime });
  });
  expect(
    screen.queryByRole("menuitem", { name: "Sign in to Claude Code" }),
  ).toBeNull();
  expect(
    screen.getByRole("menuitem", { name: "Sign in to Codex" }),
  ).toHaveTextContent("Sign in");
});
it.each(harnesses)(
  "$name can sign in from the account menu",
  async (harness) => {
    install(harness.id);
    render(<HarnessAccountMenu avatar="W" />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Account: sign-in needed" }),
    );
    expect(
      await screen.findByRole("menuitem", {
        name: `Sign in to ${harness.name}`,
      }),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("menuitem", { name: `Sign in to ${harness.name}` }),
    );
    await act(async () => {
      await refreshHarnessSignIn(harness.id);
    });
    expect(
      screen.queryByRole("menuitem", { name: `Sign in to ${harness.name}` }),
    ).toBeNull();
  },
);
it.each(harnesses)("$name stays silent when signed in", async (harness) => {
  install(harness.id);
  signedIn = true;
  await refreshHarnessSignIn(harness.id);
  render(notice(harness.id));
  expect(screen.queryByRole("button", { name: /Sign in/ })).toBeNull();
  expect(screen.queryByText(/isn't signed in/)).toBeNull();
});
it("badges the account only while an installed harness needs sign-in and offers each login", async () => {
  install("account-fixture");
  render(<HarnessAccountMenu avatar="W" />);
  const trigger = await screen.findByRole("button", {
    name: "Account: sign-in needed",
  });
  expect(
    screen.getByLabelText("Harness sign-in needs attention"),
  ).toBeVisible();
  fireEvent.click(trigger);
  expect(
    await screen.findByRole("menuitem", { name: "Sign in to Claude Code" }),
  ).toBeVisible();
  expect(
    screen.getByRole("menuitem", { name: "Sign in to Codex" }),
  ).toBeVisible();
  await act(async () => {
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign in to Codex" }));
  });
  await act(async () => {
    for (const harness of harnesses) await refreshHarnessSignIn(harness.id);
  });
  expect(screen.queryByLabelText("Harness sign-in needs attention")).toBeNull();
  expect(screen.queryByRole("menu")).toBeNull();
  signedIn = false;
  await act(async () => {
    await refreshHarnessSignIn("cli-fixture");
  });
  expect(
    screen.getByRole("button", { name: "Account: sign-in needed" }),
  ).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("menu")).toBeNull();
});
it("unknown, unsupported and uninstalled harnesses stay quiet", async () => {
  install("unknown");
  useWorkbenchStore.setState({
    harnesses: harnesses.map((h) => ({ ...h, installState: "notInstalled" })),
  });
  render(
    <>
      {notice("unknown")}
      <HarnessAccountMenu avatar="W" />
    </>,
  );
  expect(screen.queryByText(/isn't signed in/)).toBeNull();
  expect(screen.queryByLabelText("Harness sign-in needs attention")).toBeNull();
});

it("keeps cached sign-in problems out of the composer and location area", async () => {
  install("account-fixture");
  await refreshHarnessSignIn("account-fixture");
  const view = render(notice("account-fixture"));
  await act(async () => {
    await import("../chat/ComposerAddActions");
  });
  expect(view.container).toBeEmptyDOMElement();
  expect(screen.queryByText("Codex isn't signed in")).toBeNull();
});

it("keeps failed and unknown checks silent", async () => {
  install("cli-fixture");
  statusFailure = true;
  await refreshHarnessSignIn("cli-fixture");
  await refreshHarnessSignIn("account-fixture");
  render(
    <>
      {notice("cli-fixture")}
      <HarnessAccountMenu avatar="W" />
    </>,
  );
  expect(screen.queryByText(/isn't signed in/)).toBeNull();
  expect(screen.queryByLabelText("Harness sign-in needs attention")).toBeNull();
});

it("updates the account badge from Settings account state without changing harness selection", async () => {
  install("account-fixture");
  signedIn = true;
  useWorkbenchStore.setState({
    harnessAccount: "managedChatgpt",
    harnessAccountReadStartedAt: ++nativeTime,
    harnessDiscoveryPhase: "ready",
  });
  await refreshHarnessSignIn("cli-fixture");
  render(
    <>
      {notice("account-fixture")}
      <HarnessAccountMenu avatar="W" />
    </>,
  );
  expect(screen.queryByLabelText("Harness sign-in needs attention")).toBeNull();
  act(() =>
    useWorkbenchStore.setState({
      harnessAccount: "signedOut",
      harnessAccountReadStartedAt: ++nativeTime,
      harnessDiscoveryPhase: "signInRequired",
    }),
  );
  expect(screen.queryByText("Codex isn't signed in")).toBeNull();
  expect(
    screen.getByLabelText("Harness sign-in needs attention"),
  ).toBeVisible();
  await act(async () => {
    vi.spyOn(acpHarnessDesktop, "discoverCodex").mockImplementationOnce(
      async (scope) =>
        ({
          ...scope,
          rootId: "root",
          account: "apiKey",
          accountReadStartedAt: ++nativeTime,
          models: [],
        }) as never,
    );
    await useWorkbenchStore.getState().retryHarnessDiscovery();
  });
  expect(screen.queryByText(/isn't signed in/)).toBeNull();
  expect(screen.queryByLabelText("Harness sign-in needs attention")).toBeNull();
});

it("reports a failed login with a retry action instead of discarding it", async () => {
  install("cli-fixture");
  loginFailure = true;
  render(<HarnessAccountMenu avatar="W" />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Account: sign-in needed" }),
  );
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Sign in to Claude Code" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Could not start sign-in.",
  );
  expect(
    screen.getByRole("menuitem", { name: "Sign in to Claude Code" }),
  ).toBeEnabled();
});

it("keeps a failed Claude turn's recovery visible after selecting signed-in Codex", async () => {
  install("account-fixture");
  signedIn = true;
  await refreshHarnessSignIn("account-fixture");
  useWorkbenchStore.setState({
    selectedHarnessId: "cli-fixture",
    harnessAccount: null,
  });
  await refreshHarnessSignIn("cli-fixture");
  reportHarnessSignInRequired("cli-fixture", ++nativeTime);
  const view = render(
    <ComposerStatusNotices
      workbenchError={null}
      chatError={composerProblem(
        "signInRequired",
        "failed-turn",
        "Authentication required",
      )}
      signInHarness={harnesses[0]}
      imageStatus={null}
      authRecovery={{
        harnessId: "cli-fixture",
        scope: "failed-turn",
        onSignedIn: () => undefined,
      }}
    />,
  );
  expect(await screen.findByText("Claude Code isn't signed in")).toBeVisible();
  await act(async () => {
    await useWorkbenchStore.getState().selectHarness("account-fixture");
  });
  view.rerender(
    <ComposerStatusNotices
      workbenchError={null}
      chatError={composerProblem(
        "signInRequired",
        "failed-turn",
        "Authentication required",
      )}
      signInHarness={harnesses[0]}
      imageStatus={null}
      authRecovery={{
        harnessId: "cli-fixture",
        scope: "failed-turn",
        onSignedIn: () => undefined,
      }}
    />,
  );
  expect(harnessSignInStatus("account-fixture").state).toBe("signedIn");
  expect(screen.getByText("Claude Code isn't signed in")).toBeVisible();
  expect(
    screen.getByRole("button", { name: "Sign in to Claude Code" }),
  ).toBeEnabled();
  expect(screen.getByRole("button", { name: "Check sign-in" })).toBeEnabled();
});

it("requires selecting Codex before checking its retained recovery after external login", async () => {
  install("account-fixture");
  vi.spyOn(desktop, "listHarnessModels").mockResolvedValue([]);
  vi.spyOn(acpDesktop, "getClaudeAuthStatus").mockResolvedValue({
    loggedIn: true,
    accountReadStartedAt: ++nativeTime,
  });
  signedIn = true;
  // Native selection keeps returning the snapshot cached before the refusal.
  const cached = await acpHarnessDesktop.discoverCodex({
    projectId: "workspace",
    projectGeneration: String(
      useWorkbenchStore.getState().nativeTransitionEpoch,
    ),
    bindingId: "account",
  } as Parameters<typeof acpHarnessDesktop.discoverCodex>[0]);
  vi.spyOn(acpHarnessDesktop, "discoverCodex").mockResolvedValue(cached);
  await refreshHarnessSignIn("account-fixture");
  reportHarnessSignInRequired("account-fixture", ++nativeTime);
  signedIn = false;
  function RecoveryComposer() {
    useWorkbenchStore((state) => state.selectedHarnessId);
    const [recovered, setRecovered] = useState(false);
    return (
      <>
        {recovered ? (
          <span>Ready to retry</span>
        ) : (
          <ComposerStatusNotices
            workbenchError={null}
            chatError={null}
            signInHarness={harnesses[1]}
            imageStatus={null}
            authRecovery={{
              harnessId: "account-fixture",
              scope: "failed-turn",
              onSignedIn: () => setRecovered(true),
            }}
          />
        )}
        <HarnessAccountMenu avatar="W" />
      </>
    );
  }
  render(<RecoveryComposer />);
  expect(await screen.findByText("Codex isn't signed in")).toBeVisible();
  await act(async () => {
    await useWorkbenchStore.getState().selectHarness("cli-fixture");
  });
  expect(screen.getByRole("button", { name: "Check sign-in" })).toBeDisabled();
  expect(screen.getByText("Select Codex to check sign-in.")).toBeVisible();
  signedIn = true;
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
  });
  expect(screen.getByText("Codex isn't signed in")).toBeVisible();
  expect(
    screen.getByLabelText("Harness sign-in needs attention"),
  ).toBeVisible();
  expect(screen.queryByText("Ready to retry")).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  await act(async () => {
    await useWorkbenchStore.getState().selectHarness("account-fixture");
  });
  expect(screen.getByText("Codex isn't signed in")).toBeVisible();
  expect(screen.queryByText("Ready to retry")).toBeNull();
  vi.mocked(acpHarnessDesktop.discoverCodex).mockImplementationOnce(
    async () => ({ ...cached, accountReadStartedAt: ++nativeTime }),
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
  });
  expect(await screen.findByText("Ready to retry")).toBeVisible();
  expect(screen.queryByLabelText("Harness sign-in needs attention")).toBeNull();
});

it("keeps a newer expired-account fact across inventory refresh and view remount", async () => {
  install("account-fixture");
  signedIn = true;
  useWorkbenchStore.setState({
    harnessAccount: "managedChatgpt",
    harnessAccountReadStartedAt: ++nativeTime,
    harnessDiscoveryPhase: "ready",
  });
  const view = render(<HarnessAccountMenu avatar="W" />);
  act(() =>
    useWorkbenchStore.setState({
      harnessAccount: "signedOut",
      harnessAccountReadStartedAt: ++nativeTime,
      harnessDiscoveryPhase: "signInRequired",
    }),
  );
  expect(
    await screen.findByRole("button", { name: "Account: sign-in needed" }),
  ).toBeVisible();
  act(() => useWorkbenchStore.setState({ harnesses: [...harnesses] }));
  expect(
    screen.getByRole("button", { name: "Account: sign-in needed" }),
  ).toBeVisible();
  view.unmount();
  render(<HarnessAccountMenu avatar="W" />);
  expect(
    await screen.findByRole("button", { name: "Account: sign-in needed" }),
  ).toBeVisible();
});

it.each(harnesses)(
  "$name protocol auth failures override an older signed-in fact and recover through the shared model",
  async (harness) => {
    install(harness.id);
    signedIn = true;
    for (const entry of harnesses) await refreshHarnessSignIn(entry.id);
    reportHarnessSignInRequired(harness.id, ++nativeTime);
    function RecoveryComposer() {
      const [recovered, setRecovered] = useState(false);
      return (
        <>
          {recovered ? (
            <span>Ready to retry</span>
          ) : (
            <ComposerStatusNotices
              workbenchError={null}
              chatError={null}
              imageStatus={null}
              authRecovery={{
                harnessId: harness.id,
                scope: "failed-turn",
                onSignedIn: () => setRecovered(true),
              }}
            />
          )}
          <HarnessAccountMenu avatar="W" />
        </>
      );
    }
    render(<RecoveryComposer />);
    expect(
      await screen.findByText(`${harness.name} isn't signed in`),
    ).toBeVisible();
    expect(
      screen.getByLabelText("Harness sign-in needs attention"),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
    expect(await screen.findByText("Ready to retry")).toBeVisible();
    expect(
      screen.queryByLabelText("Harness sign-in needs attention"),
    ).toBeNull();
  },
);
