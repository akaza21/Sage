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
  | "versionTooOld"
  | "bindingMissing";

// A missing binding names no harness on this computer; it keeps the fallback.
const unavailableMessages: Record<
  Exclude<HarnessUnavailableReason, "bindingMissing">,
  string
> = {
  admissionIncomplete: "is still being checked.",
  adapterAbsent: "isn't set up for Sage yet.",
  adapterVersionUnreadable: "has a version Sage can't read.",
  candidateInvalid: "didn't pass Sage's checks.",
  capabilityProjectionUnavailable: "has features Sage couldn't check.",
  developmentOnly: "needs a development build.",
  driftDetected: "changed since Sage checked it.",
  nodeAbsent: "needs Node.js installed.",
  nodeEngineMismatch: "needs a newer Node.js version.",
  nodeVersionUnreadable: "needs a Node.js version Sage can read.",
  observationOnly: "can't be verified on this computer.",
  platformUnsupported: "isn't supported on this computer.",
  versionTooOld: "needs an update for Sage.",
};

export function harnessUnavailableReason(
  reason: HarnessUnavailableReason | null | undefined,
  name: string,
): string {
  const message =
    reason && Object.hasOwn(unavailableMessages, reason)
      ? unavailableMessages[reason as keyof typeof unavailableMessages]
      : "can't start yet.";
  return `${name} ${message}`;
}

import type { SessionAccessModeCapability } from "./sessionAccessMode";

export type HarnessBrand = "claude" | "codex" | "opencode" | "pi";

export interface HarnessSignIn {
  command: string;
  hint: string;
  statusCheck?: "claudeCli";
}

export interface HarnessUpdate {
  latestVersion: string;
  command: string;
  /** Curated known commands newer than the installed release. */
  unlocks?: readonly string[];
}

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
  /** A release newer than the tested one keeps the tested features; an older
   * or unreadable one loses them. */
  characterisation?:
    "characterised" | "newerThanTested" | "uncharacterised" | null;
  unavailableReason?: HarnessUnavailableReason | null;
  signIn?: HarnessSignIn | null;
  brand?: HarnessBrand | null;
  update?: HarnessUpdate | null;
  /** The newest published release from the last npm check, if any ran. */
  latestVersion?: string | null;
  /** The release Sage runs when it is not the installed one. */
  runsVersion?: string | null;
  /** Sage runs a fallback until the installed release reaches this one. */
  directFromVersion?: string | null;
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

export function harnessHasSignInCheck(
  harness: HarnessDefinition | undefined,
): boolean {
  return (
    harness?.installState === "available" &&
    Boolean(harness.accountModelDiscovery || harness.signIn?.statusCheck)
  );
}

/** Discovery is still checking this harness, so it may yet be available. */
export const harnessSettling = (harness: HarnessDefinition) =>
  harness.installState === "detecting" ||
  (!!harness.versionChecking && harness.installState !== "available");

export function preferredAvailableHarness(
  harnesses: readonly HarnessDefinition[],
  rememberedId?: string | null,
): HarnessDefinition | null {
  let best: HarnessDefinition | null = null;
  for (const harness of harnesses) {
    // Kept while it is checked, so a harness answering first never takes
    // the remembered one's place.
    if (harness.id === rememberedId && harnessSettling(harness)) return harness;
    if (
      harness.installState !== "available" ||
      (harness.transport === "acp" && !harness.bindingId)
    )
      continue;
    if (harness.id === rememberedId) return harness;
    best ??= harness;
  }
  return best;
}

export interface ModelOption {
  id: string;
  name: string;
  description?: string;
  isDefault?: boolean;
  contextWindow?: number;
  effortLevels?: readonly string[];
}

/** Presentation-only mark selection; never selects protocol behavior. */
export function harnessIconKind(
  name: string | null | undefined,
): HarnessBrand | undefined {
  if (/claude/i.test(name ?? "")) return "claude";
  if (/codex/i.test(name ?? "")) return "codex";
  if (/opencode/i.test(name ?? "")) return "opencode";
  // Pi alone: forks such as Oh My Pi keep the generic mark.
  if (/^pi$/i.test(name?.trim() ?? "")) return "pi";
  return undefined;
}
