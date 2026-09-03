import { useId } from "react";

import type { AcpItem } from "../../../domain/acp";

type UnknownProjectionItem = Extract<AcpItem, { kind: "unsupported" }>;

export function UnknownItem({ item }: { item: UnknownProjectionItem }) {
  const descriptionId = useId();
  return (
    <article
      className="acp-activity-card acp-activity-card--unknown acp-unknown"
      role="group"
      aria-label="Unsupported tool activity"
      aria-describedby={descriptionId}
      data-activity-kind="unknown"
      data-activity-state="unsupported"
    >
      <span className="sr-only" id={descriptionId}>
        This harness activity is not supported by Sage
      </span>
      <header>
        <strong>Unsupported activity</strong>
        <span
          className="acp-state"
          role="status"
          aria-label="Unsupported"
          aria-live="off"
        >
          Unsupported
        </span>
      </header>
      <details className="acp-details">
        <summary>Activity details</summary>
        <p>{item.payload.summary}</p>
        <span className="acp-unknown__category">{item.payload.category}</span>
        {item.payload.truncated ? <span>Summary truncated</span> : null}
      </details>
    </article>
  );
}
