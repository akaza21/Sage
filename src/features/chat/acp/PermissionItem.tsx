import type { AcpItem, SagePermissionRequestId } from "../../../domain/acp";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

interface PermissionItemProps {
  item: PermissionProjectionItem;
  onDeny?: (requestId: SagePermissionRequestId) => void;
}

const permissionLabels = {
  received: "Permission requested",
  declinePending: "Denying permission",
  declineCommitted: "Permission denial committed",
  responseQueued: "Permission denial queued",
  resolvedDeclined: "Permission denied",
} as const;

export function PermissionItem({ item, onDeny }: PermissionItemProps) {
  const canDeny = item.payload.state === "received" && Boolean(onDeny);
  return (
    <article className="acp-permission" aria-label="Permission request">
      <header>
        <strong>{item.payload.label}</strong>
        <span role="status" aria-live="assertive">
          {permissionLabels[item.payload.state]}
        </span>
      </header>
      <p>{item.payload.summary}</p>
      <button
        type="button"
        className="acp-deny-action"
        disabled={!canDeny}
        onClick={() => onDeny?.(item.payload.requestId)}
      >
        {item.payload.state === "received"
          ? "Deny permission"
          : permissionLabels[item.payload.state]}
      </button>
    </article>
  );
}
