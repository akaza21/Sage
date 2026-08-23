import { useId, useState } from "react";

import type { AcpItem, PermissionDecision } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { getPermissionStatus } from "./statusSemantics";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

const allowOnceLabel = "Allow once";

export function PermissionItem({ item }: { item: PermissionProjectionItem }) {
  const descriptionId = useId();
  const [submitting, setSubmitting] = useState(false);
  const status = getPermissionStatus(item);
  const actionable =
    item.toolState === "waitingForApproval" &&
    (item.payload.state === "received" || item.payload.state === "asked");

  async function decide(decision: PermissionDecision) {
    setSubmitting(true);
    try {
      await acpDesktop.resolvePermissionDecision({
        projectId: item.projectId,
        chatId: item.chatId,
        sessionId: item.sessionId,
        requestId: item.payload.requestId,
        decision,
      });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <article
      className="acp-permission"
      role="group"
      aria-label="Permission request"
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        Choose how Sage should handle this permission request
      </span>
      <header>
        <strong>{item.payload.label}</strong>
        <span
          role="status"
          aria-label={status.accessible}
          aria-live="off"
          aria-atomic="true"
        >
          {status.visual}
        </span>
      </header>
      <details className="acp-details">
        <summary>Details</summary>
        <p>{item.payload.summary}</p>
      </details>
      {actionable ? (
        <div className="acp-permission__actions">
          <button
            type="button"
            disabled={submitting}
            onClick={() => decide("ask")}
          >
            Ask
          </button>
          <button
            type="button"
            disabled={submitting}
            onClick={() => decide("allowOnce")}
          >
            {allowOnceLabel}
          </button>
          <button
            type="button"
            disabled={submitting}
            onClick={() => decide("deny")}
          >
            Deny
          </button>
        </div>
      ) : null}
    </article>
  );
}
