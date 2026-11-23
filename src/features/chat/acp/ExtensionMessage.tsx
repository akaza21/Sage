import { useId, useState } from "react";
import type { AcpItem } from "../../../domain/acp";
import { safeNullableText, safeText } from "../../../services/acpDesktopGuards";

// Long or unbroken details wrap and scroll inside a bounded block.
const detailsStyle = {
  whiteSpace: "pre-wrap",
  overflowWrap: "anywhere",
  maxHeight: "16rem",
  overflow: "auto",
} as const;

/** JSON details are indented; clipped or other text is shown as it came. */
function readable(details: string): string {
  try {
    return JSON.stringify(JSON.parse(details), null, 2);
  } catch {
    return details;
  }
}

/**
 * A displayed extension custom message: its label, safe text, and details
 * behind a disclosure with no DOM while collapsed. The always-loaded
 * projection keeps the raw payload, so it is decoded and bounded here.
 */
export default function ExtensionMessage({
  item,
}: {
  item: Extract<AcpItem, { kind: "extensionMessage" }>;
}) {
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  const { payload } = item;
  const label = safeText(payload.label, 128);
  const text = safeText(payload.text, 16_384);
  const details = safeNullableText(payload.details, 16_384);
  if (label === null || text === null || details === undefined)
    return (
      <p className="acp-unknown" data-activity-kind="unknown">
        An extension message could not be shown.
      </p>
    );
  return (
    <article
      className="acp-activity acp-activity-card"
      data-activity-kind="extensionMessage"
    >
      <strong>{label}</strong>
      <p className="acp-activity__output">{text}</p>
      {details !== null ? (
        <>
          <button
            type="button"
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={() => setExpanded(!expanded)}
          >
            Details
          </button>
          {expanded ? (
            <pre id={detailsId} tabIndex={0} style={detailsStyle}>
              {readable(details)}
            </pre>
          ) : null}
        </>
      ) : null}
    </article>
  );
}
