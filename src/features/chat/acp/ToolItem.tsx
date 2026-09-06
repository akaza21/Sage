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
  allowBrowse,
  onOpenResource,
}: Pick<ToolItemProps, "item" | "allowBrowse" | "onOpenResource">) {
  const presentation = item.payload.presentation;
  switch (presentation.kind) {
    case "file":
    case "diff":
      return (
        <ResourceLocations
          locations={presentation.resources}
          target={presentation.kind}
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
        extractAdmittedBrowseUrl(
          "outputSummary" in payload ? (payload.outputSummary ?? "") : "",
        );
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
    case "command":
      return presentation.summary ? (
        <pre className="acp-tool-command" aria-label="Command">
          {presentation.summary}
        </pre>
      ) : null;
    case "generic":
      return presentation.summary ? <p>{presentation.summary}</p> : null;
  }
}

export function ToolItem({
  item,
  allowBrowse = true,
  onOpenResource,
}: ToolItemProps) {
  const descriptionId = useId();
  const detail =
    item.kind === "toolCall"
      ? item.payload.inputSummary
      : item.payload.outputSummary;
  const label = item.payload.presentation.label;
  const status = toolStateLabels[item.toolState];
  const terminal = terminalToolStates.has(item.toolState);
  const kind = item.payload.presentation.kind;
  return (
    <article
      className={`acp-activity-card acp-activity-card--tool acp-tool-card acp-tool-card--${item.toolState}`}
      role="group"
      aria-label={label}
      aria-describedby={detail ? descriptionId : undefined}
      data-activity-kind={kind}
      data-activity-state={item.toolState}
    >
      {detail ? (
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
        allowBrowse={allowBrowse}
        onOpenResource={onOpenResource}
      />
      {detail ? (
        <details className="acp-details">
          <summary>Tool details</summary>
          <p>{detail}</p>
        </details>
      ) : null}
    </article>
  );
}
