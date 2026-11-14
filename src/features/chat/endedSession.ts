import { useMemo } from "react";
import type { AcpChatPage } from "../../domain/acpProjection";

export interface EndedSession {
  id: string;
  bindingId: string;
}

/**
 * The session a chat with no head ran last. The session its next send
 * starts on the same binding takes that session's access, unless the access
 * needs a confirmation (`store::preceding_session`, `carry_chat_access`).
 */
export function endedSession(
  page: Pick<AcpChatPage, "currentSessionId" | "sessions"> | null | undefined,
): EndedSession | undefined {
  if (!page || page.currentSessionId) return undefined;
  let last: AcpChatPage["sessions"][number] | undefined;
  for (const session of page.sessions) {
    if (!session.legacy && (!last || session.ordinal > last.ordinal))
      last = session;
  }
  return last && { id: last.id, bindingId: last.bindingId };
}

/** The chat's `endedSession`, kept the same object while it is the same. */
export function useEndedSession(
  page: Pick<AcpChatPage, "currentSessionId" | "sessions"> | null | undefined,
): EndedSession | undefined {
  const ended = endedSession(page);
  const id = ended?.id;
  const bindingId = ended?.bindingId;
  return useMemo(
    () => (id && bindingId ? { id, bindingId } : undefined),
    [id, bindingId],
  );
}
