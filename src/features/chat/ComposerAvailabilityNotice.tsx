import { Notice } from "../../ui/Notice";
import { Stacked, type StackEntry } from "../../ui/NoticeStack";

/**
 * Send's disabled reason and a failed load's Retry, for the notice stack.
 * When a picker or recovery action explains the refusal, the reason stays
 * an sr-only description for Send and does not make the stack visible.
 */
export function composerAvailability({
  reason,
  id,
  scope,
  compact,
  onRetry,
}: {
  reason: string | null;
  id: string;
  scope: string;
  compact: boolean;
  onRetry?: (() => void) | null;
}): StackEntry[] {
  const quiet = compact || !!onRetry;
  return [
    !!reason && (
      <Stacked key={id} quiet={quiet}>
        <p id={id} className={quiet ? "sr-only" : "composer__disabled-reason"}>
          {reason}
        </p>
      </Stacked>
    ),
    !!onRetry && (
      <Notice
        key={`${scope}:chat-load`}
        identity={`${scope}:chat-load`}
        severity="blocking"
        message="This chat failed to load."
        action={{ label: "Retry", onClick: onRetry }}
      />
    ),
  ];
}
