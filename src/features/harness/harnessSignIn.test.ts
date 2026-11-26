import { useWorkbenchStore } from "../../app/store";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type * as AcpHarnessDesktopModule from "../../services/acpHarnessDesktop";
import { harnessSignInFailures } from "../../services/acpHarnessDesktop";

const desktop = vi.hoisted(() => ({
  getClaudeAuthStatus: vi.fn(),
  startClaudeAuthLogin: vi.fn(),
  discoverCodex: vi.fn(),
}));
vi.mock("../../services/acpDesktop", () => ({ acpDesktop: desktop }));
vi.mock("../../services/acpHarnessDesktop", async (importOriginal) => ({
  ...(await importOriginal<typeof AcpHarnessDesktopModule>()),
  acpHarnessDesktop: desktop,
}));

import {
  ensureHarnessSignInChecked,
  harnessesNeedingSignIn,
  harnessSignInStatus,
  harnessSupportsSignIn,
  refreshHarnessSignIn,
  reportHarnessSignInRequired,
  resetHarnessSignInForTest,
  startHarnessSignIn,
  subscribeHarnessSignIn,
} from "./harnessSignIn";

let nativeTime = 100;

const initial = useWorkbenchStore.getState();
beforeEach(() => {
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
  desktop.getClaudeAuthStatus.mockReset();
  desktop.startClaudeAuthLogin.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
  resetHarnessSignInForTest();
  useWorkbenchStore.setState(initial, true);
});

it("only curated harnesses have a sign-in status", async () => {
  expect(harnessSupportsSignIn("claude-code")).toBe(true);
  expect(harnessSupportsSignIn("pi")).toBe(false);
  expect(harnessSignInStatus("pi").state).toBe("unsupported");
  expect(await refreshHarnessSignIn("pi")).toBe("unsupported");
  expect(desktop.getClaudeAuthStatus).not.toHaveBeenCalled();
});

it("reports signed out, signed in and an unavailable check", async () => {
  desktop.getClaudeAuthStatus.mockResolvedValueOnce({
    loggedIn: false,
    accountReadStartedAt: ++nativeTime,
  });
  expect(await refreshHarnessSignIn("claude-code")).toBe("signedOut");
  expect(harnessesNeedingSignIn().map((entry) => entry.id)).toEqual([
    "claude-code",
  ]);
  desktop.getClaudeAuthStatus.mockResolvedValueOnce({
    loggedIn: true,
    accountReadStartedAt: ++nativeTime,
  });
  expect(await refreshHarnessSignIn("claude-code")).toBe("signedIn");
  desktop.getClaudeAuthStatus.mockRejectedValueOnce(new Error("timeout"));
  expect(await refreshHarnessSignIn("claude-code")).toBe("unavailable");
  expect(harnessSignInStatus("claude-code").checkedAt).not.toBeNull();
});

it("checks visible harnesses once while their cached status is fresh", async () => {
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: false,
    accountReadStartedAt: ++nativeTime,
  }));
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
    .mockResolvedValueOnce({
      loggedIn: false,
      accountReadStartedAt: ++nativeTime,
    })
    .mockResolvedValueOnce({
      loggedIn: true,
      accountReadStartedAt: ++nativeTime,
    });
  await startHarnessSignIn("claude-code");
  expect(desktop.startClaudeAuthLogin).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(3_000);
  expect(harnessSignInStatus("claude-code").state).toBe("signedOut");
  await vi.advanceTimersByTimeAsync(3_000);
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
  await vi.advanceTimersByTimeAsync(30_000);
  expect(desktop.getClaudeAuthStatus).toHaveBeenCalledTimes(2);
  unsubscribe();
});

