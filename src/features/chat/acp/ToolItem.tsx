import { useId } from "react";

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

type ToolProjectionItem = Extract<
  AcpItem,
  { kind: "toolCall" | "toolUpdateResult" }
>;

export type OpenResourceTarget = "file" | "diff";

interface ToolItemProps {
  item: ToolProjectionItem;
  linkedResult?: Extract<AcpItem, { kind: "toolUpdateResult" }>;
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
              {location.label} · unavailable
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
    case "file":
    case "diff":
      // Tool locations admit file reads only. Saved Diff authority comes from
      // the separate file-change projection carrying its immutable DiffRef.
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
            locations={presentation.resources.map((resource) => ({
              kind: "resource" as const,
              resource,
            }))}
            target="file"
            onOpenResource={onOpenResource}
          />
        </>
      );
    case "mcp":
      return (
        <p>
          {presentation.serverLabel} · {presentation.toolLabel}
        </p>
      );
    case "subagent":
      return <p>Harness-owned activity</p>;
    case "web": {
      const payload = item.payload;
      const browse =
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
    case "command": {
      const output = outputSummary;
      return (
        <>
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
    case "generic":
      return presentation.summary ? <p>{presentation.summary}</p> : null;
  }
}

export function ToolItem({
  item,
  linkedResult,
  allowBrowse = true,
  onOpenResource,
}: ToolItemProps) {
  const descriptionId = useId();
  const callOutput =
    "outputSummary" in item.payload
      ? (item.payload.outputSummary ?? null)
      : null;
  const outputSummary =
    callOutput ?? linkedResult?.payload.outputSummary ?? null;
  const inputSummary =
    item.kind === "toolCall" ? item.payload.inputSummary : null;
  const detail =
    item.payload.presentation.kind === "command"
      ? outputSummary
      : (outputSummary ?? inputSummary);
  const commandOutput =
    item.payload.presentation.kind === "command" && outputSummary !== null;
  const callTerminal = terminalToolStates.has(item.toolState);
  const visualState = callTerminal
    ? item.toolState
    : (linkedResult?.toolState ?? item.toolState);
  const label = item.payload.presentation.label;
  const status = toolStateLabels[visualState];
  const terminal = terminalToolStates.has(visualState);
  const kind = item.payload.presentation.kind;
  return (
    <article
      className={`acp-activity-card acp-activity-card--tool acp-tool-card acp-tool-card--${visualState}`}
      role="group"
      aria-label={label}
      aria-describedby={detail ? descriptionId : undefined}
      data-activity-kind={kind}
      data-activity-state={visualState}
      data-acp-item-id={item.id}
      data-tool-result-id={linkedResult?.id}
    >
      {detail && !commandOutput ? (
        <span className="sr-only" id={descriptionId}>
          {detail}
        </span>
      ) : null}
      <header className="acp-activity-card__header acp-tool-card__header">
        <strong>{label}</strong>
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
      {detail && !commandOutput ? (
        <details className="acp-details">
          <summary>Tool details</summary>
          <p>{detail}</p>
        </details>
      ) : null}
    </article>
  );
}
