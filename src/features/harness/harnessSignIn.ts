import { useEffect, useMemo, useSyncExternalStore } from "react";

import { acpDesktop } from "../../services/acpDesktop";

/**
 * AP-B B4: harness sign-in state, shown before a turn starts.
 *
 * This is the small exported status API other surfaces (for example the
 * AP-G Settings › Harnesses page) consume: read with
 * `useHarnessSignInStatus`, refresh with `refreshHarnessSignIn`, and start
 * the harness's own sign-in with `startHarnessSignIn`. The native CLI owns the
 * truth; this module only caches its last bounded answer for rendering.
 */
export type HarnessSignInState =
  | "unsupported"
  | "unknown"
  | "checking"
  | "signedIn"
  | "signedOut"
  | "unavailable";

export interface HarnessSignInStatus {
  state: HarnessSignInState;
  checkedAt: number | null;
}

interface SignInProvider {
  status: () => Promise<boolean>;
  start: () => Promise<void>;
}

/** Curated sign-in providers: the harnesses with a native status check. */
const PROVIDERS: Readonly<Record<string, SignInProvider>> = {
  "claude-code": {
    status: async () => (await acpDesktop.getClaudeAuthStatus()).loggedIn,
    start: () => acpDesktop.startClaudeAuthLogin(),
  },
};

const STATUS_TTL_MS = 60_000;
const POLL_INTERVAL_MS = 3_000;
const POLL_ATTEMPTS = 40;
const UNSUPPORTED: HarnessSignInStatus = {
  state: "unsupported",
  checkedAt: null,
};
const UNKNOWN: HarnessSignInStatus = { state: "unknown", checkedAt: null };

const statuses = new Map<string, HarnessSignInStatus>();
const listeners = new Set<() => void>();
let storeVersion = 0;
const polls = new Map<string, ReturnType<typeof setTimeout>>();

function publish(harnessId: string, status: HarnessSignInStatus) {
  statuses.set(harnessId, status);
  storeVersion += 1;
  for (const listener of listeners) listener();
}

export function harnessSupportsSignIn(harnessId: string): boolean {
  return Object.prototype.hasOwnProperty.call(PROVIDERS, harnessId);
}

export function harnessSignInStatus(harnessId: string): HarnessSignInStatus {
  if (!harnessSupportsSignIn(harnessId)) return UNSUPPORTED;
  return statuses.get(harnessId) ?? UNKNOWN;
}

export async function refreshHarnessSignIn(
  harnessId: string,
): Promise<HarnessSignInState> {
  const provider = PROVIDERS[harnessId];
  if (!provider) return "unsupported";
  const previous = harnessSignInStatus(harnessId);
  publish(harnessId, { state: "checking", checkedAt: previous.checkedAt });
  let state: HarnessSignInState;
  try {
    state = (await provider.status()) ? "signedIn" : "signedOut";
  } catch {
    state = "unavailable";
  }
  publish(harnessId, { state, checkedAt: Date.now() });
  return state;
}

function stopPolling(harnessId: string) {
  const timer = polls.get(harnessId);
  if (timer !== undefined) clearTimeout(timer);
  polls.delete(harnessId);
}

/**
 * Opens the harness's own sign-in (never handles credentials) and then
 * re-checks on a bounded schedule until it reports signed in.
 */
