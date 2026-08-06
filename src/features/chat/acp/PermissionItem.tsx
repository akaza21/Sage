import { useId } from "react";

import type { AcpItem } from "../../../domain/acp";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

const permissionLabels = {
  received: "Declining",
  declinePending: "Declining",
  declineCommitted: "Declined",
  responseQueued: "Declined",
  resolvedDeclined: "Declined",
} as const;

export function PermissionItem({ item }: { item: PermissionProjectionItem }) {
  const descriptionId = useId();
  const failedClosed =
    item.lifecycle === "failed" && item.toolState === "failed";
  const status = failedClosed
    ? "Failed closed"
    : permissionLabels[item.payload.state];
  const terminal = failedClosed || item.payload.state === "resolvedDeclined";
  const accessibleStatus = failedClosed
    ? "Permission failed closed"
    : terminal
      ? "Permission declined"
      : status;
  return (
    <article
      className="acp-permission"
      role="group"
      aria-label="Permission request"
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        Sage declines permission requests in this read-only foundation
      </span>
      <header>
        <strong>{item.payload.label}</strong>
        <span
          role="status"
          aria-label={accessibleStatus}
          aria-live="assertive"
          aria-atomic="true"
        >
          {status}
        </span>
      </header>
      <details className="acp-details">
        <summary>Details</summary>
        <p>{item.payload.summary}</p>
      </details>
    </article>
  );
}
