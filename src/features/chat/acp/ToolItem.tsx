import { useId } from "react";
import {
  ArrowRightLeft,
  Bot,
  Brain,
  FilePen,
  FileSymlink,
  FileText,
  Globe,
  Plug,
  Search,
  Terminal,
  Trash2,
  Wrench,
} from "../../../ui/leanIcons";

import type { AcpItem } from "../../../domain/acp";
import type { ResourceRef } from "../../../domain/workbench";
import { ToolItemDetails } from "./ToolItemDetails";
import { extractAdmittedBrowseUrl } from "../../preview/harnessBrowse";
import { terminalToolStates, toolStateLabels } from "./statusSemantics";
import { toolDurationLabel } from "./toolActivityRollup";
import { ToolRunningIndicator } from "./ToolRunningIndicator";

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

export type OpenResourceTarget = "file" | "diff";

type ToolKind = ToolProjectionItem["payload"]["presentation"]["kind"];

const toolKindIcons: Record<ToolKind, typeof FileText> = {
  read: FileText,
  edit: FilePen,
  delete: Trash2,
  move: FileSymlink,
  search: Search,
  execute: Terminal,
  fetch: Globe,
  think: Brain,
  switchMode: ArrowRightLeft,
  mcp: Plug,
  subagent: Bot,
  other: Wrench,
};

function ToolKindIcon({ kind }: { kind: ToolKind }) {
  const Icon = toolKindIcons[kind];
  return <Icon aria-hidden="true" size={14} data-tool-kind-icon={kind} />;
}

export interface ToolItemProps {
  item: ToolProjectionItem;
  allowBrowse?: boolean;
  onOpenResource?: (resource: ResourceRef, target: OpenResourceTarget) => void;
}

export function ToolItem({
  item,
  allowBrowse = true,
  onOpenResource,
}: ToolItemProps) {
  const descriptionId = useId();
  const outputSummary = item.payload.outputSummary ?? null;
  const inputSummary = item.payload.inputSummary;
  const kind = item.payload.presentation.kind;
  const detail =
    kind === "mcp"
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
  const status = toolStateLabels[visualState];
  const terminal = terminalToolStates.has(visualState);
  const toolOutput =
    kind === "execute" || kind === "fetch" ? null : outputSummary;
  const inlineOutput = kind === "fetch" ? false : outputSummary !== null;
  const duration = terminal
    ? toolDurationLabel(item.createdAt, item.terminalAt ?? null)
    : null;
  const commandLine =
    presentation.kind === "execute"
      ? inputSummary?.trim()
        ? inputSummary
        : presentation.summary?.trim() || null
      : null;
  const browse =
    presentation.kind === "fetch"
      ? (extractAdmittedBrowseUrl(presentation.url ?? "") ??
        extractAdmittedBrowseUrl(presentation.summary ?? "") ??
        extractAdmittedBrowseUrl(inputSummary ?? "") ??
        extractAdmittedBrowseUrl(outputSummary ?? ""))
      : null;
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
        <ToolKindIcon kind={kind} />
        <strong>{label}</strong>
        {duration ? (
          <span className="acp-tool-card__duration">{duration}</span>
        ) : null}
        {visualState === "inProgress" ? (
          <ToolRunningIndicator />
        ) : (
          <span
            className="acp-state"
            role="status"
            aria-label={terminal ? `${label}: ${status}` : status}
            aria-live="off"
            aria-atomic="true"
          >
            {status}
          </span>
        )}
      </header>
      {kind === "execute" && (commandLine || outputSummary !== null) ? (
        <div className="acp-tool-command-frame">
          <p className="acp-tool-command__disclosure" role="note">
            Shell
          </p>
          {commandLine ? (
            <pre
              className="acp-tool-command acp-tool-command--line"
              aria-label="Command line"
              tabIndex={0}
            >
              {`$ ${commandLine}`}
            </pre>
          ) : null}
          {outputSummary !== null ? (
            <pre
              id={descriptionId}
              className="acp-tool-command acp-tool-command--output"
              aria-label="Command output"
              tabIndex={0}
            >
              {outputSummary || "No output"}
            </pre>
          ) : null}
        </div>
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
