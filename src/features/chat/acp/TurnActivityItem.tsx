import { memo, useState } from "react";
import {
  Archive,
  Check,
  Circle,
  LoaderCircle,
  XCircle,
} from "../../../ui/leanIcons";

import type { AcpItem, TurnState } from "../../../domain/acp";
import type { AcpTurnCommand } from "../../../domain/acpCommand";
import type {
  AgentArtifactOpenHandler,
  AgentArtifactOwner,
} from "../../../domain/agentArtifact";
import { chatTimestamp } from "../chatTimestamp";
import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";
import { CollaborationItem } from "./CollaborationItem";
import { ImageItem } from "./ImageItem";
import { PermissionItem } from "./PermissionItem";
import { ToolItem } from "./ToolItem";
import { UnknownItem } from "./UnknownItem";
import { terminalItemLifecycles } from "./statusSemantics";
import { isWorkingTool, workingToolStatus } from "./workingTool";

const loadMarkdown = () =>
  import("./ChatMarkdown").then(({ ChatMarkdown }) => ({
    default: ChatMarkdown,
  }));

/** What a command chip names beside the command, by source. */
const commandSourceLabels: Partial<Record<AcpTurnCommand["source"], string>> = {
  builtin: "Built-in",
  skill: "Skill",
  prompt: "Prompt",
  extension: "Extension",
  mcp: "MCP",
  plugin: "Plugin",
};

/** What a prompt that ran `/name` typed after the name, or null. */
function commandInput(text: string, name?: string): string | null {
  const rest =
    name && text.startsWith(`/${name}`) ? text.slice(name.length + 1) : null;
  return rest !== null && (rest === "" || /^\s/u.test(rest))
    ? rest.trim()
    : null;
}

type TurnActivityProjectionItem = Extract<
  AcpItem,
  { kind: "thoughtSummary" | "plan" | "compactionNotice" | "error" }
>;

