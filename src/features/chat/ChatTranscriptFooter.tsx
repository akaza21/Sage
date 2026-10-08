import { useEffect, useState, type ReactNode } from "react";
import { Check, Copy, RotateCcw } from "lucide-react";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import { terminalTurnStates } from "./acp/statusSemantics";
import { dayTimeLabel } from "./chatTimestamp";
import {
  formatWorkedDuration,
  formatWorkingDuration,
} from "./transcriptSummary";

function lastAssistantItem(
  page: AcpChatPage | null,
): Extract<AcpItem, { kind: "text" }> | null {
  if (!page) return null;
  const items = page.turns.flatMap((turn) => turn.items);
  return (
    [...items]
      .reverse()
      .find(
        (item): item is Extract<AcpItem, { kind: "text" }> =>
          item.kind === "text" && item.payload.role === "assistant",
      ) ?? null
  );
}

export function ChatTranscriptTurnSummary({
  turn,
  hasActivities,
  children,
}: {
  turn: TurnProjection;
  hasActivities: boolean;
  children?: ReactNode;
}) {
  const terminal = terminalTurnStates.has(turn.state);
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (terminal) return;
    const timer = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [terminal, turn.id]);
  const duration = terminal
    ? formatWorkedDuration(turn.createdAt, turn.terminalAt)
    : (formatWorkingDuration(turn.createdAt, nowMs) ?? "Working");

  if (!hasActivities && !duration) return null;

  if (!terminal) {
    return (
      <div className="chat-transcript-work chat-transcript-work--running">
        <div className="chat-transcript-work__heading">{duration}</div>
        {children ? (
          <div className="chat-transcript-work__groups">{children}</div>
        ) : null}
      </div>
    );
  }

  return (
    <details className="chat-transcript-work">
      <summary>{duration ?? "Worked"}</summary>
      {children ? (
        <div className="chat-transcript-work__groups">{children}</div>
      ) : null}
    </details>
  );
}

export function ChatTranscriptFooter({
  page,
  messages,
  onRetry,
}: {
  page: AcpChatPage | null;
  messages: readonly ChatMessage[];
  onRetry?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const acpAssistant = lastAssistantItem(page);
  const legacyAssistant = [...messages]
    .reverse()
    .find((message) => message.role === "assistant");
  const text = acpAssistant?.payload.text ?? legacyAssistant?.text ?? "";
  const timestamp = acpAssistant?.createdAt ?? legacyAssistant?.createdAt;

  if (!text) return null;

  async function copyAnswer() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_200);
    } catch {
      setCopied(false);
    }
  }

  return (
    <>
      {!page && typeof timestamp === "number" && Number.isFinite(timestamp) ? (
        <div className="chat-transcript-day" role="separator">
          <time dateTime={new Date(timestamp).toISOString()}>
            {dayTimeLabel(timestamp)}
          </time>
        </div>
      ) : null}
      {text ? (
        <div className="chat-transcript-actions" aria-label="Message actions">
          <button type="button" onClick={() => void copyAnswer()}>
            {copied ? (
              <Check size={14} aria-hidden="true" />
            ) : (
              <Copy size={14} aria-hidden="true" />
            )}
            {copied ? "Copied" : "Copy"}
          </button>
          {onRetry ? (
            <button type="button" onClick={onRetry}>
              <RotateCcw size={14} aria-hidden="true" />
              Retry
            </button>
          ) : null}
        </div>
      ) : null}
    </>
  );
}
