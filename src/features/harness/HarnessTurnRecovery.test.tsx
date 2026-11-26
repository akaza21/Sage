import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { acpDesktop } from "../../services/acpDesktop";
import { page } from "../chat/acp/acpProjection.fixture";
import {
  refreshHarnessSignIn,
  reportHarnessSignInRequired,
  resetHarnessSignInForTest,
} from "./harnessSignIn";
import { HarnessTurnFailureDetails } from "./HarnessTurnRecovery";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  resetHarnessSignInForTest();
  useWorkbenchStore.setState(initial, true);
  vi.restoreAllMocks();
});

it("keeps a refused turn's recovery hidden across remounts once a sign-in answered it", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "claude-code",
        name: "Claude Code",
        source: "builtIn",
        description: "",
        installState: "available",
        transport: "acp",
        bindingId: "fixture",
        accountModelDiscovery: false,
        signIn: { command: "", hint: "", statusCheck: "claudeCli" },
      },
    ],
  });
  const refused = page("failed", { kind: "authRequired" });
  refused.sessions[0].bindingId = "fixture" as never;
  refused.turns[0].id = "answered-turn" as never;
  const view = () => (
    <HarnessTurnFailureDetails turn={refused.turns[0]} page={refused} />
  );
  reportHarnessSignInRequired("claude-code", 200);
  const first = render(view());
  expect(screen.getByText("Claude Code isn't signed in")).toBeVisible();

  vi.spyOn(acpDesktop, "getClaudeAuthStatus").mockResolvedValue({
    loggedIn: true,
    accountReadStartedAt: 300,
  } as never);
  await act(() => refreshHarnessSignIn("claude-code"));
  expect(screen.queryByText("Claude Code isn't signed in")).toBeNull();
  first.unmount();

  // A later refusal signs the harness out again; this turn was answered.
  reportHarnessSignInRequired("claude-code", 400);
  render(view());
  expect(screen.queryByText(/isn't signed in/)).toBeNull();
});
