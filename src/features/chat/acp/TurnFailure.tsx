import type { TurnProjection } from "../../../domain/acp";

export function TurnFailure({
  turn,
  onRetry,
}: {
  turn: TurnProjection;
  onRetry?: () => void;
}) {
  if (turn.state !== "failed") return null;
  return (
    <div className="acp-turn__failure" role="alert" aria-label="Turn failed">
      <strong>Turn failed</strong>
      {turn.terminalReason?.message ? (
        <p data-turn-reason-message="">{turn.terminalReason.message}</p>
      ) : null}
      {turn.terminalReason?.remediation ? (
        <p data-turn-reason-remediation="">{turn.terminalReason.remediation}</p>
      ) : null}
      {onRetry ? (
        <button type="button" onClick={onRetry}>
          Retry
        </button>
      ) : null}
    </div>
  );
}
