import { useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Check, Copy } from "../../ui/leanIcons";

import { AnswerCopyFeedback } from "./answerCopyFeedback";
import { chatTimestamp, timeOfDayLabel } from "./chatTimestamp";

/**
 * Codex's row under a finished answer (s2/f0053, f0093): Copy, any extra
 * actions, then the answer's own time, muted, on hover or focus. Copy acts
 * on this answer's text only.
 */
export function AnswerActions({
  text,
  createdAt,
  children,
}: {
  text: string;
  createdAt?: number;
  children?: ReactNode;
}) {
  const onCopyFeedback = useContext(AnswerCopyFeedback);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<number | undefined>(undefined);
  const copyRequest = useRef(0);
  useEffect(
    () => () => {
      copyRequest.current++;
      window.clearTimeout(copyTimer.current);
    },
    [],
  );
  const CopyIcon = copied ? Check : Copy;
  const time = createdAt === undefined ? null : chatTimestamp(createdAt);

  async function copyAnswer() {
    const request = ++copyRequest.current;
    window.clearTimeout(copyTimer.current);
    let succeeded = true;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      succeeded = false;
    }
    if (request !== copyRequest.current) return;
    setCopied(succeeded);
    onCopyFeedback?.(succeeded ? "Copied" : "Copy failed");
    if (succeeded)
      copyTimer.current = window.setTimeout(() => setCopied(false), 1_200);
  }

  return (
    <div
      className="chat-transcript-actions"
      role="group"
      aria-label="Message actions"
    >
      <button
        type="button"
        aria-label={copied ? "Copied" : "Copy"}
        title={copied ? "Copied" : "Copy"}
        onClick={() => void copyAnswer()}
      >
        <CopyIcon size={14} aria-hidden="true" />
      </button>
      {children}
      {time ? (
        <time
          className="chat-transcript-actions__time"
          dateTime={time.dateTime}
        >
          {timeOfDayLabel(createdAt!)}
        </time>
      ) : null}
    </div>
  );
}
