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
  | "platformUnsupported"
  | "versionTooOld";

export function harnessUnavailableReason(
  reason: HarnessUnavailableReason | null | undefined,
): string {
  switch (reason) {
    case "admissionIncomplete":
      return "Sage hasn't finished checking this harness, so it won't start it yet.";
    case "adapterAbsent":
      return "This harness's ACP adapter isn't installed.";
    case "adapterVersionUnreadable":
      return "Sage couldn't read the adapter's version, so it won't launch it.";
    case "candidateInvalid":
      return "The installed harness didn't pass Sage's safety checks.";
    case "capabilityProjectionUnavailable":
      return "Sage couldn't read which features this harness supports.";
    case "developmentOnly":
      return "This development-only harness isn't part of this build.";
    case "driftDetected":
      return "This harness changed on disk after Sage checked it, so Sage stopped using it.";
    case "nodeAbsent":
      return "This harness needs Node.js, which isn't installed.";
    case "nodeEngineMismatch":
      return "The installed Node.js is older than this harness requires.";
    case "nodeVersionUnreadable":
      return "Sage couldn't read the installed Node.js version.";
    case "observationOnly":
      return "Sage found this harness but can't verify it on this computer.";
    case "platformUnsupported":
      return "This harness isn't supported on this platform.";
    default:
      return "Sage can't start this harness.";
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
  /**
   * Where the bound release sits against the release Sage was tested with.
   * A newer release keeps the tested features; an uncharacterised one (older
   * or unreadable) loses them.
   */
  characterisation?:
    "characterised" | "newerThanTested" | "uncharacterised" | null;
  unavailableReason?: HarnessUnavailableReason | null;
  signIn?: { command: string; hint: string } | null;
  brand?: HarnessBrand | null;
  /** A newer published release and the command that installs it. */
  update?: { latestVersion: string; command: string } | null;
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
  /** Thinking levels the harness lists for this model before a session. */
  effortLevels?: string[];
}

/** Presentation-only mark selection; never selects protocol behavior. */
export function harnessIconKind(
  name: string | null | undefined,
): "claude" | "codex" | "opencode" | undefined {
  if (/claude/i.test(name ?? "")) return "claude";
  if (/codex/i.test(name ?? "")) return "codex";
  if (/opencode/i.test(name ?? "")) return "opencode";
  return undefined;
}
