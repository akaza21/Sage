import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";

export function bindingNoticeAllowed(phase: HarnessDiscoveryPhase): boolean {
  return ![
    "checking",
    "signInRequired",
    "consent",
    "waiting",
    "error",
  ].includes(phase);
}
