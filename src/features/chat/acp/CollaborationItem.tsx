import { useId } from "react";

import type { AcpItem } from "../../../domain/acp";

type CollaborationProjectionItem = Extract<AcpItem, { kind: "collaboration" }>;

const collaborationStateLabels = {
  started: "Observed",
  streaming: "Active",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  unsupported: "Unsupported",
} as const;

export function CollaborationItem({
  item,
}: {
  item: CollaborationProjectionItem;
}) {
  const descriptionId = useId();
  const status = collaborationStateLabels[item.lifecycle];
  const { operation, agentStates = [], agentKeys = [] } = item.payload;
  const finished = agentStates.filter(([, state]) => state === "completed");
  const working = agentStates.filter(
    ([, state]) => state === "running" || state === "pendingInit",
  );
  const failed = agentStates.filter(
    ([, state]) => state === "errored" || state === "notFound",
  );
  const stopped = agentStates.filter(
    ([, state]) => state === "shutdown" || state === "interrupted",
  );
  const isStarted =
    (operation === "started" && Boolean(item.payload.agentKey)) ||
    (operation === "spawnAgent" && working.length > 0);
  const isFinished = finished.length > 0;
  const isInterruptedActivity =
    operation === "interrupted" && Boolean(item.payload.agentKey);
  if (
    isStarted ||
    isFinished ||
    failed.length > 0 ||
    stopped.length > 0 ||
    isInterruptedActivity
  ) {
    const count = isFinished
      ? finished.length
      : failed.length > 0
        ? failed.length
        : stopped.length > 0 || isInterruptedActivity
          ? stopped.length || 1
          : operation === "started"
            ? 1
            : working.length;
    const agentLabel =
      operation === "started" ? item.payload.label : "Subagent";
    const names =
      isFinished && item.payload.receiverLabels.length === agentKeys.length
        ? finished.map(
            ([key]) => item.payload.receiverLabels[agentKeys.indexOf(key)],
          )
        : [];
    const named = names.length === count && names.every(Boolean);
    const namedList =
      count === 1
        ? names[0]
        : `${names.slice(0, -1).join(", ")} and ${names[count - 1]}`;
    const text = isFinished
      ? named
        ? `${namedList} finished`
        : count === 1
          ? "Subagent finished"
          : `${count} subagents finished`
      : failed.length > 0
        ? count === 1
          ? "Subagent failed to start"
          : `${count} subagents failed to start`
        : stopped.length > 0 || isInterruptedActivity
          ? count === 1
            ? "Subagent stopped"
            : `${count} subagents stopped`
          : count === 1
            ? `${agentLabel} started working`
            : `${count} subagents started working`;
    return (
      <p className="acp-subagent-row" role="status" aria-label={text}>
        <span className="acp-subagent-row__glyphs" aria-hidden="true">
          {Array.from({ length: Math.min(count, 3) }, (_, index) => (
            <span
              key={index}
              className={`acp-subagent-row__glyph acp-subagent-row__glyph--${index}`}
            >
              {isFinished ? "●" : "✦"}
            </span>
          ))}
        </span>
        {text}
      </p>
    );
  }
  return (
    <article
      className="acp-collaboration"
      role="group"
      aria-label={`Harness collaboration: ${item.payload.label}`}
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        Read-only activity owned and scheduled by the harness
      </span>
      <header>
        <span className="acp-collaboration__identity">
          <span className="acp-collaboration__kind">Collaboration</span>
          <strong
            className="acp-collaboration__label"
            title={item.payload.label}
          >
            {item.payload.label}
          </strong>
        </span>
        <span
          className="acp-collaboration__status"
          role="status"
          aria-label={status}
          aria-live="off"
          data-lifecycle={item.lifecycle}
        >
          {status}
        </span>
      </header>
      <p>{item.payload.summary}</p>
      {item.payload.receiverLabels.length > 0 ? (
        <p className="acp-collaboration__receivers">
          Participants: {item.payload.receiverLabels.join(", ")}
        </p>
      ) : null}
    </article>
  );
}
