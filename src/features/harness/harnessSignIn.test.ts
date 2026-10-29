import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const desktop = vi.hoisted(() => ({
  getClaudeAuthStatus: vi.fn(),
  startClaudeAuthLogin: vi.fn(),
}));
vi.mock("../../services/acpDesktop", () => ({ acpDesktop: desktop }));

import {
  harnessSignInAction,
  harnessSignInDecorations,
  harnessSignInLabel,
  harnessSignInStatus,
  harnessSupportsSignIn,
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
  startHarnessSignIn,
  useHarnessSignInStatuses,
} from "./harnessSignIn";

beforeEach(() => {
  desktop.getClaudeAuthStatus.mockReset();
  desktop.startClaudeAuthLogin.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
  resetHarnessSignInForTest();
});

it("only curated harnesses have a sign-in status", async () => {
  expect(harnessSupportsSignIn("claude-code")).toBe(true);
  expect(harnessSupportsSignIn("pi")).toBe(false);
  expect(harnessSignInStatus("pi").state).toBe("unsupported");
  expect(await refreshHarnessSignIn("pi")).toBe("unsupported");
  expect(desktop.getClaudeAuthStatus).not.toHaveBeenCalled();
});

it("reports signed out, signed in and an unavailable check", async () => {
  desktop.getClaudeAuthStatus.mockResolvedValueOnce({ loggedIn: false });
  expect(await refreshHarnessSignIn("claude-code")).toBe("signedOut");
  expect(harnessSignInLabel("signedOut")).toMatch(/Signed out/);
  desktop.getClaudeAuthStatus.mockResolvedValueOnce({ loggedIn: true });
  expect(await refreshHarnessSignIn("claude-code")).toBe("signedIn");
  desktop.getClaudeAuthStatus.mockRejectedValueOnce(new Error("timeout"));
  expect(await refreshHarnessSignIn("claude-code")).toBe("unavailable");
  expect(harnessSignInStatus("claude-code").checkedAt).not.toBeNull();
});

it("checks visible harnesses once and exposes their status to the picker", async () => {
  desktop.getClaudeAuthStatus.mockResolvedValue({ loggedIn: false });
  const { result, rerender } = renderHook(
    ({ ids }: { ids: string[] }) => useHarnessSignInStatuses(ids),
    { initialProps: { ids: ["claude-code", "codex"] } },
  );
  await act(async () => {});
  expect(result.current.get("claude-code")?.state).toBe("signedOut");
  expect(result.current.has("codex")).toBe(false);
  rerender({ ids: ["claude-code", "codex"] });
  await act(async () => {});
  expect(desktop.getClaudeAuthStatus).toHaveBeenCalledTimes(1);
});

it("signs in through the harness's own flow and polls until signed in", async () => {
  vi.useFakeTimers();
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  desktop.getClaudeAuthStatus
    .mockResolvedValueOnce({ loggedIn: false })
    .mockResolvedValueOnce({ loggedIn: true });
  const action = harnessSignInAction("claude-code", "Claude Code", "signedOut");
  expect(action?.label).toBe("Sign in");
  expect(action?.accessibleLabel).toBe("Sign in to Claude Code");
  await startHarnessSignIn("claude-code");
  expect(desktop.startClaudeAuthLogin).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(3_000);
  expect(harnessSignInStatus("claude-code").state).toBe("signedOut");
  await vi.advanceTimersByTimeAsync(3_000);
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
  await vi.advanceTimersByTimeAsync(30_000);
  expect(desktop.getClaudeAuthStatus).toHaveBeenCalledTimes(2);
  expect(harnessSignInAction("claude-code", "Claude Code", "signedIn")).toBe(
    undefined,
  );
});

it("decorates picker rows with the sign-in label and action", async () => {
  desktop.getClaudeAuthStatus.mockResolvedValue({ loggedIn: false });
  await refreshHarnessSignIn("claude-code");
  const decorations = harnessSignInDecorations([
    { id: "claude-code", name: "Claude Code" },
    { id: "pi", name: "Pi" },
  ]);
  expect([...decorations.keys()]).toEqual(["claude-code"]);
  expect(decorations.get("claude-code")?.label).toMatch(/Signed out/);
  expect(decorations.get("claude-code")?.action?.label).toBe("Sign in");
});
