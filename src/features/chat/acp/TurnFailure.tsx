import { useId, useState } from "react";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { HarnessSignInPart } from "../lazyParts";
import { turnReasonMessage, turnTerminalReasonLabels } from "./statusSemantics";
import { Notice } from "../../../ui/Notice";
import { turnFailureIdentity, turnFailureTarget } from "./noticeOwnership";

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
  const [initialTurn] = useState({ id: turn.id, state: turn.state });
  if (turn.state !== "failed") return null;
  const reason = turn.terminalReason;
  const message =
    turnReasonMessage(turn) ||
    (reason && reason.kind !== "other"
      ? turnTerminalReasonLabels[reason.kind]
      : "The harness gave no error details.");
  return (
    <div
      id={turnFailureTarget(turn)}
      tabIndex={-1}
      role="group"
      aria-labelledby={headingId}
    >
      <strong id={headingId}>Turn failed</strong>
      <Notice
        identity={turnFailureIdentity(turn)}
        severity="blocking"
        message={<span data-turn-reason-message="">{message}</span>}
        announce={initialTurn.id === turn.id && initialTurn.state !== "failed"}
        action={onRetry ? { label: "Retry", onClick: onRetry } : undefined}
      />
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
    </div>
  );
}
