import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const desktop = vi.hoisted(() => ({
  getClaudeAuthStatus: vi.fn(),
  startClaudeAuthLogin: vi.fn(),
}));
vi.mock("../../services/acpDesktop", () => ({ acpDesktop: desktop }));
vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));

import { HarnessSignInPicker } from "./HarnessSignInPicker";
import { resetHarnessSignInForTest } from "./harnessSignIn";

afterEach(() => {
  cleanup();
  resetHarnessSignInForTest();
  vi.clearAllMocks();
});

it("keeps the picker synchronous while loading sign-in decorations lazily", async () => {
  desktop.getClaudeAuthStatus.mockResolvedValue({ loggedIn: false });
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  const select = vi.fn();
  render(
    <HarnessSignInPicker
      ariaLabel="Harness"
      candidates={[
        {
          id: "claude-code",
          label: "Claude Code",
          subtitle: "Sonnet",
          accessibleLabel: "Use Claude Code",
        },
        { id: "pi", label: "Pi" },
      ]}
      harnesses={[
        { id: "claude-code", name: "Claude Code", installState: "available" },
        { id: "pi", name: "Pi", installState: "available" },
      ]}
      selectedId="claude-code"
      onSelect={select}
      open
    />,
  );
  expect(screen.getByRole("button", { name: "Harness" })).toBeInTheDocument();
  const action = await screen.findByRole("button", {
    name: "Sign in to Claude Code",
  });
  expect(screen.getByText("Sonnet \u00b7 Signed out")).toBeVisible();
  fireEvent.click(action);
  expect(desktop.startClaudeAuthLogin).toHaveBeenCalledTimes(1);
  expect(select).not.toHaveBeenCalled();
  expect(desktop.getClaudeAuthStatus).toHaveBeenCalledTimes(1);
});