it("lists only installed harnesses known to need sign-in", async () => {
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: false,
    accountReadStartedAt: ++nativeTime,
  }));
  await refreshHarnessSignIn("claude-code");
  expect(harnessesNeedingSignIn().map((entry) => entry.name)).toEqual([
    "Claude Code",
  ]);
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: true,
    accountReadStartedAt: ++nativeTime,
  }));
  await refreshHarnessSignIn("claude-code");
  expect(harnessesNeedingSignIn()).toEqual([]);
});
it("keeps Sign in stable while polling, coalesces double clicks, and stops on picker cleanup", async () => {
  vi.useFakeTimers();
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: false,
    accountReadStartedAt: ++nativeTime,
  }));
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
  const startedAt = ++nativeTime;
  let complete!: (value: {
    loggedIn: boolean;
    accountReadStartedAt: number;
  }) => void;
  desktop.getClaudeAuthStatus
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    )
    .mockImplementation(async () => ({
      loggedIn: true,
      accountReadStartedAt: ++nativeTime,
    }));
  const older = refreshHarnessSignIn("claude-code");
  await refreshHarnessSignIn("claude-code");
  complete({ loggedIn: false, accountReadStartedAt: startedAt });
  await older;
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
});

it("preserves the bounded login check budget across repeated suspension and wake", async () => {
  vi.useFakeTimers();
  desktop.startClaudeAuthLogin.mockResolvedValue(undefined);
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: false,
    accountReadStartedAt: ++nativeTime,
  }));
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
    .mockResolvedValueOnce({
      loggedIn: false,
      accountReadStartedAt: ++nativeTime,
    })
    .mockImplementation(async () => ({
      loggedIn: true,
      accountReadStartedAt: ++nativeTime,
    }));
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
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: true,
    accountReadStartedAt: ++nativeTime,
  }));
  await startHarnessSignIn("claude-code");
  await vi.advanceTimersByTimeAsync(120_000);
  expect(harnessSignInStatus("claude-code").state).toBe("unknown");
  const unsubscribe = subscribeHarnessSignIn(() => undefined);
  await vi.advanceTimersByTimeAsync(0);
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
  unsubscribe();
});

it("refreshes an expired cached fact without showing a checking prompt", async () => {
  vi.useFakeTimers();
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: false,
    accountReadStartedAt: ++nativeTime,
  }));
  await refreshHarnessSignIn("claude-code");
  await vi.advanceTimersByTimeAsync(60_001);
  desktop.getClaudeAuthStatus.mockImplementation(async () => ({
    loggedIn: true,
    accountReadStartedAt: ++nativeTime,
  }));
  ensureHarnessSignInChecked(["claude-code"]);
  expect(harnessSignInStatus("claude-code").state).toBe("signedOut");
  await vi.advanceTimersByTimeAsync(0);
  expect(harnessSignInStatus("claude-code").state).toBe("signedIn");
});

it("discards account checks from a previous workspace and allows a fresh check", async () => {
  useWorkbenchStore.setState({
    project: { id: "first", name: "First", roots: [] },
    selectedHarnessId: "account",
    harnessDiscoveryPhase: "signInRequired",
    harnesses: [
      {
        id: "account",
        name: "Account harness",
        description: "",
        source: "builtIn",
        installState: "available",
        transport: "acp",
        bindingId: "account-binding",
        accountModelDiscovery: true,
      },
    ],
  });
  let complete!: () => void;
  desktop.discoverCodex
    .mockImplementationOnce(
      (scope) =>
        new Promise((resolve) => {
          complete = () =>
            resolve({
              ...scope,
              rootId: null,
              account: "signedOut",
              accountReadStartedAt: ++nativeTime,
              models: [],
            });
        }),
    )
    .mockImplementation(async (scope) => ({
      ...scope,
      rootId: null,
      account: "managedChatgpt",
      accountReadStartedAt: ++nativeTime,
      models: [],
    }));
  const old = refreshHarnessSignIn("account");
  useWorkbenchStore.setState({
    project: { id: "second", name: "Second", roots: [] },
    nativeTransitionEpoch: 2,
    harnessDiscoveryPhase: "signInRequired",
    harnessAccount: "signedOut",
    harnessAccountReadStartedAt: ++nativeTime,
  });
  complete();
  await old;
  expect(harnessSignInStatus("account").state).toBe("unknown");
  await refreshHarnessSignIn("account");
  expect(harnessSignInStatus("account").state).toBe("signedIn");
});

