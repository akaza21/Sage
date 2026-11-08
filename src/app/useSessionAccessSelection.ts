import { useSessionAccessModeStore } from "./sessionAccessModeStore";
import {
  sessionAccessEffective,
  sessionAccessModeCandidates,
  type SessionAccessContext,
  type SessionAccessModeCapability,
} from "../domain/sessionAccessMode";

/** The live session an access choice applies to, as the chat page shows it. */
export interface SessionAccessTarget {
  sessionId: string;
  availableModeIds: readonly string[] | null;
}

export function useSessionAccessSelection(
  bindingId: string | null | undefined,
  capabilities: readonly SessionAccessModeCapability[] | undefined,
  target?: SessionAccessTarget | null,
) {
  const mode = useSessionAccessModeStore((state) =>
    bindingId ? state.modes[bindingId] : undefined,
  );
  const fullAccess = useSessionAccessModeStore((state) =>
    target ? state.fullAccessSessions.includes(target.sessionId) : false,
  );
  const ready = useSessionAccessModeStore((state) => state.ready);
  const error = useSessionAccessModeStore((state) => state.error);
  const fullAccessError = useSessionAccessModeStore(
    (state) => state.fullAccessError,
  );
  const setMode = useSessionAccessModeStore((state) => state.setMode);
  const session: SessionAccessContext | null = target
    ? { ...target, fullAccess }
    : null;
  const candidates = sessionAccessModeCandidates(capabilities, session);
  const effective = sessionAccessEffective(mode, candidates, session);
  return {
    ready,
    error,
    fullAccessError,
    setMode,
    candidates,
    effective,
    reason: !ready
      ? "Permission mode could not be loaded."
      : (error ??
        (effective === null
          ? "Choose a supported access mode before sending."
          : null)),
  };
}
