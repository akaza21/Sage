import { useEffect, useId, useState, type ReactNode } from "react";
import { ChevronRight } from "../../ui/leanIcons";

import type { AcpItem, TurnProjection } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import { TurnDaySeparator } from "./acp/AcpTurnDates";
import { terminalTurnStates, turnStateLabels } from "./acp/statusSemantics";
import { WorkedTurnTerminal } from "./acp/WorkedTurnTerminal";
import { AnswerActions } from "./AnswerActions";
import "./chat-transcript-work.css";
import { formatDuration } from "./transcriptSummary";

const isAssistantText = (
  item: AcpItem,
): item is Extract<AcpItem, { kind: "text" }> =>
  item.kind === "text" &&
  item.payload.role === "assistant" &&
  !!item.payload.text.trim();

export function ChatTranscriptTurnSummary({
  turn,
  hasActivities,
  children,
  steered,
  runningHeading = true,
  aside,
}: {
  turn: TurnProjection;
  hasActivities: boolean;
  children?: ReactNode;
  steered?: readonly ReactNode[];
  runningHeading?: boolean;
  /** Sits beside the settled heading, such as the folded plan chip. */
  aside?: ReactNode;
}) {
  const terminal = terminalTurnStates.has(turn.state);
  const [expanded, setExpanded] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const groupsId = useId();
  useEffect(() => {
    if (terminal) return;
    const timer = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [terminal, turn.id]);
  const verb = terminal ? "Worked" : "Working";
  const duration =
    turn.state === "waitingApproval" || turn.state === "waitingUser"
      ? turnStateLabels[turn.state]
      : (formatDuration(
          verb,
          turn.createdAt,
          terminal ? turn.terminalAt : nowMs,
        ) ?? verb);

  if (terminal && !hasActivities) {
    if (!turn.items.some(isAssistantText)) return null;
    return (
      <div className="chat-transcript-work">
        <div className="chat-transcript-work__heading">{duration}</div>
        {aside}
      </div>
    );
  }
  const open = !terminal || expanded;
  const groups =
    (open && children) || steered?.length ? (
      <WorkedTurnTerminal value={terminal}>
        <div
          id={groupsId}
          className={`chat-transcript-work__groups${terminal && expanded ? " stagger-reveal" : ""}`}
        >
          {open ? children : steered}
        </div>
      </WorkedTurnTerminal>
    ) : null;

  if (!terminal) {
    const thinking = hasActivities ? null : (
      <p className="acp-thought acp-live-shimmer" data-activity-kind="thought">
        Thinking
      </p>
    );
    if (!runningHeading && thinking) return thinking;
    return (
      <div className="chat-transcript-work chat-transcript-work--running">
        <div className="chat-transcript-work__heading">{duration}</div>
        {thinking ?? groups}
      </div>
    );
  }

  return (
    <>
      <div className="chat-transcript-work">
        <button
          type="button"
          className="chat-transcript-work__heading chat-transcript-work__toggle"
          aria-expanded={expanded}
          aria-controls={expanded ? groupsId : undefined}
          onClick={() => setExpanded(!expanded)}
        >
          {duration}
          <ChevronRight
            size={12}
            className="chat-transcript-work__chevron"
            aria-hidden="true"
          />
        </button>
        {aside}
      </div>
      {groups}
    </>
  );
}

export function ChatTranscriptFooter({
  messages,
}: {
  messages: readonly ChatMessage[];
}) {
  const assistant = [...messages]
    .reverse()
    .find((message) => message.role === "assistant");
  if (!assistant?.text.trim()) return null;
  return (
    <>
      <TurnDaySeparator
        timestamp={
          Number.isFinite(assistant.createdAt) ? assistant.createdAt : null
        }
      />
      <AnswerActions text={assistant.text} />
    </>
  );
}
