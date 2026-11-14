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

export function useSessionControl(boundChatId?: string | null) {
  const [status, setStatus] = useState<SessionControlStatus>({ kind: "idle" });
  const pending = useRef(false);
  const run = useCallback(
    async (control: SessionControlIntent, done: string) => {
      if (pending.current) return;
      const context = currentContext(boundChatId);
      const session = context?.page.sessions.find(
        (item) => item.id === context.page.currentSessionId,
      );
      if (!context || !session?.runtimeEpoch) {
        setStatus({ kind: "failed", message: "The session is not running." });
        return;
      }
      pending.current = true;
      setStatus({ kind: "pending" });
      try {
        const summary = await controlAcpSession({
          ...context.scope,
          projectGeneration: context.commandProjectGeneration,
          sessionId: session.id,
          runtimeEpoch: session.runtimeEpoch,
          control,
        });
        setStatus({ kind: "done", message: summary ?? done });
      } catch (error) {
        setStatus({
          kind: "failed",
          message:
            error instanceof AcpDesktopError
              ? error.message
              : "The request failed.",
        });
      } finally {
        pending.current = false;
      }
    },
    [boundChatId],
  );
  return { status, run };
}
