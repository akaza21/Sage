export const SESSION_ACCESS_MODES = [
  "readOnly",
  "ask",
  "workspaceWrite",
] as const;

export type SessionAccessMode = (typeof SESSION_ACCESS_MODES)[number];

export const DEFAULT_SESSION_ACCESS_MODE: SessionAccessMode = "readOnly";

export function admitSessionAccessMode(value: unknown): SessionAccessMode {
  return (
    SESSION_ACCESS_MODES.find((mode) => mode === value) ??
    DEFAULT_SESSION_ACCESS_MODE
  );
}

export type SessionAccessModes = Readonly<Record<string, SessionAccessMode>>;
export function admitSessionAccessModes(value: unknown): SessionAccessModes {
  return Object.fromEntries(
    Object.entries(value && typeof value === "object" ? value : {})
      .slice(0, 8)
      .filter(
        ([binding, mode]) =>
          binding.length > 0 &&
          binding.length <= 128 &&
          SESSION_ACCESS_MODES.includes(mode as SessionAccessMode),
      ),
  );
}
export const SESSION_ACCESS_MODE_SCOPE =
  "Applies from your next message with this harness.";

export interface SessionAccessModeAvailability {
  effective: SessionAccessMode | null;
  unavailableReason: string | null;
}

export interface SessionAccessModeCapability {
  id: SessionAccessMode;
  label: string;
  description: string;
  fallback?: boolean;
}

export function sessionAccessModeAvailability(
  mode: SessionAccessMode | undefined,
  capabilities: readonly SessionAccessModeCapability[] | undefined,
): SessionAccessModeAvailability {
  const requested =
    mode === undefined ? undefined : admitSessionAccessMode(mode);
  const admitted = sessionAccessModeCandidates(capabilities);
  if (!admitted.some((capability) => capability.id === requested)) {
    return {
      effective:
        admitted.find(
          (capability) =>
            capability.fallback &&
            (requested === undefined || capability.id === "readOnly"),
        )?.id ?? null,
      unavailableReason: "This access mode is not supported by this harness.",
    };
  }
  return { effective: requested ?? null, unavailableReason: null };
}

export function sessionAccessModeCandidates(
  capabilities: readonly SessionAccessModeCapability[] | undefined,
) {
  return (
    capabilities?.filter(
      (capability) =>
        SESSION_ACCESS_MODES.includes(capability.id) &&
        typeof capability.label === "string" &&
        capability.label.trim().length > 0 &&
        capability.label.length <= 128 &&
        typeof capability.description === "string" &&
        capability.description.trim().length > 0 &&
        capability.description.length <= 512,
    ) ?? []
  );
}
