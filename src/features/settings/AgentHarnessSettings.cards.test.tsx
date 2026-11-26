import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { HarnessDefinition } from "../../domain/harness";
import {
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
} from "../harness/harnessSignIn";
import { AgentSettingsSurface } from "./AgentSettingsSurface";
import { useAgentSettingsState } from "./agentSettingsState";

const claude = vi.hoisted(() => ({ loggedIn: true, at: 100 }));
vi.mock("../../services/acpDesktop", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../services/acpDesktop")>();
  return {
    acpDesktop: {
      ...actual.acpDesktop,
      getClaudeAuthStatus: async () => ({
        loggedIn: claude.loggedIn,
        accountReadStartedAt: ++claude.at,
      }),
    },
  };
});

const workbench = useWorkbenchStore.getState();
const base = {
  description: "Discovered adapter",
  source: "builtIn",
  transport: "acp",
  accountModelDiscovery: false,
} as const;
const available = (
  id: string,
  name: string,
  extra: Partial<HarnessDefinition> = {},
) =>
  ({
    ...base,
    id,
    name,
    installState: "available",
    bindingId: `${id}-binding`,
    version: "1.0.0",
    ...extra,
  }) as HarnessDefinition;
const cliSignIn = {
  command: "cli auth login",
  hint: "Complete sign-in in the terminal.",
  statusCheck: "claudeCli",
} as const;

beforeEach(() => {
  resetHarnessSignInForTest();
  useAgentSettingsState.getState().open("Harnesses");
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectTransitioning: false,
    selectedHarnessId: null,
    harnessDiscoveryPhase: "ready",
    harnesses: [
      {
        ...base,
        id: "missing",
        name: "Missing",
        installState: "notInstalled",
        bindingId: null,
      },
      {
        ...base,
        id: "blocked",
        name: "Blocked",
        installState: "unavailable",
        bindingId: null,
        version: "0.1.0",
        unavailableReason: "nodeAbsent",
      },
      {
        ...base,
        id: "unavailable-binding:gone",
        name: "Unavailable harness",
        installState: "unavailable",
        bindingId: "gone",
        unavailableReason: "bindingMissing",
      },
      available("pi", "Pi"),
    ],
  });
});

afterEach(() => {
  cleanup();
  resetHarnessSignInForTest();
  useWorkbenchStore.setState(workbench, true);
});

const card = async (name: string) =>
  (await screen.findByRole("heading", { level: 3, name })).closest(
    "section",
  ) as HTMLElement;

it("gives each harness its own headed card with a text badge, runnable ones first", async () => {
  render(<AgentSettingsSurface />);
  const pi = await card("Pi");
  expect(within(pi).getByText("Available")).toBeVisible();
  expect(within(pi).getByText("v1.0.0")).toBeVisible();
  expect(
    within(pi).getByRole("button", { name: "Select and check" }),
  ).toBeVisible();
  const blocked = await card("Blocked");
  expect(within(blocked).getByText("Unavailable")).toBeVisible();
  expect(within(blocked).getByText("Blocked needs Node.js installed.")).toBeVisible();
  expect(within(blocked).queryByRole("button")).toBeNull();
  const missing = await card("Missing");
  expect(within(missing).getByText("Not installed")).toBeVisible();
  expect(
    screen
      .getAllByRole("heading", { level: 3 })
      .map((heading) => heading.textContent),
  ).toEqual(["Pi", "Blocked", "Missing"]);
  expect(
    screen.getAllByRole("group").map((group) => group.getAttribute("aria-label")),
  ).toEqual([
    "Available harnesses",
    "Unavailable harnesses",
    "Harnesses not installed",
  ]);
});

it("leaves out a Chat's missing-binding placeholder, which is no harness on this computer", async () => {
  render(<AgentSettingsSurface />);
  await card("Pi");
  expect(screen.queryByText("Unavailable harness")).toBeNull();
});

it("shows the native sign-in probe's answer and drops the login hint once signed in", async () => {
  useWorkbenchStore.setState({
    harnesses: [available("cli", "CLI agent", { signIn: cliSignIn })],
  });
  claude.loggedIn = true;
  await refreshHarnessSignIn("cli");
  render(<AgentSettingsSurface />);
  const signedIn = await card("CLI agent");
  expect(within(signedIn).getByText("Ready")).toBeVisible();
  expect(within(signedIn).getByText("Signed in.")).toBeVisible();
  expect(within(signedIn).queryByText(/not exposed/)).toBeNull();
  expect(within(signedIn).queryByText(/cli auth login/)).toBeNull();
});

it("marks a signed-out harness as needing sign-in and opens its login hint", async () => {
  useWorkbenchStore.setState({
    harnesses: [available("cli", "CLI agent", { signIn: cliSignIn })],
  });
  claude.loggedIn = false;
  await refreshHarnessSignIn("cli");
  render(<AgentSettingsSurface />);
  const signedOut = await card("CLI agent");
  expect(within(signedOut).getByText("Sign-in needed")).toBeVisible();
  expect(within(signedOut).getByText("Not signed in.")).toBeVisible();
  expect(
    within(signedOut).getByText(
      (_, node) =>
        node?.tagName === "P" &&
        node.textContent ===
          "Run cli auth login in a terminal. Complete sign-in in the terminal.",
    ),
  ).toBeVisible();
});

it("keeps a non-selected harness's details collapsed until asked", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      available("pi", "Pi", { signIn: { command: "pi", hint: "Then run /login." } }),
    ],
  });
  render(<AgentSettingsSurface />);
  const pi = await card("Pi");
  const hint = within(pi).getByText(
    (_, node) =>
      node?.tagName === "P" &&
      node.textContent === "Run pi in a terminal. Then run /login.",
  );
  expect(hint).not.toBeVisible();
  fireEvent.click(within(pi).getByText("Details"));
  expect(hint).toBeVisible();
});
