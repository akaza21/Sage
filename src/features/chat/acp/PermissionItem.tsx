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
  const status = permissionLabels[item.payload.state];
  const terminal = item.payload.state === "resolvedDeclined";
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
          aria-label={terminal ? "Permission declined" : status}
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
