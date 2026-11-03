import { acpDesktop } from "../../services/acpDesktop";

/** The native CLI owns sign-in truth; this bounded cache serves visible pickers. */
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
const polls = new Map<string, ReturnType<typeof setTimeout>>();
const refreshes = new Map<string, symbol>();
const signIns = new Map<string, Promise<void>>();
const pollOwners = new Map<string, symbol>();

function publish(harnessId: string, status: HarnessSignInStatus) {
  statuses.set(harnessId, status);
  for (const listener of listeners) listener();
}

export function harnessSupportsSignIn(harnessId: string): boolean {
  return Object.hasOwn(PROVIDERS, harnessId);
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
  const request = Symbol();
  refreshes.set(harnessId, request);
  if (previous.checkedAt === null)
    publish(harnessId, { state: "checking", checkedAt: previous.checkedAt });
  let state: HarnessSignInState;
  try {
    state = (await provider.status()) ? "signedIn" : "signedOut";
  } catch {
    state = "unavailable";
  }
  if (refreshes.get(harnessId) === request) {
    publish(harnessId, { state, checkedAt: Date.now() });
  }
  return state;
}

function stopPolling(harnessId: string) {
  const timer = polls.get(harnessId);
  if (timer !== undefined) clearTimeout(timer);
  polls.delete(harnessId);
  pollOwners.delete(harnessId);
}

/**
 * Opens the harness's own sign-in (never handles credentials) and then
 * re-checks on a bounded schedule until it reports signed in.
 */
export function startHarnessSignIn(harnessId: string): Promise<void> {
  const pending = signIns.get(harnessId);
  if (pending) return pending;
  const provider = PROVIDERS[harnessId];
  if (!provider) return Promise.reject(new Error("Sign-in is unavailable."));
  stopPolling(harnessId);
  const owner = Symbol();
  pollOwners.set(harnessId, owner);
  const current = () => pollOwners.get(harnessId) === owner;
  const request = provider
    .start()
    .then(() => {
      if (!current()) return;
      let attempts = 0;
      const poll = () => {
        if (!current()) return;
        attempts += 1;
        void refreshHarnessSignIn(harnessId).then((state) => {
          if (!current()) return;
          if (state === "signedIn" || attempts >= POLL_ATTEMPTS)
            stopPolling(harnessId);
          else polls.set(harnessId, setTimeout(poll, POLL_INTERVAL_MS));
        });
      };
      polls.set(harnessId, setTimeout(poll, POLL_INTERVAL_MS));
    })
    .finally(() => signIns.delete(harnessId));
  signIns.set(harnessId, request);
  return request;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (!listeners.size)
      for (const id of [...pollOwners.keys()]) stopPolling(id);
  };
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
  refreshes.clear();
  signIns.clear();
  pollOwners.clear();
  statuses.clear();
  listeners.clear();
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
