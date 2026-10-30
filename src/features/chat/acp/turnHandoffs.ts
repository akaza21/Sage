import type { AcpChatPage, TurnProjection } from "../../../domain/acp";

// Decoded pages carry fresh handoff arrays; only this Turn's bounded (<=32)
// summaries may invalidate its memoized row.
export function sameTurnHandoffs(
  left: AcpChatPage,
  right: AcpChatPage,
  turnId: TurnProjection["id"],
): boolean {
  if (left.handoffs === right.handoffs) return true;
  const before =
    left.handoffs?.filter((handoff) => handoff.turnId === turnId) ?? [];
  const after =
    right.handoffs?.filter((handoff) => handoff.turnId === turnId) ?? [];
  return (
    before.length === after.length &&
    before.every(
      (handoff, index) =>
        handoff.id === after[index].id &&
        handoff.revision === after[index].revision &&
        handoff.state === after[index].state,
    )
  );
}
