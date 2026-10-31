import { useEffect, useState, type ReactNode } from "react";
import { ChevronRight, RotateCcw } from "../../ui/leanIcons";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import { TurnDaySeparator } from "./acp/AcpTurnDates";
import { terminalTurnStates } from "./acp/statusSemantics";
import {
  WorkedActivityVisibility,
  WorkedTurnTerminal,
} from "./acp/WorkedActivityVisibility";
import { AnswerActions } from "./AnswerActions";
import {
  formatWorkedDuration,
  formatWorkingDuration,
} from "./transcriptSummary";

const isAssistantText = (
  item: AcpItem,
): item is Extract<AcpItem, { kind: "text" }> =>
  item.kind === "text" &&
  item.payload.role === "assistant" &&
  !!item.payload.text.trim();

function lastAssistantItem(
  page: AcpChatPage | null,
): Extract<AcpItem, { kind: "text" }> | null {
  const turn = page?.turns.at(-1);
  if (!turn || !terminalTurnStates.has(turn.state)) return null;
  for (let index = turn.items.length; index--;) {
    const item = turn.items[index];
    if (isAssistantText(item)) return item;
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

  thinkingPlaceholder?: boolean;
}) {
  const terminal = terminalTurnStates.has(turn.state);
  const [expanded, setExpanded] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (terminal) return;
    const timer = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [terminal, turn.id]);
  const duration = terminal
    ? (formatWorkedDuration(turn.createdAt, turn.terminalAt) ?? "Worked")
    : (formatWorkingDuration(turn.createdAt, nowMs) ?? "Working");

  if (terminal && !hasActivities) {
    if (!turn.items.some(isAssistantText)) return null;
    return (
      <div className="chat-transcript-work">
        <div className="chat-transcript-work__heading">{duration}</div>
      </div>
    );
  }
  // Collapsed Worked shows no rows at all.
  const groups =
    children && (!terminal || expanded) ? (
      <WorkedTurnTerminal value={terminal}>
        <WorkedActivityVisibility value={!terminal || expanded}>
          <div className="chat-transcript-work__groups">{children}</div>
        </WorkedActivityVisibility>
      </WorkedTurnTerminal>
    ) : null;

  if (!terminal && !hasActivities)
    return (
      <p className="acp-thought acp-live-shimmer" role="status">
        {turn.state === "sending" ? "Starting agent…" : "Waiting for response…"}
      </p>
    );
  if (!terminal) {
    return (
      <div className="chat-transcript-work chat-transcript-work--running">
        <div className="chat-transcript-work__heading">{duration}</div>
        {groups}
      </div>
    );
  }

  return (
    <>
      <details
        className="chat-transcript-work"
        data-worked={turn.id}
        open={expanded}
        onToggle={(event) => {
          if (event.currentTarget.open !== expanded)
            setExpanded(event.currentTarget.open);
        }}
      >
        <summary
          onClick={(event) => {
            event.preventDefault();
            setExpanded(!expanded);
          }}
        >
          {duration}
          <ChevronRight
            size={12}
            className="chat-transcript-work__chevron"
            aria-hidden="true"
          />
        </summary>
      </details>
      {groups}
    </>
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
  const acpAssistant = lastAssistantItem(page);
  const legacyAssistant = page
    ? undefined
    : [...messages].reverse().find((message) => message.role === "assistant");
  const text = page
    ? (acpAssistant?.payload.text ?? "")
    : (legacyAssistant?.text ?? "");
  const timestamp = page ? acpAssistant?.createdAt : legacyAssistant?.createdAt;
  if (!text.trim()) return null;
  // A native answer here is the latest Turn's, which is terminal
  // (lastAssistantItem); a legacy one retries only off native ACP.
  const stopped =
    page && ["cancelled", "interrupted"].includes(page.turns.at(-1)!.state);
  const retryAction = stopped || !useNativeAcp ? onRetry : undefined;

  return (
    <>
      <TurnDaySeparator
        timestamp={
          !page && typeof timestamp === "number" && Number.isFinite(timestamp)
            ? timestamp
            : null
        }
      />
      {/* Text is guaranteed here: the footer returns early without it. */}
      <AnswerActions
        text={text}
        createdAt={page ? timestamp : undefined}
        page={page}
        forkable
      >
        {retryAction ? (
          <button
            type="button"
            aria-label="Retry"
            title="Retry"
            onClick={retryAction}
          >
            <RotateCcw size={14} aria-hidden="true" />
          </button>
        ) : null}
      </AnswerActions>
    </>
  );
}
