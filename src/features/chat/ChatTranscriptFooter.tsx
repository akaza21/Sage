import { useEffect, useState, type ReactNode } from "react";
import { Copy, RotateCcw, Check } from "../../ui/leanIcons";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import { getPermissionStatus, terminalTurnStates } from "./acp/statusSemantics";
import { dayTimeLabel } from "./chatTimestamp";
import {
  formatWorkedDuration,
  formatWorkingDuration,
} from "./transcriptSummary";

function lastAssistantItem(
  page: AcpChatPage | null,
): Extract<AcpItem, { kind: "text" }> | null {
  const turn = page?.turns.at(-1);
  if (!turn || !terminalTurnStates.has(turn.state)) return null;
  for (let index = turn.items.length - 1; index >= 0; index--) {
    const item = turn.items[index];
    if (item.kind === "text" && item.payload.role === "assistant") return item;
  }
  return null;
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

  if (terminal && !hasActivities) return null;
  const groups = children ? (
    <div className="chat-transcript-work__groups">{children}</div>
  ) : null;

  if (!terminal) {
    return (
      <div className="chat-transcript-work chat-transcript-work--running">
        <div className="chat-transcript-work__heading">{duration}</div>
        {groups}
      </div>
    );
  }

  return (
    <details
      className="chat-transcript-work"
      open={turn.items.some(
        (item) =>
          item.kind === "permissionRequest" &&
          !getPermissionStatus(item).terminal,
      )}
    >
      <summary>{duration ?? "Worked"}</summary>
      {groups}
    </details>
  );
}

export function ChatTranscriptFooter({
  useNativeAcp,
  page,
  messages,
  onRetry,
}: {
  useNativeAcp: boolean;
  page: AcpChatPage | null;
  messages: readonly ChatMessage[];
  onRetry?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const CopyIcon = copied ? Check : Copy;
  const acpAssistant = lastAssistantItem(page);
  const legacyAssistant = page
    ? undefined
    : [...messages].reverse().find((message) => message.role === "assistant");
  const text = page
    ? (acpAssistant?.payload.text ?? "")
    : (legacyAssistant?.text ?? "");
  const timestamp = page ? acpAssistant?.createdAt : legacyAssistant?.createdAt;
  const hasText = !!text.trim();
  const latestTurn = page?.turns.at(-1);
  const retryAction = (
    page
      ? latestTurn && terminalTurnStates.has(latestTurn.state)
      : !useNativeAcp && hasText
  )
    ? onRetry
    : undefined;

  if (!hasText && !retryAction) return null;

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
      <div className="chat-transcript-actions" aria-label="Message actions">
        {hasText ? (
          <button type="button" onClick={() => void copyAnswer()}>
            <CopyIcon size={14} aria-hidden="true" />
            {copied ? "Copied" : "Copy"}
          </button>
        ) : null}
        {retryAction ? (
          <button type="button" onClick={retryAction}>
            <RotateCcw size={14} aria-hidden="true" />
            Retry
          </button>
        ) : null}
      </div>
    </>
  );
}
