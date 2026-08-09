import type {
  AcpChatPage,
  AgentSessionProjection,
  SageTurnId,
  TurnProjection,
} from "../../../domain/acp";

const cancellableStates = new Set<TurnProjection["state"]>([
  "queued",
  "sending",
  "running",
  "waitingModel",
  "waitingTool",
  "waitingApproval",
  "waitingUser",
]);

function currentSession(page: AcpChatPage): AgentSessionProjection | null {
  if (!page.currentSessionId) return null;
  return (
    page.sessions.find((session) => session.id === page.currentSessionId) ??
    null
  );
}

export function cancellableTurnId(page: AcpChatPage): SageTurnId | null {
  const session = currentSession(page);
  if (!session?.runtimeEpoch || !session.capabilities.cancel) return null;
  const active = page.turns
    .filter(
      (turn) =>
        turn.sessionId === session.id &&
        turn.runtimeEpoch === session.runtimeEpoch &&
        cancellableStates.has(turn.state),
    )
    .sort((left, right) => right.ordinal - left.ordinal);
  return active.length === 1 ? active[0].id : null;
}
