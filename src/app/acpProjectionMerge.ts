import type {
  AcpChatPage,
  EntityRevision,
  TurnProjection,
} from "../domain/acp";

export function compareDecimalRevision(
  left: string,
  right: string,
): number | null {
  const canonical = /^(?:0|[1-9][0-9]*)$/;
  if (!canonical.test(left) || !canonical.test(right)) return null;
  if (left.length !== right.length) return left.length < right.length ? -1 : 1;
  return left === right ? 0 : left < right ? -1 : 1;
}

function mergeById<T extends { id: string; revision: EntityRevision }>(
  current: readonly T[],
  incoming: readonly T[],
): T[] {
  const merged = new Map(current.map((value) => [value.id, value]));
  for (const value of incoming) {
    const previous = merged.get(value.id);
    if (
      !previous ||
      (compareDecimalRevision(value.revision, previous.revision) ?? -1) >= 0
    ) {
      merged.set(value.id, value);
    }
  }
  return [...merged.values()];
}

function mergeTurn(
  current: TurnProjection,
  incoming: TurnProjection,
): TurnProjection {
  const comparison = compareDecimalRevision(
    incoming.revision,
    current.revision,
  );
  const authority = comparison !== null && comparison >= 0 ? incoming : current;
  return {
    ...authority,
    items: mergeById(current.items, incoming.items).sort(
      (left, right) =>
        left.ordinal - right.ordinal || left.id.localeCompare(right.id),
    ),
    activities: mergeById(current.activities, incoming.activities),
  };
}

function mergeTurns(
  current: readonly TurnProjection[],
  incoming: readonly TurnProjection[],
): TurnProjection[] {
  const merged = new Map(current.map((turn) => [turn.id, turn]));
  for (const turn of incoming) {
    const previous = merged.get(turn.id);
    merged.set(turn.id, previous ? mergeTurn(previous, turn) : turn);
  }
  return [...merged.values()];
}

export function normalizeProjectionPage(page: AcpChatPage): AcpChatPage {
  const sessions = page.sessions
    .slice()
    .sort(
      (left, right) =>
        left.ordinal - right.ordinal || left.id.localeCompare(right.id),
    );
  const sessionOrdinals = new Map(
    sessions.map((session) => [session.id, session.ordinal]),
  );
  const turns = page.turns
    .map((turn) => ({
      ...turn,
      items: turn.items
        .slice()
        .sort(
          (left, right) =>
            left.ordinal - right.ordinal || left.id.localeCompare(right.id),
        ),
      activities: turn.activities.slice(),
    }))
    .sort(
      (left, right) =>
        (sessionOrdinals.get(left.sessionId) ?? Number.MAX_SAFE_INTEGER) -
          (sessionOrdinals.get(right.sessionId) ?? Number.MAX_SAFE_INTEGER) ||
        left.ordinal - right.ordinal ||
        left.id.localeCompare(right.id),
    );
  return { ...page, sessions, turns };
}

export function mergeProjectionPage(
  current: AcpChatPage,
  incoming: AcpChatPage,
): AcpChatPage {
  return normalizeProjectionPage({
    ...incoming,
    currentSessionId: incoming.currentSessionId ?? current.currentSessionId,
    sessions: mergeById(current.sessions, incoming.sessions),
    turns: mergeTurns(current.turns, incoming.turns),
  });
}
