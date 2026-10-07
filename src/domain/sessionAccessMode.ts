export const SESSION_ACCESS_MODES = [
  "readOnly",
  "ask",
  "workspaceWrite",
] as const;

export type SessionAccessMode = (typeof SESSION_ACCESS_MODES)[number];

export const DEFAULT_SESSION_ACCESS_MODE: SessionAccessMode = "readOnly";

const COPY: Record<SessionAccessMode, { label: string; description: string }> =
  {
    readOnly: {
      label: "Read-only",
      description:
        "The agent can read your workspace but cannot change it. Nothing is written without you.",
    },
    ask: {
      label: "Ask each time",
      description:
        "The agent asks before each change. Harnesses without permission prompts stay read-only.",
    },
    workspaceWrite: {
      label: "Auto-accept edits in workspace",
      description:
        "The agent edits files inside this workspace without asking. It cannot write outside it.",
    },
  };

export function admitSessionAccessMode(value: unknown): SessionAccessMode {
  return (
    SESSION_ACCESS_MODES.find((mode) => mode === value) ??
    DEFAULT_SESSION_ACCESS_MODE
  );
}

export function sessionAccessModeCopy(mode: SessionAccessMode) {
  return COPY[mode];
}

export const PERMISSION_PROMPTS_UNCONFIRMED =
  "This harness has not confirmed support for permission prompts.";

export const SESSION_ACCESS_MODE_SCOPE =
  "Applies from your next message in any chat.";

export interface SessionAccessModeAvailability {
  effective: SessionAccessMode;
  unavailableReason: string | null;
}

export function sessionAccessModeAvailability(
  mode: SessionAccessMode,
  permissionRequests: boolean,
): SessionAccessModeAvailability {
  const requested = admitSessionAccessMode(mode);
  if (requested === "ask" && permissionRequests !== true) {
    return {
      effective: DEFAULT_SESSION_ACCESS_MODE,
      unavailableReason: PERMISSION_PROMPTS_UNCONFIRMED,
    };
  }
  return { effective: requested, unavailableReason: null };
}

export function sessionAccessModeCandidates(permissionRequests: boolean) {
  return SESSION_ACCESS_MODES.map((mode) => ({
    id: mode,
    label: sessionAccessModeCopy(mode).label,
    triggerLabel: sessionAccessModeCopy(mode).label,
    disabledReason:
      sessionAccessModeAvailability(mode, permissionRequests)
        .unavailableReason ?? undefined,
  }));
}
