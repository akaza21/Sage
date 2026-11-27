import type { ReactNode } from "react";
import { Notice } from "../../ui/Notice";
import type { StackEntry } from "../../ui/NoticeStack";

/**
 * Send keeps an accessible reason when the explanation belongs to a picker
 * or recovery action: that reason stays an sr-only description where it is,
 * while a visible reason and a failed load's Retry join the notice stack.
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
}): [described: ReactNode, notices: StackEntry[]] {
  const quiet = compact || !!onRetry;
  const text = reason ? (
    <p id={id} className={quiet ? "sr-only" : "composer__disabled-reason"}>
      {reason}
    </p>
  ) : null;
  return [
    quiet && text,
    [
      !quiet &&
        !!text && { key: "disabled-reason", severity: "info", node: text },
      !!onRetry && {
        key: `${scope}:chat-load`,
        severity: "blocking",
        node: (
          <Notice
            identity={`${scope}:chat-load`}
            severity="blocking"
            message="This chat failed to load."
            action={{ label: "Retry", onClick: onRetry }}
          />
        ),
      },
    ],
  ];
}
