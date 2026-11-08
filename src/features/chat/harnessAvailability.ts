import { capitalizeLabel } from "../../domain/text";
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
      return {
        label: "detecting",
        disabledReason: "Sage is still checking which harnesses are installed.",
      };
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
        disabledReason:
          "Install it, then refresh harness discovery in Settings.",
      };
    case "installing":
      return {
        label: "installing",
        disabledReason: "Wait for the installation to finish.",
      };
    case "unavailable":
      return {
        label: "unavailable",
        disabledReason: harnessUnavailableReason(harness.unavailableReason),
      };
  }
}

/** The chat's one-word harness status, most blocking condition first. */
export function harnessStatusLabel(
  harness: Pick<HarnessDefinition, "source" | "installState"> | undefined,
  discoveryPhase: string,
  harnessAvailable: boolean,
  sessionState: string | null,
): string {
  return !harness
    ? "No harness"
    : discoveryPhase === "checking"
      ? "Checking"
      : discoveryPhase === "signInRequired"
        ? "Sign in required"
        : discoveryPhase === "consent"
          ? "Sign-in consent"
          : discoveryPhase === "waiting"
            ? "Waiting for sign in"
            : discoveryPhase === "error"
              ? "Unavailable"
              : harness.source === "development"
                ? "Test simulator"
                : !harnessAvailable
                  ? harness.installState === "installing"
                    ? "Installing"
                    : "Unavailable"
                  : sessionState === "active"
                    ? "Live"
                    : sessionState
                      ? capitalizeLabel(sessionState)
                      : "Ready";
}
