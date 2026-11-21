import {
  SESSION_NOTICES_MAX,
  type SessionNotice,
  type TurnProjection,
} from "../../../domain/acpProjection";
import { turnReasonErrorItem, turnReasonMessage } from "./statusSemantics";

const RANK = { info: 0, warning: 1, error: 2 };

export function sessionNoticeProblems(records: readonly SessionNotice[]) {
  const problems = new Map<string, SessionNotice & { identity: string }>();
  for (const [index, notice] of records.slice(-SESSION_NOTICES_MAX).entries()) {
    // Native rows have durable IDs; unidentified external records stay distinct.
    const identity = notice.identity ?? `legacy-record:${index}`;
    const previous = problems.get(identity);
    if (!previous || RANK[notice.level] >= RANK[previous.level])
      problems.set(identity, { ...notice, identity });
  }
  return [...problems.values()];
}

export const turnFailureIdentity = (
  turn: Pick<TurnProjection, "sessionId" | "id">,
) => JSON.stringify(["turn-failure", turn.sessionId, turn.id]);

export const turnFailureTarget = (
  turn: Pick<TurnProjection, "sessionId" | "id">,
) =>
  `turn-failure-${encodeURIComponent(turn.sessionId)}-${encodeURIComponent(turn.id)}`;

export function turnFailureItem(turn: TurnProjection) {
  if (turn.state !== "failed") return;
  const item = turnReasonErrorItem(turn);
  return item?.payload.message?.trim() === turnReasonMessage(turn)
    ? item
    : undefined;
}
