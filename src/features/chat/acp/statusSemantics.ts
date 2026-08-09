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
  const visual = failedClosed
    ? "Failed closed"
    : declined ||
        item.payload.state === "declineCommitted" ||
        item.payload.state === "responseQueued"
      ? "Declined"
      : "Declining";
  return {
    visual,
    accessible: failedClosed
      ? "Permission failed closed"
      : declined
        ? "Permission declined"
        : visual,
    terminal: failedClosed || declined,
  };
}
