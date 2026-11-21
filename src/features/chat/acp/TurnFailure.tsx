import { useId } from "react";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { HarnessSignInPart } from "../lazyParts";
import { turnReasonMessage, turnTerminalReasonLabels } from "./statusSemantics";

export interface TurnFailureProps {
  turn: TurnProjection;
  onRetry?: () => void;
  page?: AcpChatPage;
}

export function RetryTurnButton({ onRetry }: { onRetry: () => void }) {
  return (
    <button className="acp-turn__retry" type="button" onClick={onRetry}>
      Retry
    </button>
  );
}

export function TurnFailure({ turn, onRetry, page }: TurnFailureProps) {
  const headingId = useId();
  if (turn.state !== "failed") return null;
  const reason = turn.terminalReason;
  const message =
    turnReasonMessage(turn) ||
    (reason && reason.kind !== "other"
      ? turnTerminalReasonLabels[reason.kind]
      : "The harness gave no error details.");
  return (
    <div className="acp-turn__failure" role="group" aria-labelledby={headingId}>
      <strong id={headingId}>Turn failed</strong>
      <p data-turn-reason-message="">{message}</p>
      {reason?.remediation ? (
        <p data-turn-reason-remediation="">{reason.remediation}</p>
      ) : null}
      {reason?.kind === "authRequired" && page ? (
        <HarnessSignInPart
          part="HarnessTurnSignInRecovery"
          page={page}
          turn={turn}
        />
      ) : null}
      {onRetry ? <RetryTurnButton onRetry={onRetry} /> : null}
    </div>
  );
}
