import {
  signInHarnesses,
  signInProvider,
  subscribeAccountSignIn,
} from "./harnessSignInProvider";
import { useWorkbenchStore } from "../../app/store";
import {
  harnessSignInFailures,
  harnessSignInEvidence,
  rememberSignInEvidence,
} from "../../services/acpHarnessDesktop";

/** Native account adapters own truth; visible surfaces share this bounded cache. */
export type HarnessSignInState =
  | "unsupported"
  | "unknown"
  | "checking"
  | "signedIn"
  | "signedOut"
  | "unavailable"
  // A turn was refused; the account has not been read since.
  | "refused";

export interface HarnessSignInStatus {
  state: HarnessSignInState;
  checkedAt: number | null;
  evidenceAt: number | null;
}

const STATUS_TTL_MS = 60_000;
const POLL_INTERVAL_MS = 3_000;
const POLL_ATTEMPTS = 40;
const UNSUPPORTED: HarnessSignInStatus = {
  state: "unsupported",
  checkedAt: null,
  evidenceAt: null,
};
const UNKNOWN: HarnessSignInStatus = {
  state: "unknown",
  checkedAt: null,
  evidenceAt: null,
};

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

function prune() {
  let changed = false;
  for (const [id, key] of identities) {
    if (key !== signInProvider(id)?.key) {
      stopPolling(id);
      statuses.delete(id);
      identities.delete(id);
      refreshes.delete(id);
      changed = true;
    }
  }
  if (changed) notify();
}

function publish(harnessId: string, status: HarnessSignInStatus) {
  const provider = signInProvider(harnessId);
  if (!provider) return;
  prune();
  const previous = harnessSignInStatus(harnessId);
  if (
    previous.evidenceAt != null &&
    (status.evidenceAt == null ||
      status.evidenceAt < previous.evidenceAt ||
      (status.evidenceAt === previous.evidenceAt &&
        (status.state !== "signedOut" || previous.state === "signedOut")))
  )
    return;
  if (status.state === "unavailable" && previous.state === "signedOut")
    status = { ...status, state: "signedOut" };
  identities.set(harnessId, provider.key);
  statuses.set(harnessId, status);
  const bindingId = signInHarnesses().find(
    (entry) => entry.id === harnessId,
  )?.bindingId;
  if (
    bindingId &&
    status.evidenceAt != null &&
    (status.state === "signedIn" ||
      status.state === "signedOut" ||
      status.state === "unavailable")
  ) {
    if (rememberSignInEvidence(bindingId, status.evidenceAt, status.state))
      return;
  }
  notify();
}

export function harnessSupportsSignIn(harnessId: string): boolean {
  return Boolean(signInProvider(harnessId));
}

export function harnessSignInStatus(harnessId: string): HarnessSignInStatus {
  const provider = signInProvider(harnessId);
  if (!provider) return UNSUPPORTED;
  const status =
    identities.get(harnessId) === provider.key
      ? (statuses.get(harnessId) ?? UNKNOWN)
      : UNKNOWN;
  const bindingId = signInHarnesses().find(
    (entry) => entry.id === harnessId,
  )?.bindingId;
  const evidence = bindingId && harnessSignInEvidence.get(bindingId);
  return evidence &&
    (status.evidenceAt == null || evidence[0] >= status.evidenceAt)
    ? {
        ...status,
        // A checkable account is read again before a refusal signs it out.
        state:
          evidence[1] === "refused" && !provider.backgroundCheck
            ? "signedOut"
            : evidence[1],
        evidenceAt: evidence[0],
      }
    : status;
}

export async function refreshHarnessSignIn(
  harnessId: string,
): Promise<HarnessSignInState> {
  const provider = signInProvider(harnessId);
  if (!provider) return "unsupported";
  if (provider.refreshRequirement) return "unavailable";
  const nativeTransitionEpoch =
    useWorkbenchStore.getState().nativeTransitionEpoch;
  prune();
  identities.set(harnessId, provider.key);
  const request = Symbol();
  refreshes.set(harnessId, request);
  if (harnessSignInStatus(harnessId).state === "unknown")
    publish(harnessId, {
      state: "checking",
      checkedAt: null,
      evidenceAt: null,
    });
  let read: { state: HarnessSignInState; accountReadStartedAt: number | null };
  try {
    read = await provider.status();
  } catch {
    read = { state: "unavailable", accountReadStartedAt: null };
  }
  if (refreshes.get(harnessId) === request) refreshes.delete(harnessId);
  if (
    signInProvider(harnessId)?.key === provider.key &&
    useWorkbenchStore.getState().nativeTransitionEpoch === nativeTransitionEpoch
  ) {
    publish(harnessId, {
      state: read.state,
      checkedAt: Date.now(),
      evidenceAt: read.accountReadStartedAt,
    });
    return read.state === "unavailable" || read.state === "unknown"
      ? read.state
      : harnessSignInStatus(harnessId).state;
  }
  return "unavailable";
}

function stopPolling(harnessId: string) {
  clearTimeout(polls.get(harnessId)?.timer);
  polls.delete(harnessId);
}

export function startHarnessSignIn(harnessId: string): Promise<void> {
  prune();
  const pending = polls.get(harnessId)?.starting;
  if (pending) return pending;
  const provider = signInProvider(harnessId);
  if (!provider) return Promise.reject(new Error("Sign-in is unavailable."));
  identities.set(harnessId, provider.key);
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
      if (!provider.backgroundCheck) {
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
    unsubscribeAccount = subscribeAccountSignIn((id, state, evidenceAt) => {
      publish(id, { state, checkedAt: Date.now(), evidenceAt });
    }, prune);
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
  harnessSignInEvidence.clear();
  identities.clear();
  unsubscribeAccount?.();
  unsubscribeAccount = undefined;
  listeners.clear();
}

/** All evidence compares native epoch ms, independent of delivery or replay order. */
export function reportHarnessSignInRequired(
  harnessId: string,
  terminalAt: number,
) {
  publish(harnessId, {
    state: "signedOut",
    checkedAt: Date.now(),
    evidenceAt: terminalAt,
  });
}

export function ensureHarnessSignInChecked(
  harnessIds: readonly string[],
  onFocus = false,
) {
  for (const harnessId of harnessIds) {
    const provider = signInProvider(harnessId);
    if (!provider || provider.refreshRequirement || refreshes.has(harnessId))
      continue;
    const { state, checkedAt } = harnessSignInStatus(harnessId);
    if (
      (onFocus && state === "signedOut") ||
      (provider.backgroundCheck &&
        (state === "refused" ||
          checkedAt == null ||
          Date.now() - checkedAt > STATUS_TTL_MS))
    )
      void refreshHarnessSignIn(harnessId);
  }
}

export function harnessesNeedingSignIn() {
  return signInHarnesses().filter(
    (harness) => harnessSignInStatus(harness.id).state === "signedOut",
  );
}

harnessSignInFailures.changed = (bindingId, state) => {
  notify();
  // A refused turn reads the account now, not at the next mount or focus.
  const harness = signInHarnesses().find(
    (entry) => entry.bindingId === bindingId,
  );
  if (state === "refused" && harness) ensureHarnessSignInChecked([harness.id]);
};
