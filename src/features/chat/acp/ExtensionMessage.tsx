import { useState } from "react";
import type { AcpItem } from "../../../domain/acp";
import {
  safeNullableText,
  safeText,
} from "../../../services/acpDesktopGuards";

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
            onClick={() => setExpanded(!expanded)}
          >
            Details
          </button>
          {expanded ? <pre>{details}</pre> : null}
        </>
      ) : null}
    </article>
  );
}
