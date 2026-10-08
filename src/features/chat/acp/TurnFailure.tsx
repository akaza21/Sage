import { useId } from "react";

import type { TurnProjection } from "../../../domain/acp";

export function TurnFailure({
  turn,
  onRetry,
}: {
  turn: TurnProjection;
  onRetry?: () => void;
}) {
  const headingId = useId();
  if (turn.state !== "failed") return null;
  return (
    <div className="acp-turn__failure" role="group" aria-labelledby={headingId}>
      <strong id={headingId}>Turn failed</strong>
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
