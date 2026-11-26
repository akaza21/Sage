import type {
  AcpChatPage,
  AcpItem,
  ItemLifecycle,
  ToolState,
  TurnProjection,
  TurnState,
  TurnTerminalReason,
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

/** The newest unfinished turn, else the newest; only `sessionId`'s if given. */
export function currentOrLatestTurn(
  page: AcpChatPage,
  sessionId?: string,
): TurnProjection | null {
  let current: TurnProjection | null = null;
  let latest: TurnProjection | null = null;
  for (const turn of page.turns) {
    if (sessionId !== undefined && turn.sessionId !== sessionId) continue;
    if (!latest || turn.ordinal > latest.ordinal) latest = turn;
    if (
      !terminalTurnStates.has(turn.state) &&
      (!current || turn.ordinal > current.ordinal)
    ) {
      current = turn;
    }
  }
  return current ?? latest;
}

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
  other: "The harness gave no error details.",
};

export function getTurnStatusLabel(turn: TurnProjection): string {
  return (turn.state === "interrupted" || turn.state === "cancelled") &&
    turn.terminalReason?.kind === "cancelledByUser"
    ? turnTerminalReasonLabels.cancelledByUser
    : turnStateLabels[turn.state];
}

/** A stopped turn may carry its denial on a tool rather than its terminal event. */
export function turnReasonMessage(turn: TurnProjection): string | undefined {
  const detail = turn.terminalReason?.message?.trim();
  if (detail) return detail;
  if (turn.state === "failed")
    return turnReasonErrorItem(turn)?.payload.message?.trim();
  if (turn.state !== "interrupted" && turn.state !== "cancelled") return;
  for (let index = turn.items.length; index--;) {
    const item = turn.items[index];
    if (item.kind === "error" && item.payload.kind === undefined) {
      const message = item.payload.message?.trim();
      if (message) return message;
    }
    if (
      item.kind === "toolCall" &&
      item.toolState === "declined" &&
      item.payload.outputSummary?.trim()
    )
      return item.payload.outputSummary;
  }
}

/** The same retained source record used for a failed turn's fallback reason. */
export function turnReasonErrorItem(turn: TurnProjection) {
  for (let index = turn.items.length; index--;) {
    const item = turn.items[index];
    if (item.kind === "error" && item.payload.kind === undefined) return item;
  }
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
  const terminal = failedClosed || declined || allowed || cancelled;
  return {
    visual,
    accessible: terminal ? `Permission ${visual.toLowerCase()}` : visual,
    terminal,
  };
}

/** A background task, or a running subagent that can still be stopped: what Resources lists. */
export const isBackgroundProcess = (row: {
  kind?: unknown;
  state?: unknown;
  stoppable?: unknown;
}) =>
  row.kind === "backgroundTask" ||
  (row.kind === "subagent" && row.state === "running" && !!row.stoppable);

/** The session still runs background work. Busy only: Resources decodes the rows it shows. */
export const backgroundWorkRunning = (page: AcpChatPage) =>
  !!(
    page.sessionSurface as { activities?: { state?: unknown }[] } | undefined
  )?.activities?.some?.(
    (row) => row?.state === "running" && isBackgroundProcess(row),
  );