export const ItemView = memo(
  function ItemView({
    item,
    workingItems,
    interactive,
    neutralTextRole,
    turnState,
    fileReferenceOwner,
    onOpenResource,
    selectableTurnId,
    command,
  }: {
    item: AcpItem;
    workingItems?: readonly AcpItem[];
    interactive: boolean;
    neutralTextRole?: boolean;
    turnState?: TurnState;
    fileReferenceOwner?: AgentArtifactOwner;
    onOpenResource?: AgentArtifactOpenHandler;
    selectableTurnId?: string;
    /** The listed command a user prompt ran, shown as a chip. */
    command?: AcpTurnCommand;
  }) {
    switch (item.kind) {
      // A steered message is the user's own bubble, at its place in the Turn.
      case "userPrompt":
      case "text": {
        const role = item.kind === "text" ? item.payload.role : "user";
        const timestamp = chatTimestamp(item.createdAt);
        // Final assistant prose is quotable (and focusable for the menu key).
        const answerTurn = role === "assistant" && selectableTurnId;
        const input =
          role === "user"
            ? commandInput(item.payload.text, command?.name)
            : null;
        const sourceLabel = command && commandSourceLabels[command.source];
        const partial =
          role === "assistant" &&
          (turnState === "interrupted" || turnState === "cancelled") &&
          item.lifecycle !== "completed" &&
          item.payload.text.trim().length > 0;
        return (
          <article
            className={`acp-message acp-message--${role}`}
            data-item-id={item.id}
            data-selectable-answer={answerTurn || undefined}
            tabIndex={answerTurn ? 0 : undefined}
            data-message-role={role}
            data-answer-state={partial ? "partial" : undefined}
          >
            <header className="acp-message__meta">
              {role === "assistant" ? (
                <span className="acp-message__avatar" aria-hidden="true">
                  S
                </span>
              ) : null}
              <span className="acp-message__role">
                {neutralTextRole
                  ? "Unattributed"
                  : role === "assistant"
                    ? "Agent"
                    : role === "user"
                      ? "You"
                      : "System"}
              </span>
              {timestamp ? (
                <time dateTime={timestamp.dateTime} title={timestamp.dateTime}>
                  {timestamp.label}
                </time>
              ) : null}
            </header>
            {input !== null ? (
              <p className="acp-message__command">
                <span className="acp-message__command-name">
                  /{command?.name}
                </span>
                {sourceLabel ? (
                  <span className="acp-message__command-source">
                    {sourceLabel}
                  </span>
                ) : null}
              </p>
            ) : null}
            {input === "" ? null : (
              <LazyModuleBoundary
                label="Markdown renderer"
                load={loadMarkdown}
                render={(ChatMarkdown) => (
                  <ChatMarkdown
                    text={input ?? item.payload.text}
                    streaming={
                      item.lifecycle === "started" ||
                      item.lifecycle === "streaming"
                    }
                    fileReferenceOwner={
                      role === "assistant" ? fileReferenceOwner : undefined
                    }
                  />
                )}
              />
            )}
          </article>
        );
      }
      case "thoughtSummary":
      case "error":
        return <TurnActivityItem item={item} />;
      case "plan":
      case "compactionNotice":
        return <TurnActivityItem item={item} workingItems={workingItems} />;
      case "toolCall":
        return (
          <ToolItem
            item={item}
            allowBrowse={interactive}
            onOpenResource={interactive ? onOpenResource : undefined}
          />
        );
      case "fileChangeDiff":
        return interactive ? (
          <p className="acp-activity__edit-reference">
            Edit included in the Turn changed-file summary
          </p>
        ) : (
          <article
            className="acp-activity__edit-reference"
            data-item-id={item.id}
          >
            {item.payload.changes.map((change, index) => (
              <p key={index}>{change.summary}</p>
            ))}
          </article>
        );
      case "image":
        return <ImageItem item={item} />;
      case "permissionRequest":
        return <PermissionItem item={item} />;
      case "unsupported":
        return <UnknownItem item={item} />;
      case "collaboration":
        return <CollaborationItem item={item} />;
    }
  },
  (left, right) =>
    left.item.id === right.item.id &&
    left.item.revision === right.item.revision &&
    (left.item.kind !== "plan"
      ? true
      : left.workingItems === right.workingItems) &&
    left.interactive === right.interactive &&
    left.neutralTextRole === right.neutralTextRole &&
    left.turnState === right.turnState &&
    left.command === right.command &&
    left.fileReferenceOwner === right.fileReferenceOwner &&
    left.onOpenResource === right.onOpenResource,
);

const MAX_PLAN_WORKING_ROWS = 3;
const planStateLabels = {
  pending: "Pending",
  inProgress: "In progress",
  completed: "Completed",
  failed: "Failed",
} as const;

function PlanStateIcon({ state }: { state: keyof typeof planStateLabels }) {
  const Icon =
    state === "completed"
      ? Check
      : state === "inProgress"
        ? LoaderCircle
        : state === "failed"
          ? XCircle
          : Circle;
  return (
    <Icon
      className={state === "inProgress" ? "acp-plan__working" : undefined}
      size={14}
      aria-hidden="true"
    />
  );
}

