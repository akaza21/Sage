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
      aria-label={`Harness collaboration: ${item.payload.label}`}
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        Read-only activity owned and scheduled by the harness
      </span>
      <header>
        <span className="acp-collaboration__identity">
          <span className="acp-collaboration__kind">Collaboration</span>
          <strong
            className="acp-collaboration__label"
            title={item.payload.label}
          >
            {item.payload.label}
          </strong>
        </span>
        <span
          className="acp-collaboration__status"
          role="status"
          aria-label={status}
          aria-live="off"
          data-lifecycle={item.lifecycle}
        >
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
