import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { CodexAuthScope } from "../../domain/acp";
import { loginCodexApiKey } from "../../services/codexApiKeyDesktop";
import { CodexAccountSettings } from "./CodexAccountSettings";

vi.mock("../../services/codexApiKeyDesktop", () => ({
  loginCodexApiKey: vi.fn(),
}));

const workbench = useWorkbenchStore.getState();

beforeEach(() => {
  useWorkbenchStore.setState({
    harnessDiscoveryPhase: "ready",
    harnessAccount: "managedChatgpt",
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(workbench, true);
});

it("sends a typed API key once, clears the field and keeps no copy in the store", () => {
  let call: ((scope: CodexAuthScope) => unknown) | undefined;
  useWorkbenchStore.setState({
    signInHarnessWith: async (next) => {
      call = next;
    },
  });
  render(<CodexAccountSettings disabled={false} />);
  expect(screen.getByText("Signed in with ChatGPT.")).toBeVisible();

  const field = screen.getByLabelText("OpenAI API key");
  expect(field).toHaveAttribute("type", "password");
  fireEvent.change(field, { target: { value: " sk-test-secret " } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

  expect(field).toHaveValue("");
  const scope = {} as CodexAuthScope;
  call?.(scope);
  expect(loginCodexApiKey).toHaveBeenCalledWith(scope, "sk-test-secret");
  expect(JSON.stringify(useWorkbenchStore.getState())).not.toContain(
    "sk-test-secret",
  );
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
