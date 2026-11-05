import type { TurnProjection } from "../../../domain/acp";
import {
  getTurnStatusLabel,
  turnTerminalReasonLabels,
  turnReasonMessage,
} from "./statusSemantics";

export function TurnHeader({ turn }: { turn: TurnProjection }) {
  const reason = turn.terminalReason;
  const stopped = turn.state === "interrupted" || turn.state === "cancelled";
  const userStopped = stopped && reason?.kind === "cancelledByUser";
  const label = getTurnStatusLabel(turn);
  const failed = turn.state === "failed";
  const message = failed
    ? undefined
    : turnReasonMessage(turn) ||
      (reason && !userStopped
        ? turnTerminalReasonLabels[reason.kind]
        : undefined);
  const remediation = reason?.remediation;
  return (
    <header className="acp-turn__header">
      <span
        className={stopped ? "acp-turn__reason" : "acp-state acp-turn__state"}
        role="status"
        aria-label={label}
        aria-live="off"
        aria-atomic="true"
        data-turn-status={turn.id}
        data-turn-reason={userStopped ? reason.kind : undefined}
        tabIndex={-1}
      >
        {label}
      </span>
      {!failed && (message || remediation) ? (
        <div
          className="acp-turn__reason-detail"
          data-turn-reason={reason && !userStopped ? reason.kind : undefined}
        >
          {message ? (
            <p className="acp-turn__reason-text" data-turn-reason-message="">
              {message}
            </p>
          ) : null}
          {remediation ? (
            <p
              className="acp-turn__reason-text acp-turn__reason-text--remediation"
              data-turn-reason-remediation=""
            >
              {remediation}
            </p>
          ) : null}
        </div>
      ) : null}
    </header>
  );
}
