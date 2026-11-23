import { Notice } from "../../ui/Notice";

/** Send keeps an accessible reason when the explanation belongs to a picker or recovery action. */
export function ComposerAvailabilityNotice({
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
}) {
  return (
    <>
      {reason && (
        <p
          id={id}
          className={
            compact || onRetry ? "sr-only" : "composer__disabled-reason"
          }
        >
          {reason}
        </p>
      )}
      {onRetry && (
        <Notice
          identity={`${scope}:chat-load`}
          severity="blocking"
          message="This chat failed to load."
          action={{ label: "Retry", onClick: onRetry }}
        />
      )}
    </>
  );
}