function PlanActivity({
  item,
  workingItems,
}: {
  item: Extract<AcpItem, { kind: "plan" }>;
  workingItems: readonly AcpItem[];
}) {
  const [expanded, setExpanded] = useState(true);
  const complete = item.payload.entries.filter(
    (entry) => entry.state === "completed",
  ).length;
  const activityState = item.payload.entries.some(
    (entry) => entry.state === "failed",
  )
    ? "failed"
    : complete === item.payload.entries.length
      ? "completed"
      : "inProgress";
  const inProgressIndex = item.payload.entries.findIndex(
    (entry) => entry.state === "inProgress",
  );
  const working = workingItems.filter(isWorkingTool);
  const visibleWorking = working.slice(0, MAX_PLAN_WORKING_ROWS);
  const omittedWorkingCount = working.length - visibleWorking.length;
  return (
    <section
      className="acp-activity-card acp-activity-card--plan acp-plan"
      aria-label="Agent plan"
      data-activity-kind="plan"
      data-activity-state={activityState}
    >
      <button
        className="acp-plan__summary"
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((current) => !current)}
      >
        <strong>Plan</strong>
        <span className="acp-state" role="status" aria-live="off">
          {expanded
            ? `Hide · ${complete} of ${item.payload.entries.length} complete`
            : `${complete} of ${item.payload.entries.length} complete`}
        </span>
      </button>
      {expanded ? (
        <ol>
          {item.payload.entries.map((entry, index) => (
            <li key={`${index}:${entry.label}`} data-plan-state={entry.state}>
              <PlanStateIcon state={entry.state} />
              <span>{entry.label}</span>
              <span className="acp-plan__state">
                {planStateLabels[entry.state]}
              </span>
              {index === inProgressIndex && visibleWorking.length > 0 ? (
                <ul
                  className="acp-plan__activity"
                  aria-label="Current activity"
                >
                  {visibleWorking.map((workingItem) => (
                    <li key={workingItem.id}>
                      <span className="acp-plan__pulse" aria-hidden="true" />
                      <span>
                        <span>{workingItem.payload.presentation.label}</span>
                        <span>{workingToolStatus(workingItem)}</span>
                      </span>
                    </li>
                  ))}
                  {omittedWorkingCount > 0 ? (
                    <li className="acp-plan__working-more">
                      {omittedWorkingCount} more activity item
                      {omittedWorkingCount === 1 ? "" : "s"} running
                    </li>
                  ) : null}
                </ul>
              ) : null}
            </li>
          ))}
        </ol>
      ) : null}
    </section>
  );
}

function ErrorActivity({
  item,
}: {
  item: Extract<AcpItem, { kind: "error" }>;
}) {
  return (
    <article
      className="acp-activity-card acp-activity-card--error acp-error"
      role="alert"
      aria-live="off"
      data-activity-kind="error"
      data-activity-state="failed"
    >
      <header>
        <strong>
          {item.payload.kind === "attempt"
            ? item.payload.code
            : item.payload.code === "browserMcpUnavailable"
              ? "Browser tools unavailable"
              : item.payload.code === "clientFileWriteRefused"
                ? "File write unavailable"
                : "Agent notice"}
        </strong>
        <span className="acp-state">
          {item.payload.kind === "attempt"
            ? "Automatic retry"
            : item.payload.recoverable
              ? "Retry available"
              : "Needs attention"}
        </span>
      </header>
      <p>{item.payload.message}</p>
    </article>
  );
}

export function TurnActivityItem({
  item,
  workingItems = [],
}: {
  item: TurnActivityProjectionItem;
  workingItems?: readonly AcpItem[];
}) {
  switch (item.kind) {
    case "thoughtSummary":
      if (terminalItemLifecycles.has(item.lifecycle)) return null;
      return (
        <p
          className="acp-thought acp-live-shimmer"
          data-activity-kind="thought"
        >
          Thinking
        </p>
      );
    case "plan":
      return <PlanActivity item={item} workingItems={workingItems} />;
    case "compactionNotice":
      if (item.lifecycle === "unsupported") return null;
      return (
        <p
          className="acp-compaction-notice acp-activity-row"
          data-activity-kind="compactionNotice"
          data-activity-state={item.lifecycle}
          role="status"
        >
          <Archive size={14} aria-hidden="true" />
          <span>
            {item.lifecycle === "completed"
              ? "Context automatically compacted"
              : item.lifecycle === "started" || item.lifecycle === "streaming"
                ? "Compacting context…"
                : item.lifecycle === "failed"
                  ? "Context compaction failed"
                  : "Context compaction cancelled"}
          </span>
        </p>
      );
    case "error":
      return <ErrorActivity item={item} />;
  }
}
