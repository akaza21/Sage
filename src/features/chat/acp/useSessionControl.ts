import { useCallback, useRef, useState } from "react";
import { AcpDesktopError } from "../../../services/acpDesktop";
import {
  controlAcpSession,
  type SessionControlIntent,
} from "../../../services/acpSessionSurface";
import { currentContext } from "./useAcpChatActions";

export type SessionControlStatus =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "done"; message: string }
  | { kind: "failed"; message: string };

/** `control` addressed to the chat's running session, or `null` when none runs. */
export function sessionRequest<Control>(
  chatId: string | null | undefined,
  control: Control,
) {
  const context = currentContext(chatId);
  const session = context?.page.sessions.find(
    (item) => item.id === context.page.currentSessionId,
  );
  return context && session?.runtimeEpoch
    ? {
        ...context.scope,
        projectGeneration: context.commandProjectGeneration,
        sessionId: session.id,
        runtimeEpoch: session.runtimeEpoch,
        control,
      }
    : null;
}

export function useSessionControl(boundChatId?: string | null) {
  const [status, setStatus] = useState<SessionControlStatus>({ kind: "idle" });
  const pending = useRef(false);
  const run = useCallback(
    async (control: SessionControlIntent, done: string) => {
      if (pending.current) return false;
      const request = sessionRequest(boundChatId, control);
      if (!request) {
        setStatus({ kind: "failed", message: "The session is not running." });
        return false;
      }
      pending.current = true;
      setStatus({ kind: "pending" });
      try {
        const summary = await controlAcpSession(request);
        setStatus({ kind: "done", message: summary ?? done });
        return true;
      } catch (error) {
        setStatus({
          kind: "failed",
          message:
            error instanceof AcpDesktopError
              ? error.message
              : "The request failed.",
        });
        return false;
      } finally {
        pending.current = false;
      }
    },
    [boundChatId],
  );
  return { status, run };
}

/**
 * Drives the chat's start that ended needing recovery to completion. Success
 * shows in the projection: the session becomes one the next send starts.
 */
export function recoverStart(chatId?: string | null) {
  const request = sessionRequest(chatId, { kind: "recoverStart" } as const);
  if (request) void controlAcpSession(request).catch(() => undefined);
}
