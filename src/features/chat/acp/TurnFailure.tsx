import { useId } from "react";

import type { TurnProjection } from "../../../domain/acp";
import { getTurnFailure } from "./statusSemantics";

export function TurnFailure({
  turn,
  onRetry,
}: {
  turn: TurnProjection;
  onRetry?: () => void;
}) {
  const headingId = useId();
  const failure = getTurnFailure(turn);
  if (!failure) return null;
  return (
    <div className="acp-turn__failure" role="group" aria-labelledby={headingId}>
      <strong id={headingId}>Turn failed</strong>
      <p data-turn-reason-message="">{failure.message}</p>
      {failure.remediation ? (
        <p data-turn-reason-remediation="">{failure.remediation}</p>
      ) : null}
      {onRetry ? (
        <button className="acp-turn__retry" type="button" onClick={onRetry}>
          Retry
        </button>
      ) : null}
    </div>
  );
}
