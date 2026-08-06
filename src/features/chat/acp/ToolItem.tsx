import { useId } from "react";

import type { AcpItem, SafeResourceLocation } from "../../../domain/acp";
import type { ResourceRef } from "../../../domain/workbench";

type ToolProjectionItem = Extract<
  AcpItem,
  { kind: "toolCall" | "toolUpdateResult" }
>;

export type OpenResourceTarget = "file" | "diff";

interface ToolItemProps {
  item: ToolProjectionItem;
  onOpenResource?: (resource: ResourceRef, target: OpenResourceTarget) => void;
}

const toolStateLabels = {
  pending: "Pending",
  inProgress: "In progress",
  waitingForApproval: "Waiting for approval",
  completed: "Completed",
  failed: "Failed",
  declined: "Declined",
  cancelled: "Cancelled",
} as const;

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
            onOpenResource ? (
              <button
                type="button"
                className="acp-resource-action"
                aria-label={`Open ${target} ${location.resource.relativePath}`}
                onClick={() => onOpenResource(location.resource, target)}
              >
                {location.resource.relativePath}
              </button>
            ) : (
              <span>{location.resource.relativePath}</span>
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
  onOpenResource,
}: Pick<ToolItemProps, "item" | "onOpenResource">) {
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
    case "command":
    case "generic":
    case "web":
      return presentation.summary ? <p>{presentation.summary}</p> : null;
  }
}

export function ToolItem({ item, onOpenResource }: ToolItemProps) {
  const descriptionId = useId();
  const detail =
    item.kind === "toolCall"
      ? item.payload.inputSummary
      : item.payload.outputSummary;
  const label = item.payload.presentation.label;
  const status = toolStateLabels[item.toolState];
  const terminal = ["completed", "failed", "declined", "cancelled"].includes(
    item.toolState,
  );
  return (
    <article
      className={`acp-tool-card acp-tool-card--${item.toolState}`}
      role="group"
      aria-label={label}
      aria-describedby={detail ? descriptionId : undefined}
    >
      {detail ? (
        <span className="sr-only" id={descriptionId}>
          {detail}
        </span>
      ) : null}
      <header className="acp-tool-card__header">
        <strong>{label}</strong>
        <span
          className="acp-state"
          role="status"
          aria-label={terminal ? `${label}: ${status}` : status}
          aria-live="polite"
          aria-atomic="true"
        >
          {status}
        </span>
      </header>
      <Presentation item={item} onOpenResource={onOpenResource} />
      {detail ? (
        <details className="acp-details">
          <summary>
            {item.kind === "toolCall" ? "Input summary" : "Result summary"}
          </summary>
          <p>{detail}</p>
        </details>
      ) : null}
    </article>
  );
}
