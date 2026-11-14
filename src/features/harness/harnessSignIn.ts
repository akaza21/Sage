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
type SignInPoll = {
  resume?: () => void;
  starting?: Promise<void>;
  timer?: ReturnType<typeof setTimeout>;
};
// Only curated providers own polls. All visible picker subscribers share one
// lifetime; suspension retains intent and attempt counts without any timers.
const polls = new Map<string, SignInPoll>();
const refreshes = new Map<string, symbol>();

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
  const request = Symbol();
  refreshes.set(harnessId, request);
  if (!statuses.has(harnessId))
    publish(harnessId, { state: "checking", checkedAt: null });
  let state: HarnessSignInState;
  try {
    state = (await provider.status()) ? "signedIn" : "signedOut";
  } catch {
    state = "unavailable";
  }
  if (refreshes.get(harnessId) === request) {
    refreshes.delete(harnessId);
    publish(harnessId, { state, checkedAt: Date.now() });
  }
  return state;
}

function stopPolling(harnessId: string) {
  clearTimeout(polls.get(harnessId)?.timer);
  polls.delete(harnessId);
}

export function startHarnessSignIn(harnessId: string): Promise<void> {
  const pending = polls.get(harnessId)?.starting;
  if (pending) return pending;
  const provider = PROVIDERS[harnessId];
  if (!provider) return Promise.reject(new Error("Sign-in is unavailable."));
  stopPolling(harnessId);
  const owner: SignInPoll = {};
  polls.set(harnessId, owner);
  const request = provider
    .start()
    .then(() => {
      owner.starting = undefined;
      if (polls.get(harnessId) !== owner) return;
      let attempts = 0;
      let reading = false;
      const poll = () => {
        if (!listeners.size || reading || polls.get(harnessId) !== owner)
          return;
        reading = true;
        void refreshHarnessSignIn(harnessId).then((state) => {
          reading = false;
          if (polls.get(harnessId) !== owner) return;
          if (state === "signedIn" || ++attempts >= POLL_ATTEMPTS)
            stopPolling(harnessId);
          else if (listeners.size)
            owner.timer = setTimeout(poll, POLL_INTERVAL_MS);
        });
      };
      owner.resume = poll;
      if (listeners.size) owner.timer = setTimeout(poll, POLL_INTERVAL_MS);
    })
    .catch((error: unknown) => {
      if (polls.get(harnessId) === owner) stopPolling(harnessId);
      throw error;
    });
  owner.starting = request;
  return request;
}

export function subscribeHarnessSignIn(listener: () => void) {
  const wake = !listeners.size;
  listeners.add(listener);
  if (wake) for (const owner of polls.values()) owner.resume?.();
  return () => {
    listeners.delete(listener);
    if (!listeners.size) {
      for (const owner of polls.values()) clearTimeout(owner.timer);
    }
  };
}

const SIGN_IN_LABELS: Partial<Record<HarnessSignInState, string>> = {
  signedIn: "Signed in",
  signedOut: "Signed out",
  checking: "Checking sign-in\u2026",
  unavailable: "Sign-in status unavailable",
};

export function harnessSignInLabel(state: HarnessSignInState): string | null {
  return SIGN_IN_LABELS[state] ?? null;
}

export function resetHarnessSignInForTest() {
  for (const harnessId of [...polls.keys()]) stopPolling(harnessId);
  refreshes.clear();
  statuses.clear();
  listeners.clear();
}

export function harnessSignInAction(
  harnessId: string,
  harnessName: string,
  state: HarnessSignInState,
):
  { label: string; accessibleLabel: string; onAction: () => void } | undefined {
  const signIn = state === "signedOut";
  if (signIn || state === "unavailable" || state === "unknown")
    return {
      label: signIn ? "Sign in" : "Check sign-in",
      accessibleLabel: signIn
        ? `Sign in to ${harnessName}`
        : `Check ${harnessName} sign-in`,
      onAction: () =>
        void (signIn ? startHarnessSignIn : refreshHarnessSignIn)(
          harnessId,
        ).catch(() => undefined),
    };
}

export function ensureHarnessSignInChecked(harnessIds: readonly string[]) {
  for (const harnessId of harnessIds) {
    if (!harnessSupportsSignIn(harnessId) || refreshes.has(harnessId)) continue;
    const checkedAt = statuses.get(harnessId)?.checkedAt;
    if (checkedAt == null || Date.now() - checkedAt > STATUS_TTL_MS)
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
  const decorations = new Map<string, HarnessSignInDecoration>();
  for (const { id, name } of harnesses) {
    if (!harnessSupportsSignIn(id)) continue;
    const state = statuses.get(id)?.state ?? "unknown";
    decorations.set(id, {
      label: harnessSignInLabel(state),
      action: harnessSignInAction(id, name, state),
    });
  }
  return decorations;
}
