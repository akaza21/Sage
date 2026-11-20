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
import type { HarnessDefinition } from "../../domain/harness";
import { ComposerStatusNotices } from "../chat/ComposerStatusNotices";
import { HarnessAccountMenu } from "./HarnessSignInStatus";
import {
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
} from "./harnessSignIn";

let signedIn = false;
let statusFailure = false;
let expired = false;
let loginFailure = false;
vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));
vi.mock("../../services/acpDesktop", () => ({
  acpDesktop: {
    getClaudeAuthStatus: async () => {
      if (statusFailure) throw new Error("offline");
      return { loggedIn: signedIn };
    },
    startClaudeAuthLogin: async () => {
      if (loginFailure) throw new Error("failed");
      signedIn = true;
    },
  },
}));
vi.mock("../../services/acpHarnessDesktop", () => ({
  acpHarnessDesktop: {
    discoverCodex: async (scope: object) => ({
      ...scope,
      rootId: "root",
      account: signedIn ? "managedChatgpt" : "signedOut",
      models: [],
    }),
    startCodexAuth: async () => {
      signedIn = true;
      return { phase: "signedIn" };
    },
  },
}));
vi.mock("../../services/codexAuthStatusDesktop", () => ({
  codexAuthStatusDesktop: {
    getAuth: async () => {
      if (statusFailure) throw new Error("offline");
      return {
        phase: expired ? "recovery" : signedIn ? "signedIn" : "signedOut",
      };
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
  expired = false;
  loginFailure = false;
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
    harnessDiscoveryPhase: "idle",
    harnessAccount: null,
    retryHarnessDiscovery: async () => undefined,
  });
}
function notice(id: string) {
  return (
    <ComposerStatusNotices
      selectedHarnessId={id}
      workbenchError={null}
      chatError={null}
      imageStatus={null}
      missingBindingNotice={false}
      discoveryPhase="ready"
    />
  );
}
it.each(harnesses)(
  "$name uses the same composer notice and clears after login",
  async (harness) => {
    install(harness.id);
    render(notice(harness.id));
    expect(
      await screen.findByText(`${harness.name} isn't signed in`),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: `Sign in to ${harness.name}` }),
    );
    await act(async () => {
      await refreshHarnessSignIn(harness.id);
    });
    expect(screen.queryByText(`${harness.name} isn't signed in`)).toBeNull();
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
  install("cli-fixture");
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

it("uses Codex recovery as a sign-in problem", async () => {
  install("account-fixture");
  expired = true;
  render(notice("account-fixture"));
  expect(await screen.findByText("Codex isn't signed in")).toBeVisible();
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
      harnessDiscoveryPhase: "signInRequired",
    }),
  );
  expect(await screen.findByText("Codex isn't signed in")).toBeVisible();
  expect(
    screen.getByLabelText("Harness sign-in needs attention"),
  ).toBeVisible();
  act(() =>
    useWorkbenchStore.setState({
      harnessAccount: "apiKey",
      harnessDiscoveryPhase: "ready",
    }),
  );
  expect(screen.queryByText(/isn't signed in/)).toBeNull();
  expect(screen.queryByLabelText("Harness sign-in needs attention")).toBeNull();
});

it("reports a failed login with a retry action instead of discarding it", async () => {
  install("cli-fixture");
  loginFailure = true;
  render(notice("cli-fixture"));
  fireEvent.click(
    await screen.findByRole("button", { name: "Sign in to Claude Code" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Could not start sign-in.",
  );
  expect(
    screen.getByRole("button", { name: "Sign in to Claude Code" }),
  ).toBeEnabled();
});

it("keeps a newer expired-account fact across inventory refresh and view remount", async () => {
  install("account-fixture");
  signedIn = true;
  useWorkbenchStore.setState({
    harnessAccount: "managedChatgpt",
    harnessDiscoveryPhase: "ready",
  });
  const view = render(notice("account-fixture"));
  expired = true;
  await act(async () => {
    await refreshHarnessSignIn("account-fixture");
  });
  expect(await screen.findByText("Codex isn't signed in")).toBeVisible();
  act(() => useWorkbenchStore.setState({ harnesses: [...harnesses] }));
  expect(screen.getByText("Codex isn't signed in")).toBeVisible();
  view.unmount();
  render(notice("account-fixture"));
  expect(await screen.findByText("Codex isn't signed in")).toBeVisible();
});

it.each(harnesses)(
  "$name protocol auth failures override an older signed-in fact and recover through the shared model",
  async (harness) => {
    install(harness.id);
    signedIn = true;
    for (const entry of harnesses) await refreshHarnessSignIn(entry.id);
    function RecoveryComposer() {
      const [recovered, setRecovered] = useState(false);
      return (
        <>
          {recovered ? (
            <span>Ready to retry</span>
          ) : (
            <ComposerStatusNotices
              selectedHarnessId={harness.id}
              workbenchError={null}
              chatError={null}
              imageStatus={null}
              missingBindingNotice={false}
              discoveryPhase="ready"
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
