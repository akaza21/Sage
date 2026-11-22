import { useState } from "react";
import type { AcpItem } from "../../../domain/acp";

export function ExtensionMessage({
  item,
}: {
  item: Extract<AcpItem, { kind: "extensionMessage" }>;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <article
      className="acp-activity acp-activity-card"
      data-activity-kind="extensionMessage"
    >
      <strong>{item.payload.label}</strong>
      <p className="acp-activity__output">{item.payload.text}</p>
      {item.payload.details !== null ? (
        <>
          <button
            type="button"
            aria-expanded={expanded}
            onClick={() => setExpanded(!expanded)}
          >
            Details
          </button>
          {expanded ? <pre>{item.payload.details}</pre> : null}
        </>
      ) : null}
    </article>
  );
}
