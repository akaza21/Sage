import { useContext } from "react";
import { ShieldCheck } from "../../../ui/leanIcons";

import type { AcpItem } from "../../../domain/acp";
import { awaitingPermission, getPermissionStatus } from "./statusSemantics";
import { WorkedTurnTerminal } from "./WorkedActivityVisibility";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

/**
 * The transcript record of a permission. While a live Turn awaits a decision
 * the request is answered in the composer slot (`PermissionPrompt`); once
 * settled the transcript keeps this compact record. A terminal Turn cannot
 * hold a pending decision, so an open request there reads "Not answered".
 */
export function PermissionItem({ item }: { item: PermissionProjectionItem }) {
  const turnEnded = useContext(WorkedTurnTerminal);
  const openRequest = awaitingPermission(item as AcpItem);
  if (openRequest && !turnEnded) return null;
  const status = openRequest
    ? { visual: "Not answered", accessible: "Permission not answered" }
    : getPermissionStatus(item);
  return (
    <article
      className="acp-permission acp-permission--settled"
      role="group"
      aria-label={
        item.payload.source === "browser"
          ? "Browser permission"
          : "Permission request"
      }
      data-activity-state={item.toolState}
      data-acp-item-id={item.id}
    >
      <details>
        <summary>
          <ShieldCheck size={14} aria-hidden="true" />
          <span className="acp-permission__label">{item.payload.label}</span>
          <span aria-hidden="true"> · </span>
          <span
            className="acp-permission__status"
            role="status"
            aria-label={status.accessible}
            aria-live="off"
            aria-atomic="true"
          >
            {status.visual}
          </span>
        </summary>
        <p>{item.payload.summary}</p>
      </details>
    </article>
  );
}
