import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { CodexAuthPhase, CodexAuthScope } from "../../domain/acp";
import { loginCodexApiKey } from "../../services/codexApiKeyDesktop";
import { CodexAccountSettings } from "./CodexAccountSettings";

vi.mock("../../services/codexApiKeyDesktop", () => ({
  loginCodexApiKey: vi.fn(),
}));

const workbench = useWorkbenchStore.getState();
const login = vi.mocked(loginCodexApiKey);
const retryHarnessDiscovery = vi.fn(async () => undefined);
const scope = {
  projectId: "project-a",
  projectGeneration: "0",
  bindingId: "codex-binding",
} as CodexAuthScope;

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    nativeTransitionEpoch: 0,
    harnesses: [
      {
        id: "codex",
        name: "Codex",
        description: "ACP",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId: "codex-binding",
        accountModelDiscovery: true,
      },
    ],
    selectedHarnessId: "codex",
    harnessDiscoveryPhase: "ready",
    harnessAccount: "managedChatgpt",
    retryHarnessDiscovery,
  });
});

afterEach(() => {
  cleanup();
  login.mockReset();
  retryHarnessDiscovery.mockClear();
  useWorkbenchStore.setState(workbench, true);
});

function answer(phase: CodexAuthPhase) {
  login.mockResolvedValue({ ...scope, phase, revision: "2" });
}

function submitKey() {
  const field = screen.getByLabelText("OpenAI API key");
  expect(field).toHaveAttribute("type", "password");
  fireEvent.change(field, { target: { value: " sk-test-secret " } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
  return field;
}

it("sends a typed API key once, rediscovers the account and keeps no copy", async () => {
  answer("signedIn");
  render(<CodexAccountSettings disabled={false} />);
  expect(screen.getByText("Signed in with ChatGPT.")).toBeVisible();

  const field = submitKey();
  expect(field).toHaveValue("");
  await waitFor(() => expect(retryHarnessDiscovery).toHaveBeenCalledOnce());
  expect(login).toHaveBeenCalledOnce();
  expect(login).toHaveBeenCalledWith(scope, "sk-test-secret");
  expect(JSON.stringify(useWorkbenchStore.getState())).not.toContain(
    "sk-test-secret",
  );
});

it("reports a sign-in that does not converge instead of rediscovering", async () => {
  answer("signedOut");
  render(<CodexAccountSettings disabled={false} />);
  submitKey();
  await waitFor(() =>
    expect(useWorkbenchStore.getState().harnessDiscoveryPhase).toBe("error"),
  );
  expect(useWorkbenchStore.getState().harnessDiscoveryError).toBe(
    "The Codex account change did not converge safely.",
  );
  expect(useWorkbenchStore.getState().harnessAccount).toBeNull();
  expect(retryHarnessDiscovery).not.toHaveBeenCalled();
});

it("offers sign out only for an account the harness signed in", () => {
  useWorkbenchStore.setState({ harnessAccount: "providerManaged" });
  const { rerender } = render(<CodexAccountSettings disabled={false} />);
  expect(screen.getByRole("button", { name: "Sign out" })).toBeDisabled();

  useWorkbenchStore.setState({ harnessAccount: "apiKey" });
  rerender(<CodexAccountSettings disabled={false} />);
  expect(screen.getByText("Signed in with an OpenAI API key.")).toBeVisible();
  expect(screen.getByRole("button", { name: "Sign out" })).toBeEnabled();
});
