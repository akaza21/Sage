import { useId } from "react";
import { type AcpItem } from "../../../domain/acp";
import { type ResourceRef } from "../../../domain/workbench";
import type { AgentArtifactOpenIntent } from "../../../domain/agentArtifact";
import {
  ArrowRightLeft,
  Bot,
  Brain,
  Command,
  FilePen,
  FileSymlink,
  FileText,
  Globe,
  Image,
  Search,
  TerminalSquare,
  Trash2,
  Wrench,
} from "../../../ui/leanIcons";
import { extractAdmittedBrowseUrl } from "../../preview/harnessBrowse";
import { CompactToolItem } from "./CompactToolItem";
import { terminalToolStates, toolStateLabels } from "./statusSemantics";
import { toolDurationLabel } from "./toolActivityRollup";
import { compactToolLabels, toolCommandContent } from "./toolCommandContent";
import { ToolCommandFrame } from "./ToolCommandFrame";
import { ToolItemDetails } from "./ToolItemDetails";
import { ToolRunningIndicator } from "./ToolRunningIndicator";

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

export type OpenResourceTarget = AgentArtifactOpenIntent["target"];

type ToolKind = ToolProjectionItem["payload"]["presentation"]["kind"];

const toolKindIcons: Record<ToolKind, typeof FileText> = {
  read: FileText,
  edit: FilePen,
  delete: Trash2,
  move: FileSymlink,
  search: Search,
  execute: TerminalSquare,
  fetch: Globe,
  webSearch: Command,
  imageView: Image,
  think: Brain,
  switchMode: ArrowRightLeft,
  mcp: Command,
  subagent: Bot,
  other: Wrench,
};

export function ToolKindIcon({ kind }: { kind: ToolKind }) {
  const Icon = toolKindIcons[kind];
  return <Icon aria-hidden="true" size={14} data-tool-kind-icon={kind} />;
}

export interface ToolItemProps {
  item: ToolProjectionItem;
  compact?: boolean;
  allowBrowse?: boolean;
  onOpenResource?: (resource: ResourceRef, target: OpenResourceTarget) => void;
}

export function ToolItem({
  item,
  compact = false,
  allowBrowse = true,
  onOpenResource,
}: ToolItemProps) {
  const descriptionId = useId();
  const { commandLine, output: outputSummary } = toolCommandContent(item);
  const inputSummary = item.payload.inputSummary;
  const kind = item.payload.presentation.kind;
  const detail =
    kind === "mcp" || kind === "webSearch"
      ? null
      : kind === "execute"
        ? outputSummary
        : (outputSummary ?? inputSummary);
  const visualState = item.toolState;
  const presentation = item.payload.presentation;
  const label =
    presentation.kind === "mcp"
      ? presentation.serverLabel === "MCP"
        ? presentation.toolLabel
        : `${presentation.serverLabel} · ${presentation.toolLabel}`
      : presentation.label;
  const progress =
    presentation.kind === "mcp" && visualState === "inProgress"
      ? (presentation.progress ?? null)
      : null;
  const status = toolStateLabels[visualState];
  const terminal = terminalToolStates.has(visualState);
  const toolOutput =
    kind === "execute" || kind === "fetch" || detail === null
      ? null
      : outputSummary;
  const inlineOutput = kind === "fetch" ? false : outputSummary !== null;
  const duration = terminal
    ? toolDurationLabel(item.createdAt, item.terminalAt ?? null)
    : null;
  const browse =
    presentation.kind === "fetch"
      ? (extractAdmittedBrowseUrl(presentation.url ?? "") ??
        extractAdmittedBrowseUrl(presentation.summary ?? "") ??
        extractAdmittedBrowseUrl(inputSummary ?? "") ??
        extractAdmittedBrowseUrl(outputSummary ?? ""))
      : null;
  if (compact && compactToolLabels[kind] !== undefined)
    return <CompactToolItem item={item} onOpenResource={onOpenResource} />;
  return (
    <article
      className={`acp-activity-card acp-activity-card--tool acp-tool-card acp-tool-card--${visualState}`}
      role="group"
      aria-label={label}
      aria-describedby={detail ? descriptionId : undefined}
      data-activity-kind={kind}
      data-activity-state={visualState}
      data-acp-item-id={item.id}
    >
      {detail && !inlineOutput ? (
        <span className="sr-only" id={descriptionId}>
          {detail}
        </span>
      ) : null}
      <header className="acp-activity-card__header acp-tool-card__header">
        {visualState === "inProgress" ? (
          <ToolRunningIndicator />
        ) : (
          <ToolKindIcon kind={kind} />
        )}
        <strong>{label}</strong>
        {duration ? (
          <span className="acp-tool-card__duration">{duration}</span>
        ) : null}
        {visualState !== "inProgress" ? (
          <span
            className="acp-state"
            role="status"
            aria-label={terminal ? `${label}: ${status}` : status}
            aria-live="off"
            aria-atomic="true"
          >
            {status}
          </span>
        ) : null}
      </header>
      {progress ? <p className="acp-tool-card__progress">{progress}</p> : null}
      {kind === "execute" && (commandLine || outputSummary !== null) ? (
        <ToolCommandFrame
          label="Shell"
          commandLine={commandLine ? `$ ${commandLine}` : null}
          output={outputSummary}
          descriptionId={descriptionId}
        />
      ) : null}
      <ToolItemDetails
        item={item}
        browse={browse}
        allowBrowse={allowBrowse}
        onOpenResource={onOpenResource}
      />
      {toolOutput !== null ? (
        <details className="acp-tool-command-frame acp-tool-output" open>
          <summary className="acp-tool-command__disclosure">
            Tool output
          </summary>
          <pre
            id={descriptionId}
            className="acp-tool-command acp-tool-command--output"
            aria-label="Tool output"
          >
            {toolOutput}
          </pre>
        </details>
      ) : null}
      {detail && !inlineOutput ? (
        <details className="acp-details">
          <summary>Tool details</summary>
          <p>{detail}</p>
        </details>
      ) : null}
    </article>
  );
}
