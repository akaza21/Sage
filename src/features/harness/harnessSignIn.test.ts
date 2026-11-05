import { afterEach, beforeEach, expect, it, vi } from "vitest";

const desktop = vi.hoisted(() => ({
  getClaudeAuthStatus: vi.fn(),
  startClaudeAuthLogin: vi.fn(),
}));
vi.mock("../../services/acpDesktop", () => ({ acpDesktop: desktop }));

import {
  ensureHarnessSignInChecked,
  harnessSignInAction,
  harnessSignInDecorations,
  harnessSignInLabel,
  harnessSignInStatus,
  harnessSupportsSignIn,
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
  startHarnessSignIn,
  subscribeHarnessSignIn,
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

it("checks visible harnesses once while their cached status is fresh", async () => {
  desktop.getClaudeAuthStatus.mockResolvedValue({ loggedIn: false });
  ensureHarnessSignInChecked(["claude-code", "codex"]);
  await Promise.resolve();
  await Promise.resolve();
  ensureHarnessSignInChecked(["claude-code", "codex"]);
  expect(desktop.getClaudeAuthStatus).toHaveBeenCalledOnce();
});

it("signs in through the harness's own flow and polls until signed in", async () => {
  vi.useFakeTimers();
  const unsubscribe = subscribeHarnessSignIn(() => undefined);
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
  unsubscribe();
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

it("keeps Sign in stable while polling, coalesces double clicks, and stops on picker cleanup", async () => {
  vi.useFakeTimers();
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  desktop.getClaudeAuthStatus.mockResolvedValue({ loggedIn: false });
  await refreshHarnessSignIn("claude-code");
  const observed: string[] = [];
  const unsubscribe = subscribeHarnessSignIn(() =>
    observed.push(harnessSignInStatus("claude-code").state),
  );
  await Promise.all([
    startHarnessSignIn("claude-code"),
    startHarnessSignIn("claude-code"),
  ]);
  expect(desktop.startClaudeAuthLogin).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(3000);
  expect(observed).not.toContain("checking");
  unsubscribe();
  await vi.advanceTimersByTimeAsync(120000);
  expect(desktop.getClaudeAuthStatus).toHaveBeenCalledTimes(2);
});
it("does not let a stale refresh replace a later check", async () => {
  let complete!: (value: { loggedIn: boolean }) => void;
  desktop.getClaudeAuthStatus
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    )
    .mockResolvedValue({ loggedIn: true });
  const older = refreshHarnessSignIn("claude-code");
  await refreshHarnessSignIn("claude-code");
  complete({ loggedIn: false });
  await older;
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
});

it("preserves the bounded login check budget across repeated suspension and wake", async () => {
  vi.useFakeTimers();
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  desktop.getClaudeAuthStatus.mockResolvedValue({ loggedIn: false });
  await refreshHarnessSignIn("claude-code");
  const observed: string[] = [];
  const observe = () => observed.push(harnessSignInStatus("claude-code").state);
  let unsubscribe = subscribeHarnessSignIn(observe);
  await startHarnessSignIn("claude-code");
  await vi.advanceTimersByTimeAsync(3000);
  for (let attempt = 1; attempt < 40; attempt += 1) {
    unsubscribe();
    await vi.advanceTimersByTimeAsync(3000);
    expect(observed).toHaveLength(attempt);
    unsubscribe = subscribeHarnessSignIn(observe);
    await vi.advanceTimersByTimeAsync(0);
  }
  expect(observed).toHaveLength(40);
  unsubscribe();
  unsubscribe = subscribeHarnessSignIn(observe);
  await vi.advanceTimersByTimeAsync(120_000);
  expect(observed).toHaveLength(40);
  unsubscribe();
});

it("keeps a newer sign-in check started by a status notification", async () => {
  desktop.getClaudeAuthStatus
    .mockResolvedValueOnce({ loggedIn: false })
    .mockResolvedValue({ loggedIn: true });
  let newer: Promise<unknown> | undefined;
  const unsubscribe = subscribeHarnessSignIn(() => {
    if (harnessSignInStatus("claude-code").state === "signedOut" && !newer)
      newer = refreshHarnessSignIn("claude-code");
  });
  await refreshHarnessSignIn("claude-code");
  await newer;
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
  unsubscribe();
});

it("waits for a visible picker before checking a login started without subscribers", async () => {
  vi.useFakeTimers();
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  desktop.getClaudeAuthStatus.mockResolvedValue({ loggedIn: true });
  await startHarnessSignIn("claude-code");
  await vi.advanceTimersByTimeAsync(120_000);
  expect(harnessSignInStatus("claude-code").state).toBe("unknown");
  const unsubscribe = subscribeHarnessSignIn(() => undefined);
  await vi.advanceTimersByTimeAsync(0);
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
  unsubscribe();
});
