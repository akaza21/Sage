import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const desktop = vi.hoisted(() => ({
  getClaudeAuthStatus: vi.fn(),
  startClaudeAuthLogin: vi.fn(),
}));
vi.mock("../../services/acpDesktop", () => ({ acpDesktop: desktop }));
vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));

import { HarnessSignInPicker } from "./HarnessSignInPicker";
import {
  harnessSignInStatus,
  resetHarnessSignInForTest,
} from "./harnessSignIn";
import { settleLazyModules } from "../../test/settleLazyModules";

afterEach(() => {
  cleanup();
  resetHarnessSignInForTest();
  vi.clearAllMocks();
  vi.useRealTimers();
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

it("checks sign-in on opening and sleeps a pending login while the picker is closed", async () => {
  vi.useFakeTimers();
  let loggedIn = false;
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({ loggedIn }));
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  render(
    <HarnessSignInPicker
      ariaLabel="Harness"
      candidates={[{ id: "claude-code", label: "Claude Code" }]}
      harnesses={[
        { id: "claude-code", name: "Claude Code", installState: "available" },
      ]}
      selectedId="claude-code"
      onSelect={() => undefined}
    />,
  );
  await settleLazyModules();
  expect(harnessSignInStatus("claude-code").state).toBe("unknown");
  const trigger = screen.getByRole("button", { name: "Harness" });
  fireEvent.click(trigger);
  await settleLazyModules();
  expect(
    screen.getByRole("button", { name: "Sign in to Claude Code" }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "Sign in to Claude Code" }),
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  fireEvent.click(trigger);
  expect(screen.queryByRole("listbox")).toBeNull();
  loggedIn = true;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(harnessSignInStatus("claude-code").state).toBe("signedOut");
  fireEvent.click(trigger);
  await settleLazyModules();
  expect(screen.getByText("Signed in")).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Sign in to Claude Code" }),
  ).toBeNull();
});
