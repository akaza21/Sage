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
            ? "available — untested version"
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
