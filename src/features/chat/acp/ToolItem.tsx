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
} from "lucide-react";

import { useAgentArtifactFollowStore } from "../../../app/agentArtifactFollowStore";
import type { AcpItem, SafeResourceLocation } from "../../../domain/acp";
import type { ResourceRef } from "../../../domain/workbench";
import {
  extractAdmittedBrowseUrl,
  harnessBrowseOwner,
} from "../../preview/harnessBrowse";
import { ChatFileChip } from "./FileChip";
import { chatFileChipBasename } from "./chatFileChip";
import { CURATED_BROWSE_TOOL_NAME, createToolHost } from "./toolHost";
import { terminalToolStates, toolStateLabels } from "./statusSemantics";
import { toolDurationLabel } from "./toolActivityRollup";

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

interface ToolItemProps {
  item: ToolProjectionItem;
  allowBrowse?: boolean;
  onOpenResource?: (resource: ResourceRef, target: OpenResourceTarget) => void;
}

function ResourceLocations({
  locations,
  target,
  onOpenResource,
}: {
  locations: readonly SafeResourceLocation[];
  target: OpenResourceTarget;
  onOpenResource?: ToolItemProps["onOpenResource"];
}) {
  if (locations.length === 0) return null;
  return (
    <ul className="acp-resource-list" aria-label="Referenced resources">
      {locations.map((location, index) => (
        <li key={`${location.kind}:${index}`}>
          {location.kind === "resource" ? (
            chatFileChipBasename(location.resource.relativePath) ? (
              onOpenResource ? (
                <button
                  type="button"
                  className="acp-resource-action"
                  aria-label={`Open ${target} ${location.resource.relativePath}`}
                  data-resource-root-id={location.resource.rootId}
                  data-resource-relative-path={location.resource.relativePath}
                  onClick={() => onOpenResource(location.resource, target)}
                >
                  <ChatFileChip path={location.resource.relativePath} />
                </button>
              ) : (
                <ChatFileChip path={location.resource.relativePath} />
              )
            ) : (
              <span className="acp-resource-unavailable">unavailable</span>
            )
          ) : (
            <span className="acp-resource-unavailable">
              {location.reason === "missing"
                ? "File location unavailable"
                : `${location.label} · unavailable`}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

function Presentation({
  item,
  outputSummary,
  allowBrowse,
  onOpenResource,
  descriptionId,
}: Pick<ToolItemProps, "item" | "allowBrowse" | "onOpenResource"> & {
  outputSummary: string | null;
  descriptionId: string;
}) {
  const presentation = item.payload.presentation;
  switch (presentation.kind) {
    case "read":
    case "edit":
    case "delete":
    case "move":
      return (
        <ResourceLocations
          locations={presentation.resources}
          target="file"
          onOpenResource={onOpenResource}
        />
      );
    case "search":
      return (
        <>
          <p>{presentation.querySummary}</p>
          <ResourceLocations
            locations={presentation.resources}
            target="file"
            onOpenResource={onOpenResource}
          />
        </>
      );
    case "mcp":
      return null;
    case "subagent":
      return <p>Harness-owned activity</p>;
    case "fetch": {
      const payload = item.payload;
      const browse =
        extractAdmittedBrowseUrl(presentation.url ?? "") ??
        extractAdmittedBrowseUrl(presentation.summary ?? "") ??
        extractAdmittedBrowseUrl(
          "inputSummary" in payload ? (payload.inputSummary ?? "") : "",
        ) ??
        extractAdmittedBrowseUrl(outputSummary ?? "");
      return (
        <>
          {presentation.summary ? <p>{presentation.summary}</p> : null}
          {browse && allowBrowse !== false ? (
            <button
              type="button"
              className="acp-resource-action"
              onClick={() => {
                const owner = harnessBrowseOwner();
                if (
                  !owner ||
                  owner.projectId !== item.projectId ||
                  owner.chatId !== item.chatId ||
                  owner.sessionId !== item.sessionId ||
                  owner.runtimeEpoch !== item.runtimeEpoch
                ) {
                  return;
                }
                const opened = createToolHost().invoke(
                  CURATED_BROWSE_TOOL_NAME,
                  {
                    owner,
                    url: browse.href,
                  },
                );
                if (opened) {
                  useAgentArtifactFollowStore
                    .getState()
                    .pin(owner, { kind: "preview" });
                }
              }}
            >
              Open in Sage browser
            </button>
          ) : null}
        </>
      );
    }
    case "execute": {
      const output = outputSummary;
      const commandPayload = item.payload;
      const candidateLine =
        presentation.summary ??
        ("inputSummary" in commandPayload ? commandPayload.inputSummary : null);
      const commandLine =
        candidateLine && candidateLine.trim() === presentation.label.trim()
          ? null
          : candidateLine;
      return (
        <>
          {commandLine ? (
            <pre
              className="acp-tool-command acp-tool-command--line"
              aria-label="Command line"
            >
              {commandLine}
            </pre>
          ) : null}
          {output ? (
            <div className="acp-tool-command-frame">
              <p className="acp-tool-command__disclosure" role="note">
                Retained command output
              </p>
              <pre
                id={descriptionId}
                className="acp-tool-command"
                aria-label="Command output"
              >
                {output}
              </pre>
            </div>
          ) : null}
          <ResourceLocations
            locations={presentation.resources}
            target="file"
            onOpenResource={onOpenResource}
          />
        </>
      );
    }
    case "think":
      return presentation.summary ? <p>{presentation.summary}</p> : null;
    case "switchMode":
      return presentation.modeLabel ? <p>{presentation.modeLabel}</p> : null;
    case "other":
      return (
        <>
          {presentation.rawKind &&
          presentation.rawKind.trim() !== presentation.label.trim() ? (
            <span className="acp-tool-card__raw-kind">
              {presentation.rawKind}
            </span>
          ) : null}
          {presentation.summary ? <p>{presentation.summary}</p> : null}
        </>
      );
  }
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
        <span
          className="acp-state"
          role="status"
          aria-label={terminal ? `${label}: ${status}` : status}
          aria-live="off"
          aria-atomic="true"
        >
          {status}
        </span>
      </header>
      <Presentation
        item={item}
        outputSummary={outputSummary}
        allowBrowse={allowBrowse}
        onOpenResource={onOpenResource}
        descriptionId={descriptionId}
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
