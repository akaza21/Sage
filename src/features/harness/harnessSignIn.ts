import {
  signInHarnesses,
  signInProvider,
  subscribeAccountSignIn,
  resetAccountSignInForTest,
} from "./harnessSignInProvider";

/** Native account adapters own truth; visible surfaces share this bounded cache. */
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

const STATUS_TTL_MS = 60_000;
const POLL_INTERVAL_MS = 3_000;
const POLL_ATTEMPTS = 40;
const UNSUPPORTED: HarnessSignInStatus = {
  state: "unsupported",
  checkedAt: null,
};
const UNKNOWN: HarnessSignInStatus = { state: "unknown", checkedAt: null };

const statuses = new Map<string, HarnessSignInStatus>();
const identities = new Map<string, string>();
const listeners = new Set<() => void>();
let unsubscribeAccount: (() => void) | undefined;
let revision = 0;
export const harnessSignInRevision = () => revision;
function notify() {
  revision += 1;
  for (const listener of listeners) listener();
}
type SignInPoll = {
  resume?: () => void;
  starting?: Promise<void>;
  timer?: ReturnType<typeof setTimeout>;
};
// Only installed capability providers own polls. All visible subscribers share one
// lifetime; suspension retains intent and attempt counts without any timers.
const polls = new Map<string, SignInPoll>();
const refreshes = new Map<string, symbol>();

function publish(harnessId: string, status: HarnessSignInStatus) {
  const provider = signInProvider(harnessId);
  if (!provider) return;
  identities.set(harnessId, provider.key);
  statuses.set(harnessId, status);
  notify();
}

export function harnessSupportsSignIn(harnessId: string): boolean {
  return Boolean(signInProvider(harnessId));
}

export function harnessSignInStatus(harnessId: string): HarnessSignInStatus {
  if (!harnessSupportsSignIn(harnessId)) return UNSUPPORTED;
  if (identities.get(harnessId) !== signInProvider(harnessId)?.key)
    return UNKNOWN;
  return statuses.get(harnessId) ?? UNKNOWN;
}

export async function refreshHarnessSignIn(
  harnessId: string,
): Promise<HarnessSignInState> {
  const provider = signInProvider(harnessId);
  if (!provider) return "unsupported";
  const request = Symbol();
  refreshes.set(harnessId, request);
  if (harnessSignInStatus(harnessId).state === "unknown")
    publish(harnessId, { state: "checking", checkedAt: null });
  let state: HarnessSignInState;
  try {
    state = await provider.status();
  } catch {
    state = "unavailable";
  }
  if (refreshes.get(harnessId) === request) {
    refreshes.delete(harnessId);
    if (signInProvider(harnessId)?.key === provider.key)
      publish(harnessId, {
        state:
          state === "unavailable" &&
          harnessSignInStatus(harnessId).state === "signedOut"
            ? "signedOut"
            : state,
        checkedAt: Date.now(),
      });
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
  const provider = signInProvider(harnessId);
  if (!provider) return Promise.reject(new Error("Sign-in is unavailable."));
  stopPolling(harnessId);
  const owner: SignInPoll = {};
  polls.set(harnessId, owner);
  const request = provider
    .start()
    .then(() => {
      owner.starting = undefined;
      if (polls.get(harnessId) !== owner) return;
      if (signInProvider(harnessId)?.key !== provider.key) {
        stopPolling(harnessId);
        return;
      }
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
  if (wake) {
    unsubscribeAccount = subscribeAccountSignIn(
      (id, state) => {
        refreshes.delete(id);
        publish(id, { state, checkedAt: Date.now() });
      },
      () => {
        let pruned = false;
        for (const id of statuses.keys()) {
          if (
            !harnessSupportsSignIn(id) ||
            identities.get(id) !== signInProvider(id)?.key
          ) {
            stopPolling(id);
            statuses.delete(id);
            identities.delete(id);
            refreshes.delete(id);
            pruned = true;
          }
        }
        if (pruned) notify();
      },
    );
    for (const owner of polls.values()) owner.resume?.();
  }
  return () => {
    listeners.delete(listener);
    if (!listeners.size) {
      unsubscribeAccount?.();
      unsubscribeAccount = undefined;
      for (const owner of polls.values()) clearTimeout(owner.timer);
    }
  };
}

export function resetHarnessSignInForTest() {
  for (const harnessId of [...polls.keys()]) stopPolling(harnessId);
  refreshes.clear();
  statuses.clear();
  identities.clear();
  unsubscribeAccount?.();
  unsubscribeAccount = undefined;
  listeners.clear();
  resetAccountSignInForTest();
}

/** A protocol auth refusal is newer evidence than an earlier account check. */
export function reportHarnessSignInRequired(harnessId: string) {
  refreshes.delete(harnessId);
  publish(harnessId, { state: "signedOut", checkedAt: Date.now() });
}

export function ensureHarnessSignInChecked(harnessIds: readonly string[]) {
  for (const harnessId of harnessIds) {
    if (!harnessSupportsSignIn(harnessId) || refreshes.has(harnessId)) continue;
    const checkedAt = harnessSignInStatus(harnessId).checkedAt;
    if (checkedAt == null || Date.now() - checkedAt > STATUS_TTL_MS)
      void refreshHarnessSignIn(harnessId);
  }
}

export function harnessesNeedingSignIn() {
  return signInHarnesses().filter(
    (harness) => harnessSignInStatus(harness.id).state === "signedOut",
  );
}
