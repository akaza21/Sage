export const SESSION_ACCESS_MODES = [
  "readOnly",
  "ask",
  "workspaceWrite",
  "acceptEdits",
  "plan",
  "fullAccess",
] as const;

export type SessionAccessMode = (typeof SESSION_ACCESS_MODES)[number];

/** Full access is granted to one chat session, never stored as a default. */
export const FULL_ACCESS: SessionAccessMode = "fullAccess";

export const DEFAULT_SESSION_ACCESS_MODE: SessionAccessMode = "readOnly";

export function admitSessionAccessMode(value: unknown): SessionAccessMode {
  return (
    SESSION_ACCESS_MODES.find((mode) => mode === value) ??
    DEFAULT_SESSION_ACCESS_MODE
  );
}

export type SessionAccessModes = Readonly<Record<string, SessionAccessMode>>;
/** Stored per-binding defaults. Full access is never one. */
export function admitSessionAccessModes(value: unknown): SessionAccessModes {
  return Object.fromEntries(
    Object.entries(value && typeof value === "object" ? value : {})
      .slice(0, 8)
      .filter(
        ([binding, mode]) =>
          binding.length > 0 &&
          binding.length <= 128 &&
          mode !== FULL_ACCESS &&
          SESSION_ACCESS_MODES.includes(mode as SessionAccessMode),
      ),
  );
}
export const SESSION_ACCESS_MODE_SCOPE =
  "Applies from your next message with this harness.";
export const FULL_ACCESS_SCOPE = "Applies from your next message in this chat.";

export interface SessionAccessModeAvailability {
  effective: SessionAccessMode | null;
  unavailableReason: string | null;
}

export interface SessionAccessModeCapability {
  id: SessionAccessMode;
  label: string;
  description: string;
  fallback?: boolean;
  /** The harness session mode this access mode selects. */
  sessionModeId?: string;
}

/** The live session an access choice applies to. */
export interface SessionAccessContext {
  sessionId: string;
  /** The session modes the live session advertises; null until reported. */
  availableModeIds: readonly string[] | null;
  /** Whether this session holds a Full access grant. */
  fullAccess: boolean;
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

/**
 * The modes to offer. A mode carried by a harness session mode is offered
 * only while the live session advertises it. Full access needs a live
 * session, since it is granted to that session alone.
 */
export function sessionAccessModeCandidates(
  capabilities: readonly SessionAccessModeCapability[] | undefined,
  session?: SessionAccessContext | null,
) {
  const advertised = session?.availableModeIds ?? null;
  return (
    capabilities?.filter(
      (capability) =>
        SESSION_ACCESS_MODES.includes(capability.id) &&
        typeof capability.label === "string" &&
        capability.label.trim().length > 0 &&
        capability.label.length <= 128 &&
        typeof capability.description === "string" &&
        capability.description.trim().length > 0 &&
        capability.description.length <= 512 &&
        (capability.id !== FULL_ACCESS || advertised !== null) &&
        (capability.sessionModeId === undefined ||
          advertised === null ||
          advertised.includes(capability.sessionModeId)),
    ) ?? []
  );
}

/** The mode the next prompt in this session runs under. */
export function sessionAccessEffective(
  stored: SessionAccessMode | undefined,
  candidates: readonly SessionAccessModeCapability[],
  session?: SessionAccessContext | null,
): SessionAccessMode | null {
  if (
    session?.fullAccess &&
    candidates.some((candidate) => candidate.id === FULL_ACCESS)
  )
    return FULL_ACCESS;
  return sessionAccessModeAvailability(stored, candidates).effective;
}

/** Whether the access control owns the harness's session modes. */
export function accessGovernsSessionModes(
  capabilities: readonly SessionAccessModeCapability[] | undefined,
) {
  return Boolean(
    capabilities?.some((capability) => capability.sessionModeId !== undefined),
  );
}
