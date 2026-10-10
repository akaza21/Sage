import type { TurnProjection } from "../../../domain/acp";
import { turnStateLabels, turnTerminalReasonLabels } from "./statusSemantics";

export function TurnHeader({ turn }: { turn: TurnProjection }) {
  const reason = turn.terminalReason;
  const stopped = turn.state === "interrupted" || turn.state === "cancelled";
  const userStopped = stopped && reason?.kind === "cancelledByUser";
  const label = userStopped
    ? turnTerminalReasonLabels.cancelledByUser
    : turnStateLabels[turn.state];
  const message = reason?.message;
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
      {reason && !userStopped ? (
        <span className="acp-turn__reason" data-turn-reason={reason.kind}>
          {turnTerminalReasonLabels[reason.kind]}
        </span>
      ) : null}
      {turn.state !== "failed" && (message || remediation) ? (
        <div className="acp-turn__reason-detail">
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
