import { useId } from "react";
import { RetryTurnButton, type TurnFailureProps } from "./TurnFailure";
import { turnReasonMessage, turnTerminalReasonLabels } from "./statusSemantics";
import { HarnessTurnSignInRecovery } from "../../harness/HarnessTurnSignInRecovery";

export function TurnFailureDetails({ turn, onRetry, page }: TurnFailureProps) {
  const headingId = useId();
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
      {turn.terminalReason?.kind === "authRequired" && page ? (
        <HarnessTurnSignInRecovery page={page} turn={turn} />
      ) : null}
      {onRetry ? <RetryTurnButton onRetry={onRetry} /> : null}
    </div>
  );
}
