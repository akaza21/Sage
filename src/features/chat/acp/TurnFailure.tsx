import { useId } from "react";

import type { TurnProjection } from "../../../domain/acp";
import { turnReasonMessage, turnTerminalReasonLabels } from "./statusSemantics";

export function TurnFailure({
  turn,
  onRetry,
}: {
  turn: TurnProjection;
  onRetry?: () => void;
}) {
  const headingId = useId();
  if (turn.state !== "failed") return null;
  const message =
    turnReasonMessage(turn) ||
    (turn.terminalReason && turn.terminalReason.kind !== "other"
      ? turnTerminalReasonLabels[turn.terminalReason.kind]
      : "The harness gave no error details.");
  return (
    <div className="acp-turn__failure" role="group" aria-labelledby={headingId}>
      <strong id={headingId}>Turn failed</strong>
      <p data-turn-reason-message="">{message}</p>
      {turn.terminalReason?.remediation ? (
        <p data-turn-reason-remediation="">{turn.terminalReason.remediation}</p>
      ) : null}
      {onRetry ? (
        <button className="acp-turn__retry" type="button" onClick={onRetry}>
          Retry
        </button>
      ) : null}
    </div>
  );
}
