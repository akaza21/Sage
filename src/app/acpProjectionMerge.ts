import type {
  AcpChatPage,
  AcpPageCursor,
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

/**
 * Merges an earlier-history page into the page shown. A history page is
 * anchored by its cursor, not by the live revision, so the page-level fields
 * (revision, title, head session) stay those of the page shown and each Turn,
 * Item and session merges by its own revision. The cursor moves back only
 * when the page extends the oldest edge the page shown still has.
 */
export function mergeHistoryPage(
  current: AcpChatPage,
  incoming: AcpChatPage,
  cursor: AcpPageCursor,
): AcpChatPage {
  return normalizeProjectionPage({
    ...current,
    sessions: mergeById(current.sessions, incoming.sessions),
    turns: mergeTurns(current.turns, incoming.turns),
    nextCursor:
      current.nextCursor === cursor ? incoming.nextCursor : current.nextCursor,
  });
}

/**
 * Lays a newest page over the page shown. Inside the newest page's window it
 * is authoritative; earlier Turns and Items the page shown already loaded stay,
 * with its cursor, so a refresh during a stream never drops loaded history.
 * Retained entities outside the window are not refreshed by the newest page.
 */
export function mergeNewestPage(
  current: AcpChatPage | null,
  incoming: AcpChatPage,
): AcpChatPage {
  const next = normalizeProjectionPage(incoming);
  const first = next.turns[0];
  if (
    !current ||
    !first ||
    next.nextCursor === null ||
    current.projectId !== next.projectId ||
    current.chatId !== next.chatId
  )
    return next;
  const ordinals = new Map(
    [...current.sessions, ...next.sessions].map((s) => [s.id, s.ordinal]),
  );
  const position = (turn: TurnProjection) => [
    ordinals.get(turn.sessionId) ?? Number.MAX_SAFE_INTEGER,
    turn.ordinal,
  ];
  const [firstSession, firstTurn] = position(first);
  const older = current.turns.filter((turn) => {
    const [session, ordinal] = position(turn);
    return (
      session < firstSession ||
      (session === firstSession && ordinal < firstTurn)
    );
  });
  // Retain only when the newest window overlaps the page shown: a window
  // that moved past it would leave Turns or Items no cursor can reach.
  const shown = current.turns.find((turn) => turn.id === first.id);
  const edge = first.items[0]?.ordinal;
  if (
    !shown ||
    (edge !== undefined && !shown.items.some((item) => item.ordinal >= edge))
  )
    return next;
  const earlierItems =
    edge === undefined ? [] : shown.items.filter((item) => item.ordinal < edge);
  if (older.length === 0 && earlierItems.length === 0) return next;
  return normalizeProjectionPage({
    ...next,
    nextCursor: current.nextCursor,
    turns: [
      ...older,
      { ...first, items: [...earlierItems, ...first.items] },
      ...next.turns.slice(1),
    ],
  });
}

function sameTurnData(left: unknown, right: unknown): boolean {
  if (left === right) return true;
  if (
    !left ||
    !right ||
    typeof left !== "object" ||
    typeof right !== "object" ||
    Array.isArray(left) !== Array.isArray(right) ||
    (Array.isArray(left) && left.length !== (right as unknown[]).length)
  )
    return false;
  const before = left as Record<string, unknown>;
  const after = right as Record<string, unknown>;
  const keys = Object.keys(before);
  return (
    keys.length === Object.keys(after).length &&
    keys.every(
      (key) =>
        Object.hasOwn(after, key) && sameTurnData(before[key], after[key]),
    )
  );
}

export function retainUnchangedTurns(
  current: AcpChatPage | null,
  incoming: AcpChatPage,
): AcpChatPage {
  if (
    !current ||
    current.projectId !== incoming.projectId ||
    current.chatId !== incoming.chatId
  )
    return incoming;
  const previous = new Map(current.turns.map((turn) => [turn.id, turn]));
  return {
    ...incoming,
    turns: incoming.turns.map((turn) => {
      const retained = previous.get(turn.id);
      return retained?.revision === turn.revision &&
        sameTurnData(retained, turn)
        ? retained
        : turn;
    }),
  };
}
