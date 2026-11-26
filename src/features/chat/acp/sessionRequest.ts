import { invokeAcp } from "../../../services/acpDesktopCommand";
import { currentContext } from "./useAcpChatActions";

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

/**
 * Drives the chat's start that ended needing recovery to completion. Success
 * shows in the projection: the session becomes one the next send starts.
 */
export function recoverStart(chatId?: string | null) {
  const request = sessionRequest(chatId, { kind: "recoverStart" });
  if (request)
    void invokeAcp("control_acp_session", request).catch(() => undefined);
}
