import type {
  AcpItem,
  ItemLifecycle,
  ToolState,
  TurnState,
  TurnTerminalReason,
  TurnProjection,
} from "../../../domain/acp";

export const terminalItemLifecycles: ReadonlySet<ItemLifecycle> = new Set([
  "completed",
  "failed",
  "cancelled",
  "unsupported",
]);

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

export const turnTerminalReasonLabels: Record<TurnTerminalReason, string> = {
  authRequired: "Not signed in",
  harnessExited: "Harness exited",
  protocolError: "Protocol error",
  cancelledByUser: "Cancelled by you",
  refusedVersion: "Version refused",
  timeout: "Timed out",
  other: "Reason unavailable",
};

export function getTurnStatusLabel(turn: TurnProjection): string {
  return (turn.state === "interrupted" || turn.state === "cancelled") &&
    turn.terminalReason?.kind === "cancelledByUser"
    ? turnTerminalReasonLabels.cancelledByUser
    : turnStateLabels[turn.state];
}

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

/** A harness or browser permission that still awaits the person's decision. */
export function awaitingPermission(item: AcpItem): item is PermissionItem {
  return (
    item.kind === "permissionRequest" &&
    item.toolState === "waitingForApproval" &&
    (item.payload.state === "received" || item.payload.state === "asked")
  );
}

export function getPermissionStatus(item: PermissionItem) {
  const failedClosed =
    item.lifecycle === "failed" && item.toolState === "failed";
  const declined = item.payload.state === "resolvedDeclined";
  const allowed = item.payload.state === "resolvedAllowed";
  const cancelled = item.payload.state === "cancelled";
  const allowing =
    item.payload.state === "allowPending" ||
    item.payload.state === "allowCommitted";
  const asking = item.payload.state === "asked";
  const visual = failedClosed
    ? "Failed closed"
    : cancelled
      ? "Cancelled"
      : allowed
        ? "Allowed"
        : allowing
          ? "Allowing"
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
      : cancelled
        ? "Permission cancelled"
        : allowed
          ? "Permission allowed"
          : declined
            ? "Permission declined"
            : visual,
    terminal: failedClosed || declined || allowed || cancelled,
  };
}
