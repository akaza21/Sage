import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { HarnessSignInPart } from "../lazyParts";
import {
  turnReasonErrorItem,
  turnReasonMessage,
  turnTerminalReasonLabels,
} from "./statusSemantics";
import { Notice } from "../../../ui/Notice";

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
  if (turn.state !== "failed") return null;
  const reason = turn.terminalReason;
  const message =
    turnReasonMessage(turn) ||
    (reason && reason.kind !== "other"
      ? turnTerminalReasonLabels[reason.kind]
      : "The harness gave no error details.");
  const error = turnReasonErrorItem(turn);
  return (
    <div role="group" aria-label="Turn failed">
      <Notice
        identity={JSON.stringify(["turn-failure", turn.sessionId, turn.id])}
        severity="blocking"
        message={
          <>
            <span data-turn-reason-message="">{message}</span>
            {reason?.remediation ? (
              <>
                {" "}
                <span data-turn-reason-remediation="">
                  {reason.remediation}
                </span>
              </>
            ) : null}
          </>
        }
        announce={false}
        details={
          reason || error ? (
            <FailureDetails turn={turn} page={page} />
          ) : undefined
        }
        action={onRetry ? { label: "Retry", onClick: onRetry } : undefined}
      />
    </div>
  );
}

function FailureDetails({ turn, page }: TurnFailureProps) {
  return (
    <>
      <pre>
        {JSON.stringify(
          {
            reason: turn.terminalReason,
            error: turnReasonErrorItem(turn)?.payload,
          },
          null,
          2,
        )}
      </pre>
      {turn.terminalReason?.kind === "authRequired" && page ? (
        <HarnessSignInPart
          part="HarnessTurnSignInRecovery"
          page={page}
          turn={turn}
        />
      ) : null}
    </>
  );
}