it.each([150, 250])(
  "orders a delayed native read at %i against refusal at 200",
  async (startedAt) => {
    let complete!: (value: {
      loggedIn: boolean;
      accountReadStartedAt: number;
    }) => void;
    desktop.getClaudeAuthStatus.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    const read = refreshHarnessSignIn("claude-code");
    reportHarnessSignInRequired("claude-code", 200);
    complete({ loggedIn: true, accountReadStartedAt: startedAt });
    expect(await read).toBe(startedAt < 200 ? "signedOut" : "signedIn");
    expect(harnessSignInStatus("claude-code").evidenceAt).toBe(
      startedAt < 200 ? 200 : 250,
    );
    // Replays cannot change which evidence wins.
    reportHarnessSignInRequired("claude-code", 200);
    expect(harnessSignInStatus("claude-code").state).toBe(
      startedAt < 200 ? "signedOut" : "signedIn",
    );
  },
);

it("orders failed native reads without hiding recovery or accepting an older successful read", async () => {
  reportHarnessSignInRequired("claude-code", 200);
  desktop.getClaudeAuthStatus.mockRejectedValueOnce({
    kind: "internal",
    message: "Offline",
    accountReadStartedAt: 250,
  });
  expect(await refreshHarnessSignIn("claude-code")).toBe("unavailable");
  expect(harnessSignInStatus("claude-code").state).toBe("signedOut");
  expect(harnessSignInStatus("claude-code").evidenceAt).toBe(250);
  desktop.getClaudeAuthStatus.mockResolvedValueOnce({
    loggedIn: true,
    accountReadStartedAt: 225,
  });
  expect(await refreshHarnessSignIn("claude-code")).toBe("signedOut");
  desktop.getClaudeAuthStatus.mockResolvedValueOnce({
    loggedIn: true,
    accountReadStartedAt: 300,
  });
  expect(await refreshHarnessSignIn("claude-code")).toBe("signedIn");
});

it("reads a checkable account again after a refused turn instead of signing it out", async () => {
  desktop.getClaudeAuthStatus
    .mockResolvedValueOnce({ loggedIn: true, accountReadStartedAt: 100 })
    .mockResolvedValueOnce({ loggedIn: true, accountReadStartedAt: 300 });
  expect(await refreshHarnessSignIn("claude-code")).toBe("signedIn");
  harnessSignInFailures.page({
    sessions: [{ id: "session-1", bindingId: "fixture" }],
    turns: [
      {
        state: "failed",
        sessionId: "session-1",
        terminalAt: 200,
        terminalReason: { kind: "authRequired" },
      },
    ],
  } as never);
  expect(harnessesNeedingSignIn()).toEqual([]);
  // The cached read is fresh, but a refusal still sends Sage back to Claude.
  ensureHarnessSignInChecked(["claude-code"]);
  await vi.waitFor(() =>
    expect(harnessSignInStatus("claude-code")).toMatchObject({
      state: "signedIn",
      evidenceAt: 300,
    }),
  );
  expect(desktop.getClaudeAuthStatus).toHaveBeenCalledTimes(2);
});

it.each(["page", "command"] as const)(
  "a refusal on the %s path reads a checkable account again at once",
  async (path) => {
    desktop.getClaudeAuthStatus
      .mockResolvedValueOnce({ loggedIn: true, accountReadStartedAt: 100 })
      .mockResolvedValueOnce({ loggedIn: true, accountReadStartedAt: 300 });
    expect(await refreshHarnessSignIn("claude-code")).toBe("signedIn");
    if (path === "page")
      harnessSignInFailures.page({
        sessions: [{ id: "session-1", bindingId: "fixture" }],
        turns: [
          {
            state: "failed",
            sessionId: "session-1",
            terminalAt: 200,
            terminalReason: { kind: "authRequired" },
          },
        ],
      } as never);
    else
      harnessSignInFailures.command({
        bindingId: "fixture",
        busy: true,
        error: {
          failure: { kind: "signInRequired", terminalAt: 200 } as never,
        },
      });
    // No mount, focus or explicit check follows the refusal.
    await vi.waitFor(() =>
      expect(harnessSignInStatus("claude-code")).toMatchObject({
        state: "signedIn",
        evidenceAt: 300,
      }),
    );
    expect(desktop.getClaudeAuthStatus).toHaveBeenCalledTimes(2);
  },
);
