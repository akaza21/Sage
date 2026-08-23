import type { AcpItem, ToolState, TurnState } from "../../../domain/acp";

export const terminalTurnStates: ReadonlySet<TurnState> = new Set([
  "completed",
  "interrupted",
  "cancelled",
  "failed",
  "deliveryUnknown",
]);

export const turnStateLabels: Record<TurnState, string> = {
  queued: "Queued",
  sending: "Sending",
  running: "Running",
  waitingModel: "Waiting for model",
  waitingTool: "Waiting for tool",
  waitingApproval: "Waiting for approval",
  waitingUser: "Waiting for user",
  cancelling: "Cancelling",
  completed: "Completed",
  interrupted: "Interrupted",
  cancelled: "Cancelled",
  failed: "Failed",
  deliveryUnknown: "Delivery unknown",
};

export const terminalToolStates: ReadonlySet<ToolState> = new Set([
  "completed",
  "failed",
  "declined",
  "cancelled",
]);

export const toolStateLabels: Record<ToolState, string> = {
  pending: "Pending",
  inProgress: "In progress",
  waitingForApproval: "Waiting for approval",
  completed: "Completed",
  failed: "Failed",
  declined: "Declined",
  cancelled: "Cancelled",
};

type PermissionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

export function getPermissionStatus(item: PermissionItem) {
  const failedClosed =
    item.lifecycle === "failed" && item.toolState === "failed";
  const declined = item.payload.state === "resolvedDeclined";
  const allowed = item.payload.state === "resolvedAllowed";
  const allowing =
    item.payload.state === "allowPending" ||
    item.payload.state === "allowCommitted";
  const asking = item.payload.state === "asked";
  const visual = failedClosed
    ? "Failed closed"
    : allowed
      ? "Allowed once"
      : allowing
        ? "Allowing once"
        : declined || item.payload.state === "declineCommitted"
      ? "Declined"
      : asking
        ? "Asking again"
        : item.payload.state === "responseQueued"
          ? "Responding"
          : item.payload.state === "declinePending"
            ? "Declining"
            : "Waiting for approval";
  return {
    visual,
    accessible: failedClosed
      ? "Permission failed closed"
      : allowed
        ? "Permission allowed once"
      : declined
        ? "Permission declined"
        : visual,
    terminal: failedClosed || declined || allowed,
  };
}
