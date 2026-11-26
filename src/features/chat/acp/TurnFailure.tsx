import { useState } from "react";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { HarnessSignInPart } from "../lazyParts";
import {
  turnReasonErrorItem,
  turnReasonMessage,
  turnTerminalReasonLabels,
} from "./statusSemantics";
import { Notice } from "../../../ui/Notice";
import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

export interface TurnFailureProps {
  turn: TurnProjection;
  onRetry?: () => void;
  page?: AcpChatPage;
  /** An earlier failure is one line; its remediation waits under Details. */
  latest?: boolean;
}

// Disposable WebView state: the newest 64 dismissed turns. A turn id belongs
// to one chat, so this holds each chat's dismissals without a chat key.
const dismissed: string[] = [];

export function RetryTurnButton({ onRetry }: { onRetry: () => void }) {
  return (
    <button className="acp-turn__retry" type="button" onClick={onRetry}>
      Retry
    </button>
  );
}

export function TurnFailure({
  turn,
  onRetry,
  page,
  latest = true,
}: TurnFailureProps) {
  const [hidden, setHidden] = useState(() => dismissed.includes(turn.id));
  if (turn.state !== "failed" || hidden) return null;
  const reason = turn.terminalReason;
  const message =
    turnReasonMessage(turn) ||
    (reason && reason.kind !== "other"
      ? turnTerminalReasonLabels[reason.kind]
      : "The harness gave no error details.");
  const error = turnReasonErrorItem(turn);
  return (
    <div role="group" aria-label="Turn failed" data-latest={latest}>
      <Notice
        identity={turn.id}
        severity="blocking"
        message={
          <>
            <span data-turn-reason-message="">{message}</span>
            {reason?.remediation ? (
              <span data-turn-reason-remediation=""> {reason.remediation}</span>
            ) : null}
          </>
        }
        announce={false}
        details={
          reason || error ? (
            <LazyModuleBoundary
              label="Failure details"
              load={HarnessSignInPart.preload}
              render={(Details) => (
                <Details turn={turn} page={page} error={error?.payload} />
              )}
            />
          ) : undefined
        }
        action={onRetry ? { label: "Retry", onClick: onRetry } : undefined}
        // The latest failure holds the turn's only Retry, so it stays.
        onDismiss={
          latest
            ? undefined
            : () => {
                if (dismissed.push(turn.id) > 64) dismissed.shift();
                setHidden(true);
              }
        }
      />
    </div>
  );
}
