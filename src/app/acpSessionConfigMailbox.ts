import type { AcpChatPage } from "../domain/acp";
import {
  decodeSessionConfigNotice,
  type AcpSessionConfigChangedV1,
} from "../domain/acpSessionConfig";
import type { AcpProjectionScope } from "./acpProjectionStore";
import { compareDecimalRevision } from "./acpProjectionMerge";

export function configNoticeMatches(
  scope: AcpProjectionScope | null,
  page: AcpChatPage | null,
  notice: AcpSessionConfigChangedV1,
): boolean {
  if (
    !decodeSessionConfigNotice(notice) ||
    scope?.projectId !== notice.sageProjectId ||
    scope.chatId !== notice.sageChatId
  )
    return false;
  if (!page) return true; // A bootstrap-racing notice remains only a wake hint.
  const owner = page.sessions.find(
    (session) => session.id === page.currentSessionId,
  );
  return (
    owner?.id === notice.sageAgentSessionId &&
    owner.runtimeEpoch === notice.runtimeEpoch &&
    owner.residency === "hot" &&
    ["ready", "active"].includes(owner.state)
  );
}

export function sameRevisionOwner(
  current: AcpChatPage | null,
  incoming: AcpChatPage,
): boolean {
  if (!current || current.projectionRevision !== incoming.projectionRevision)
    return true;
  const previous = current.sessions.find(
    (s) => s.id === current.currentSessionId,
  );
  const next = incoming.sessions.find(
    (s) => s.id === incoming.currentSessionId,
  );
  return (
    current.currentSessionId === incoming.currentSessionId &&
    previous?.runtimeEpoch === next?.runtimeEpoch
  );
}

// Durable freshness is checked by the caller first. A delayed snapshot at the same
// durable revision must never roll back this independent live revision stream.
export function preserveNewerConfig(
  current: AcpChatPage | null,
  incoming: AcpChatPage,
): AcpChatPage {
  const old = current?.sessionConfig;
  if (!current || !old) return incoming;
  const owner = incoming.sessions.find(
    (s) => s.id === incoming.currentSessionId,
  );
  if (
    owner?.id !== old.sageAgentSessionId ||
    owner.runtimeEpoch !== old.runtimeEpoch ||
    owner.residency !== "hot" ||
    !["ready", "active"].includes(owner.state)
  )
    return incoming;
  const next = incoming.sessionConfig;
  const configComparison = next
    ? compareDecimalRevision(next.configRevision, old.configRevision)
    : null;
  // Removal and presence share one exact-owner revision stream. A delayed
  // present page must not resurrect a newer tombstone, and a stale tombstone
  // must not erase a newer present value.
  if (
    next
      ? (configComparison ?? -1) < 0 ||
        (configComparison === 0 && old.removed && !next.removed)
      : current.projectionRevision === incoming.projectionRevision
  ) {
    return {
      ...incoming,
      sessionConfig: old,
      sessions: incoming.sessions.map((s) =>
        s.id === owner.id
          ? {
              ...s,
              capabilities: {
                ...s.capabilities,
                configCount: old.removed ? 0 : old.configOptions.length,
              },
            }
          : s,
      ),
    };
  }
  return incoming;
}
