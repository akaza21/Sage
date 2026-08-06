import { useId } from "react";

import type { AcpItem } from "../../../domain/acp";

type UnknownProjectionItem = Extract<AcpItem, { kind: "unsupported" }>;

export function UnknownItem({ item }: { item: UnknownProjectionItem }) {
  const descriptionId = useId();
  return (
    <article
      className="acp-unknown"
      role="group"
      aria-label="Unsupported tool activity"
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        This harness activity is not supported by Sage
      </span>
      <header>
        <strong>Unsupported activity</strong>
        <span role="status" aria-label="Unsupported">
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
