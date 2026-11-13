import { useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Check, Copy } from "../../ui/leanIcons";

import type { AcpChatPage } from "../../domain/acp";
import { AnswerCopyFeedback } from "./answerCopyFeedback";
import { chatTimestamp } from "./chatTimestamp";
import { ForkPart } from "./lazyParts";

export function AnswerActions({
  text,
  createdAt,
  page,
  forkable = false,
  children,
}: {
  text: string;
  createdAt?: number;
  /** The native Chat whose answer this is: its row gets the "…" menu. */
  page?: AcpChatPage | null;
  /** Only the latest answer can fork; its menu then offers the branch. */
  forkable?: boolean;
  children?: ReactNode;
}) {
  const onCopyFeedback = useContext(AnswerCopyFeedback);
  const [copiedUntil, setCopiedUntil] = useState(0);
  const copyRequest = useRef(0);
  useEffect(
    () => () => {
      copyRequest.current++;
    },
    [],
  );
  useEffect(() => {
    if (!copiedUntil) return;
    const timer = window.setTimeout(
      () => setCopiedUntil(0),
      Math.max(0, copiedUntil - performance.now()),
    );
    return () => window.clearTimeout(timer);
  }, [copiedUntil]);
  const copied = copiedUntil > 0;
  const CopyIcon = copied ? Check : Copy;
  const time = createdAt === undefined ? null : chatTimestamp(createdAt);

  async function copyAnswer() {
    const request = ++copyRequest.current;
    let succeeded = true;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      succeeded = false;
    }
    if (request !== copyRequest.current) return;
    setCopiedUntil(succeeded ? performance.now() + 1_200 : 0);
    onCopyFeedback?.(succeeded ? "Copied" : "Copy failed");
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
      {page ? (
        <ForkPart
          part="AnswerMoreActions"
          page={page}
          onCopy={copyAnswer}
          forkable={forkable}
        />
      ) : null}
      {children}
      {time ? (
        <time
          className="chat-transcript-actions__time"
          dateTime={time.dateTime}
        >
          {time.label}
        </time>
      ) : null}
    </div>
  );
}
