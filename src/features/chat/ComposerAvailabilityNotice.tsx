import type { ReactNode } from "react";
import { Notice } from "../../ui/Notice";
import type { ComposerStatusNoticeProps } from "./ComposerStatusNotices";

/**
 * Send's disabled reason and a failed load's Retry, for the composer's
 * notice stack: Retry leads it, a visible reason closes it. When a picker or
 * the Retry explains the refusal, the reason is only Send's sr-only
 * description, rendered in place.
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
}): [
  described: ReactNode,
  notices: Pick<ComposerStatusNoticeProps, "before" | "after">,
] {
  const quiet = compact || !!onRetry;
  const text = reason && (
    <p id={id} className={quiet ? "sr-only" : "composer__disabled-reason"}>
      {reason}
    </p>
  );
  return [
    quiet && text,
    {
      before: onRetry && (
        <Notice
          identity={`${scope}:chat-load`}
          severity="blocking"
          message="This chat failed to load."
          action={{ label: "Retry", onClick: onRetry }}
        />
      ),
      after: !quiet && text,
    },
  ];
}
