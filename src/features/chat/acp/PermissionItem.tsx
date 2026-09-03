import { useId } from "react";

import type { AcpItem } from "../../../domain/acp";
import { getPermissionStatus } from "./statusSemantics";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

export function PermissionItem({ item }: { item: PermissionProjectionItem }) {
  const descriptionId = useId();
  const status = getPermissionStatus(item);
  const awaitingDecision =
    item.toolState === "waitingForApproval" &&
    (item.payload.state === "received" || item.payload.state === "asked");

  return (
    <article
      className="acp-permission"
      role="group"
      aria-label="Permission request"
      aria-describedby={descriptionId}
    >
      <span className="sr-only" id={descriptionId}>
        Sage automatically denies permission requests in this build
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
      {awaitingDecision ? (
        <p className="acp-permission__unavailable" role="note">
          Permission actions are unavailable. Sage automatically denies
          permission requests in this build.
        </p>
      ) : null}
    </article>
  );
}
