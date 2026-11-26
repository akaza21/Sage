import type { HarnessDefinition } from "../../domain/harness";
import type { HarnessSignInState } from "../harness/harnessSignIn";
import type { HarnessDiscoveryPhase as Phase } from "../../app/harnessSlice";

export type HarnessBadge =
  | "ready"
  | "checking"
  | "available"
  | "signInNeeded"
  | "unavailable"
  | "notInstalled";

export const HARNESS_BADGE_LABEL: Record<HarnessBadge, string> = {
  ready: "Ready",
  checking: "Checking",
  available: "Available",
  signInNeeded: "Sign-in needed",
  unavailable: "Unavailable",
  notInstalled: "Not installed",
};

export interface HarnessCardStatus {
  badge: HarnessBadge;
  /** One line under the harness name: why it can't run, or its sign-in. */
  summary: string;
  /** Whether a terminal sign-in hint still helps: hidden once signed in. */
  showSignInHint: boolean;
}

/**
 * Derives a card's badge from capability data only: the account-model
 * discovery phase for harnesses that expose it, the shared sign-in cache
 * for harnesses with a native status probe, and nothing otherwise.
 */
export function harnessCardStatus({
  harness,
  disabledReason,
  selected,
  phase,
  probe,
}: {
  harness: HarnessDefinition;
  disabledReason: string | undefined;
  selected: boolean;
  phase: Phase;
  /** The shared sign-in cache state; "unsupported" when no probe exists. */
  probe: HarnessSignInState;
}): HarnessCardStatus {
  if (harness.installState === "notInstalled")
    return {
      badge: "notInstalled",
      summary: "Install it, then choose Refresh discovery.",
      showSignInHint: false,
    };
  if (disabledReason)
    return {
      // Detection or an install in progress is not a fault: say so, undimmed.
      badge:
        harness.installState === "detecting" ||
        harness.installState === "installing"
          ? "checking"
          : "unavailable",
      summary: disabledReason,
      showSignInHint: false,
    };
  const signIn = harness.accountModelDiscovery
    ? discoverySignIn(selected, phase)
    : probeSignIn(probe);
  return {
    badge: signIn.state,
    summary: signIn.summary,
    showSignInHint: signIn.state !== "ready",
  };
}

function discoverySignIn(
  selected: boolean,
  phase: Phase,
): { state: HarnessBadge; summary: string } {
  if (!selected)
    return {
      state: "available",
      summary: "Select this harness to check sign-in.",
    };
  switch (phase) {
    case "ready":
      return {
        state: "ready",
        summary: "Signed in; account models discovered.",
      };
    case "signInRequired":
    case "consent":
      return { state: "signInNeeded", summary: "Sign-in required." };
    case "waiting":
      return { state: "signInNeeded", summary: "Waiting for sign-in." };
    case "checking":
      return { state: "available", summary: "Checking sign-in..." };
    default:
      return { state: "available", summary: "Sign-in status unknown." };
  }
}

function probeSignIn(probe: HarnessSignInState): {
  state: HarnessBadge;
  summary: string;
} {
  switch (probe) {
    case "unsupported":
      return {
        state: "available",
        summary: "Sign-in status is not exposed by this harness.",
      };
    case "signedIn":
      return { state: "ready", summary: "Signed in." };
    case "signedOut":
      return { state: "signInNeeded", summary: "Not signed in." };
    case "checking":
      return { state: "available", summary: "Checking sign-in..." };
    default:
      return { state: "available", summary: "Sign-in status unknown." };
  }
}

const GROUP_ORDER: Record<HarnessBadge, number> = {
  ready: 0,
  checking: 1,
  available: 0,
  signInNeeded: 0,
  unavailable: 1,
  notInstalled: 2,
};

/** Runnable harnesses first, then unavailable, then not installed; stable. */
export function harnessGroupRank(badge: HarnessBadge): number {
  return GROUP_ORDER[badge];
}