export async function startHarnessSignIn(harnessId: string): Promise<void> {
  const provider = PROVIDERS[harnessId];
  if (!provider) throw new Error("This harness has no Sage sign-in action.");
  await provider.start();
  stopPolling(harnessId);
  let attempts = 0;
  const poll = () => {
    attempts += 1;
    void refreshHarnessSignIn(harnessId).then((state) => {
      if (state === "signedIn" || attempts >= POLL_ATTEMPTS) {
        stopPolling(harnessId);
        return;
      }
      polls.set(harnessId, setTimeout(poll, POLL_INTERVAL_MS));
    });
  };
  polls.set(harnessId, setTimeout(poll, POLL_INTERVAL_MS));
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Reads one harness's sign-in state, checking it when stale. */
export function useHarnessSignInStatus(
  harnessId: string | null,
  enabled = true,
): HarnessSignInStatus {
  const status = useSyncExternalStore(subscribe, () =>
    harnessId ? harnessSignInStatus(harnessId) : UNSUPPORTED,
  );
  useEffect(() => {
    if (!enabled || !harnessId || !harnessSupportsSignIn(harnessId)) return;
    const current = harnessSignInStatus(harnessId);
    const stale =
      current.checkedAt === null ||
      Date.now() - current.checkedAt > STATUS_TTL_MS;
    if (stale && current.state !== "checking")
      void refreshHarnessSignIn(harnessId);
  }, [enabled, harnessId]);
  return status;
}

/** Reads several harnesses at once (for a picker), checking stale ones. */
export function useHarnessSignInStatuses(
  harnessIds: readonly string[],
  enabled = true,
): ReadonlyMap<string, HarnessSignInStatus> {
  const version = useSyncExternalStore(subscribe, () => storeVersion);
  const key = harnessIds.filter(harnessSupportsSignIn).join("\0");
  useEffect(() => {
    if (!enabled || !key) return;
    for (const harnessId of key.split("\0")) {
      const current = harnessSignInStatus(harnessId);
      const stale =
        current.checkedAt === null ||
        Date.now() - current.checkedAt > STATUS_TTL_MS;
      if (stale && current.state !== "checking")
        void refreshHarnessSignIn(harnessId);
    }
  }, [enabled, key]);
  return useMemo(
    () =>
      new Map(
        (key ? key.split("\0") : []).map((harnessId) => [
          harnessId,
          harnessSignInStatus(harnessId),
        ]),
      ),
    // `version` advances whenever any status is published.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key, version],
  );
}

export function harnessSignInLabel(state: HarnessSignInState): string | null {
  switch (state) {
    case "signedIn":
      return "Signed in";
    case "signedOut":
      return "Signed out";
    case "checking":
      return "Checking sign-in…";
    case "unavailable":
      return "Sign-in status unavailable";
    default:
      return null;
  }
}

export function resetHarnessSignInForTest() {
  for (const harnessId of [...polls.keys()]) stopPolling(harnessId);
  statuses.clear();
  listeners.clear();
  storeVersion = 0;
}

/** The picker-row action for a harness's current sign-in state. */
export function harnessSignInAction(
  harnessId: string,
  harnessName: string,
  state: HarnessSignInState,
):
  { label: string; accessibleLabel: string; onAction: () => void } | undefined {
  if (state === "signedOut")
    return {
      label: "Sign in",
      accessibleLabel: `Sign in to ${harnessName}`,
      onAction: () => void startHarnessSignIn(harnessId).catch(() => undefined),
    };
  if (state === "unavailable" || state === "unknown")
    return {
      label: "Check sign-in",
      accessibleLabel: `Check ${harnessName} sign-in`,
      onAction: () => void refreshHarnessSignIn(harnessId),
    };
  return undefined;
}

/** Non-hook API for the lazily loaded picker bridge. */
export const subscribeHarnessSignIn = subscribe;

export function ensureHarnessSignInChecked(harnessIds: readonly string[]) {
  for (const harnessId of harnessIds.filter(harnessSupportsSignIn)) {
    const current = harnessSignInStatus(harnessId);
    const stale =
      current.checkedAt === null ||
      Date.now() - current.checkedAt > STATUS_TTL_MS;
    if (stale && current.state !== "checking")
      void refreshHarnessSignIn(harnessId);
  }
}

export interface HarnessSignInDecoration {
  label: string | null;
  action: ReturnType<typeof harnessSignInAction>;
}

export function harnessSignInDecorations(
  harnesses: readonly { id: string; name: string }[],
): Map<string, HarnessSignInDecoration> {
  return new Map(
    harnesses
      .filter((harness) => harnessSupportsSignIn(harness.id))
      .map((harness) => {
        const { state } = harnessSignInStatus(harness.id);
        return [
          harness.id,
          {
            label: harnessSignInLabel(state),
            action: harnessSignInAction(harness.id, harness.name, state),
          },
        ];
      }),
  );
}
