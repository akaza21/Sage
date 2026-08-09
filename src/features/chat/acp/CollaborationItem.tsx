import { useId } from "react";

import type { AcpItem } from "../../../domain/acp";

type CollaborationProjectionItem = Extract<AcpItem, { kind: "collaboration" }>;

const collaborationStateLabels = {
  started: "Observed",
  streaming: "Active",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  unsupported: "Unsupported",
} as const;

export function CollaborationItem({
  item,
}: {
  item: CollaborationProjectionItem;
}) {
  const descriptionId = useId();
  const status = collaborationStateLabels[item.lifecycle];
  return (
    <article
      className="acp-collaboration"
      role="group"
      aria-label="Harness collaboration"
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        Read-only activity owned and scheduled by the harness
      </span>
      <header>
        <strong>{item.payload.label}</strong>
        <span role="status" aria-label={status} aria-live="off">
          {status}
        </span>
      </header>
      <p>{item.payload.summary}</p>
      {item.payload.receiverLabels.length > 0 ? (
        <p className="acp-collaboration__receivers">
          Participants: {item.payload.receiverLabels.join(", ")}
        </p>
      ) : null}
    </article>
  );
}
