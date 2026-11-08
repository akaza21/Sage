import { useEffect } from "react";
import { useSessionAccessModeStore } from "./sessionAccessModeStore";
import {
  sessionAccessEffective,
  sessionAccessModeCandidates,
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
  const sessionId = target?.sessionId;
  const chat = useSessionAccessModeStore((state) =>
    sessionId ? state.chatModes[sessionId] : null,
  );
  const ready = useSessionAccessModeStore((state) => state.ready);
  const error = useSessionAccessModeStore((state) => state.error);
  const chatError = useSessionAccessModeStore((state) => state.chatError);
  const setMode = useSessionAccessModeStore((state) => state.setMode);
  const unread = chat === undefined;
  useEffect(() => {
    if (sessionId && unread)
      void import("./sessionAccessActions").then((actions) =>
        actions.loadChatAccess(sessionId),
      );
  }, [sessionId, unread]);
  const candidates = sessionAccessModeCandidates(
    capabilities,
    target?.availableModeIds,
  );
  const effective = sessionAccessEffective(mode, candidates, chat);
  return {
    ready,
    error,
    chatError,
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
