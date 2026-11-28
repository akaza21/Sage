import { memo } from "react";
import { Archive } from "../../../ui/leanIcons";

import type { AcpItem, TurnState } from "../../../domain/acp";
import type { AcpTurnCommand } from "../../../domain/acpCommand";
import type {
  AgentArtifactOpenHandler,
  AgentArtifactOwner,
} from "../../../domain/agentArtifact";
import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";
import { CollaborationItem } from "./CollaborationItem";
import { ImageItem } from "./ImageItem";
import { PermissionItem } from "./PermissionItem";
import { ToolItem } from "./ToolItem";
import { UnknownItem } from "./UnknownItem";
import { UserPromptClamp } from "./UserPromptClamp";
import { terminalItemLifecycles } from "./statusSemantics";
import { PlanActivity, ThoughtRow } from "./WorkedRows";

const loadExtensionMessage = () => import("./ExtensionMessage");

const loadMarkdown = () =>
  import("./ChatMarkdown").then(({ ChatMarkdown }) => ({
    default: ChatMarkdown,
  }));

const commandSourceLabels: Partial<Record<AcpTurnCommand["source"], string>> = {
  builtin: "Built-in",
  skill: "Skill",
  prompt: "Prompt",
  extension: "Extension",
  mcp: "MCP",
  plugin: "Plugin",
};

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
    turnState,
    fileReferenceOwner,
    onOpenResource,
    selectableTurnId,
    command,
  }: {
    item: AcpItem;
    workingItems?: readonly AcpItem[];
    interactive: boolean;
    turnState?: TurnState;
    fileReferenceOwner?: AgentArtifactOwner;
    onOpenResource?: AgentArtifactOpenHandler;
    selectableTurnId?: string;
    command?: AcpTurnCommand;
  }) {
    switch (item.kind) {
      case "extensionMessage":
        return (
          <LazyModuleBoundary
            label="Extension message"
            load={loadExtensionMessage}
            render={(Message) => <Message item={item} />}
          />
        );
      case "userPrompt":
      case "text": {
        const role = item.kind === "text" ? item.payload.role : "user";
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
        const prose = (
          <LazyModuleBoundary
            label="Markdown renderer"
            load={loadMarkdown}
            render={(ChatMarkdown) => (
              <ChatMarkdown
                text={input ?? item.payload.text}
                streaming={
                  item.lifecycle === "started" || item.lifecycle === "streaming"
                }
                fileReferenceOwner={
                  role === "assistant" ? fileReferenceOwner : undefined
                }
              />
            )}
          />
        );
        return (
          <article
            className={`acp-message acp-message--${role}`}
            data-item-id={item.id}
            data-selectable-answer={answerTurn || undefined}
            tabIndex={answerTurn ? 0 : undefined}
            data-message-role={role}
            data-answer-state={partial ? "partial" : undefined}
          >
            {input !== null ? (
              <p className="acp-message__command">
                <span className="acp-message__command-name">
                  /{command?.name}
                </span>
                {sourceLabel ? (
                  <span className="acp-message__command-source">
                    {/* Read and copied as "/name, Source", not one word. */}
                    <span className="sr-only">, </span>
                    {sourceLabel}
                  </span>
                ) : null}
              </p>
            ) : null}
            {input === "" ? null : role === "user" ? (
              <UserPromptClamp>{prose}</UserPromptClamp>
            ) : (
              prose
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
            Edit included in the turn changed-file summary
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
    (left.item.kind !== "plan" || left.workingItems === right.workingItems) &&
    left.interactive === right.interactive &&
    left.turnState === right.turnState &&
    left.command === right.command &&
    left.fileReferenceOwner === right.fileReferenceOwner &&
    left.onOpenResource === right.onOpenResource,
);

function ErrorActivity({
  item,
}: {
  item: Extract<AcpItem, { kind: "error" }>;
}) {
  return (
    <article
      className="acp-activity acp-activity-card acp-error"
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
      if (terminalItemLifecycles.has(item.lifecycle))
        return item.payload.summary.trim() ? <ThoughtRow item={item} /> : null;
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
