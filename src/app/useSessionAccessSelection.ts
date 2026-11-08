import { useCallback } from "react";
import { useSessionAccessModeStore } from "./sessionAccessModeStore";
import {
  FULL_ACCESS,
  sessionAccessEffective,
  sessionAccessModeCandidates,
  type SessionAccessContext,
  type SessionAccessMode,
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
  const prepareFullAccess = useSessionAccessModeStore(
    (state) => state.prepareFullAccess,
  );
  const grantFullAccess = useSessionAccessModeStore(
    (state) => state.grantFullAccess,
  );
  const revokeFullAccess = useSessionAccessModeStore(
    (state) => state.revokeFullAccess,
  );
  const sessionId = target?.sessionId;
  // Stable per session, so an open confirmation asks for one token only.
  const prepare = useCallback(
    () => (sessionId ? prepareFullAccess(sessionId) : Promise.resolve(null)),
    [sessionId, prepareFullAccess],
  );
  const session: SessionAccessContext | null = target
    ? { ...target, fullAccess }
    : null;
  const candidates = sessionAccessModeCandidates(capabilities, session);
  const effective = sessionAccessEffective(mode, candidates, session);
  /** Chooses a mode other than Full access; ends this chat's Full access first. */
  const choose = async (next: SessionAccessMode) => {
    if (!bindingId || next === FULL_ACCESS) return;
    if (target && fullAccess) {
      await revokeFullAccess(target.sessionId);
      const still = useSessionAccessModeStore
        .getState()
        .fullAccessSessions.includes(target.sessionId);
      if (still) return;
    }
    setMode(bindingId, next);
  };
  return {
    ready,
    error,
    fullAccessError,
    setMode,
    choose,
    candidates,
    effective,
    prepareFullAccess: prepare,
    grantFullAccess: (confirmToken: string) =>
      target
        ? grantFullAccess(target.sessionId, confirmToken)
        : Promise.resolve(),
    reason: !ready
      ? "Permission mode could not be loaded."
      : (error ??
        (effective === null
          ? "Choose a supported access mode before sending."
          : null)),
  };
}
