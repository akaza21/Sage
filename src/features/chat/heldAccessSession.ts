import type { AcpChatPage } from "../../domain/acpProjection";

/**
 * The session whose access a chat's next prompt runs under while no live
 * session reports it: the chat's current session, else its newest one on
 * `bindingId`, whose access the session that replaces it takes on
 * (`session_access/lineage.rs`).
 */
export function heldAccessSession(
  page: Pick<AcpChatPage, "currentSessionId" | "sessions"> | null | undefined,
  bindingId: string | null | undefined,
): string | undefined {
  if (!page) return undefined;
  if (page.currentSessionId) return page.currentSessionId;
  let newest: AcpChatPage["sessions"][number] | undefined;
  for (const session of page.sessions) {
    if (
      session.bindingId === bindingId &&
      (!newest || session.ordinal > newest.ordinal)
    )
      newest = session;
  }
  return newest?.id;
}
