import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import {
  awaitingPermission,
  getTurnStatusLabel,
  terminalTurnStates,
} from "./statusSemantics";

export const MAX_LIVE_SNAPSHOT_TEXT = 280;

export type ChatLiveStatusTone = "idle" | "working" | "waiting" | "failed";

export type ChatLiveSnapshot = {
  chatId: string;
  statusLabel: string;
  statusTone: ChatLiveStatusTone;
  latestText: string;
  streaming: boolean;
  waiting: boolean;
  working: boolean;
};

const waitingStates = new Set<TurnProjection["state"]>([
  "waitingApproval",
  "waitingUser",
]);

const workingStates = new Set<TurnProjection["state"]>([
  "queued",
  "sending",
  "running",
  "waitingModel",
  "waitingTool",
  "cancelling",
]);

const failedStates = new Set<TurnProjection["state"]>([
  "failed",
  "deliveryUnknown",
]);

function currentTurn(page: AcpChatPage): TurnProjection | null {
  let current: TurnProjection | null = null;
  let latest: TurnProjection | null = null;
  for (const turn of page.turns) {
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

function latestTextItem(turn: TurnProjection): string {
  let text = "";
  let ordinal = -1;
  for (const item of turn.items) {
    if (item.kind !== "text") continue;
    if (item.ordinal >= ordinal) {
      ordinal = item.ordinal;
      text = item.payload.text;
    }
  }
  return text;
}

function capLiveText(text: string): string {
  if (text.length <= MAX_LIVE_SNAPSHOT_TEXT) return text;
  return `${text.slice(0, MAX_LIVE_SNAPSHOT_TEXT - 1)}…`;
}

function isStreaming(turn: TurnProjection, items: readonly AcpItem[]): boolean {
  if (!terminalTurnStates.has(turn.state)) return true;
  for (const item of items) {
    if (item.lifecycle === "streaming") return true;
  }
  return false;
}

export function projectChatLiveSnapshot(
  chatId: string,
  page: AcpChatPage | null,
): ChatLiveSnapshot {
  const turn = page ? currentTurn(page) : null;
  if (!turn) {
    return {
      chatId,
      statusLabel: "Idle",
      statusTone: "idle",
      latestText: "",
      streaming: false,
      waiting: false,
      working: false,
    };
  }
  const approval =
    !terminalTurnStates.has(turn.state) &&
    (turn.state === "waitingApproval" || turn.items.some(awaitingPermission));
  return {
    chatId,
    statusLabel: approval ? "Awaiting approval" : getTurnStatusLabel(turn),
    statusTone: failedStates.has(turn.state)
      ? "failed"
      : approval || waitingStates.has(turn.state)
        ? "waiting"
        : workingStates.has(turn.state)
          ? "working"
          : "idle",
    latestText: capLiveText(latestTextItem(turn)),
    streaming: isStreaming(turn, turn.items),
    waiting: approval || waitingStates.has(turn.state),
    working: !approval && workingStates.has(turn.state),
  };
}
