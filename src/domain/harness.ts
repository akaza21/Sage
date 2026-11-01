export type HarnessInstallState =
  "available" | "detecting" | "notInstalled" | "installing" | "unavailable";

export type HarnessUnavailableReason =
  | "admissionIncomplete"
  | "adapterAbsent"
  | "adapterVersionUnreadable"
  | "candidateInvalid"
  | "capabilityProjectionUnavailable"
  | "developmentOnly"
  | "driftDetected"
  | "nodeAbsent"
  | "nodeEngineMismatch"
  | "nodeVersionUnreadable"
  | "observationOnly"
  | "platformUnsupported";

export function harnessUnavailableReason(
  reason: HarnessUnavailableReason | null | undefined,
): string {
  switch (reason) {
    case "admissionIncomplete":
      return "This harness has not completed admission, so Sage will not start it.";
    case "adapterAbsent":
      return "The ACP adapter this harness launches through is not installed.";
    case "adapterVersionUnreadable":
      return "Sage could not read the adapter's version, so it will not launch it.";
    case "candidateInvalid":
      return "The discovered launch candidate did not pass Sage's checks.";
    case "capabilityProjectionUnavailable":
      return "Sage could not safely project this harness's supported capabilities.";
    case "developmentOnly":
      return "This is a development-only harness and is not part of this build.";
    case "driftDetected":
      return "The admitted build changed on disk after admission. Sage stopped using it.";
    case "nodeAbsent":
      return "Node.js is required to launch this harness and was not found.";
    case "nodeEngineMismatch":
      return "The installed Node.js is older than this harness requires.";
    case "nodeVersionUnreadable":
      return "Sage could not read the installed Node.js version.";
    case "observationOnly":
      return "Sage can observe this harness's signature but cannot independently admit it on this workstation.";
    case "platformUnsupported":
      return "Sage cannot admit this harness on this platform.";
    default:
      return "Sage did not admit this harness and will not start it.";
  }
}

import type { SessionAccessModeCapability } from "./sessionAccessMode";

/** A curated harness's brand mark, set by the native catalogue. */
export type HarnessBrand = "claude" | "codex" | "opencode" | "pi";

interface HarnessDefinitionBase {
  id: string;
  name: string;
  description: string;
  source: "builtIn" | "registry" | "custom" | "development";
  accountModelDiscovery: boolean;
  accessModes?: readonly SessionAccessModeCapability[];
  version?: string | null;
  versionChecking?: boolean;
  boundVersion?: string | null;
  characterisation?: "characterised" | "uncharacterised" | null;
  unavailableReason?: HarnessUnavailableReason | null;
  signIn?: { command: string; hint: string } | null;
  brand?: HarnessBrand | null;
}

export type HarnessDefinition = HarnessDefinitionBase &
  (
    | {
        transport: "legacySimulator";
        installState: HarnessInstallState;
        bindingId: null;
      }
    | {
        transport: "acp";
        installState: "available";
        bindingId: string;
      }
    | {
        transport: "acp";
        installState: Exclude<HarnessInstallState, "available">;
        bindingId: string | null;
      }
  );

export interface ModelOption {
  id: string;
  name: string;
  description?: string;
  isDefault?: boolean;
  contextWindow?: number;
}
