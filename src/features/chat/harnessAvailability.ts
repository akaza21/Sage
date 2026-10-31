import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import {
  harnessUnavailableReason,
  type HarnessDefinition,
} from "../../domain/harness";

export function harnessAvailability(harness: HarnessDefinition): {
  label: string;
  disabledReason?: string;
} {
  switch (harness.installState) {
    case "detecting":
      return { label: "detecting", disabledReason: "Detecting harnesses…" };
    case "available":
      return {
        label:
          harness.characterisation === "uncharacterised"
            ? "available — uncharacterised"
            : "available",
      };
    case "notInstalled":
      return {
        label: "not installed",
        disabledReason: "Harness is not installed.",
      };
    case "installing":
      return {
        label: "installing",
        disabledReason: "Harness installation is still in progress.",
      };
    case "unavailable":
      return {
        label: "installed but not drivable",
        disabledReason: harnessUnavailableReason(harness.unavailableReason),
      };
  }
}

export function bindingNoticeAllowed(phase: HarnessDiscoveryPhase): boolean {
  return ![
    "checking",
    "signInRequired",
    "consent",
    "waiting",
    "error",
  ].includes(phase);
}
