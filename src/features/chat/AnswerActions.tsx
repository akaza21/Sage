import {
  useContext,
  useEffect,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
} from "react";
import { Check, Copy } from "../../ui/leanIcons";

import type { AcpChatPage } from "../../domain/acp";
import { AnswerCopyFeedback } from "./answerCopyFeedback";
import { chatTimestamp } from "./chatTimestamp";
import { NoticePart } from "./lazyParts";
import "./answer-actions.css";

export function AnswerActionButton({
  label,
  ...props
}: { label: string } & Omit<
  ComponentProps<"button">,
  "aria-label" | "className" | "title" | "type"
>) {
  return (
    <button
      {...props}
      type="button"
      className="chat-transcript-actions__button"
      aria-label={label}
      data-tooltip={label}
    />
  );
}

export function AnswerActions({
  text,
  createdAt,
  page,
  forkable = false,
  prompt = false,
  children,
}: {
  text: string;
  createdAt?: number;
  page?: AcpChatPage | null;
  forkable?: boolean;
  /** A user prompt's hover row: time first, then copy and its menu. */
  prompt?: boolean;
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

  const timeLabel = time ? (
    <time className="chat-transcript-actions__time" dateTime={time.dateTime}>
      {time.label}
    </time>
  ) : null;
  return (
    <div
      className={`chat-transcript-actions${prompt ? " acp-prompt-actions" : ""}`}
      role="group"
      aria-label={prompt ? "Prompt actions" : "Message actions"}
    >
      {prompt ? timeLabel : null}
      <AnswerActionButton
        label={copied ? "Copied" : prompt ? "Copy prompt" : "Copy"}
        onClick={() => void copyAnswer()}
      >
        <CopyIcon size={14} aria-hidden="true" />
      </AnswerActionButton>
      {page ? (
        <NoticePart
          part="MessageActionsTrigger"
          page={page}
          onCopy={copyAnswer}
          forkable={forkable}
        />
      ) : null}
      {children}
      {prompt ? null : timeLabel}
    </div>
  );
}
