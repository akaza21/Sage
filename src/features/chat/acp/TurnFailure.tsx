import { useId } from "react";

import type { TurnProjection } from "../../../domain/acp";
import { turnTerminalReasonLabels } from "./statusSemantics";

export function TurnFailure({
  turn,
  onRetry,
}: {
  turn: TurnProjection;
  onRetry?: () => void;
}) {
  const headingId = useId();
  if (turn.state !== "failed") return null;
  const error = [...turn.items].reverse().find((item) => item.kind === "error");
  const message =
    turn.terminalReason?.message?.trim() ||
    error?.payload.message?.trim() ||
    (turn.terminalReason
      ? turnTerminalReasonLabels[turn.terminalReason.kind]
      : "Reason unavailable");
  return (
    <div className="acp-turn__failure" role="group" aria-labelledby={headingId}>
      <strong id={headingId}>Turn failed</strong>
      <p data-turn-reason-message="">{message}</p>
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
