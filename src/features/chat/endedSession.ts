import type { AcpChatPage } from "../../domain/acpProjection";

export interface EndedSession {
  id: string;
  bindingId: string;
}

/** A chat's sessions, read for the one it ran last. */
export type EndedPage = Pick<AcpChatPage, "currentSessionId" | "sessions">;

/**
 * The session a chat with no head ran last. The session its next send
 * starts on the same binding takes that session's access, unless the access
 * needs a confirmation (`store::preceding_session`, `carry_chat_access`).
 */
export function endedSession(
  page: EndedPage | null | undefined,
): EndedSession | undefined {
  if (!page || page.currentSessionId) return undefined;
  let last: AcpChatPage["sessions"][number] | undefined;
  for (const session of page.sessions) {
    if (!session.legacy && (!last || session.ordinal > last.ordinal))
      last = session;
  }
  return last && { id: last.id, bindingId: last.bindingId };
}
