import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { HarnessSignInPart } from "../lazyParts";

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

export function TurnFailure(props: TurnFailureProps) {
  return props.turn.state === "failed" ? (
    <HarnessSignInPart
      part="TurnFailureDetails"
      {...props}
      fallback={
        props.onRetry ? <RetryTurnButton onRetry={props.onRetry} /> : null
      }
    />
  ) : null;
}
