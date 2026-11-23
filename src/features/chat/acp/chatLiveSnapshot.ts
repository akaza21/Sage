import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import {
  awaitingPermission,
  currentOrLatestTurn,
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

function isStreamingItem(item: AcpItem): boolean {
  return item.lifecycle === "streaming";
}

export function projectChatLiveSnapshot(
  chatId: string,
  page: AcpChatPage | null,
): ChatLiveSnapshot {
  const turn = page ? currentOrLatestTurn(page) : null;
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
  const terminal = terminalTurnStates.has(turn.state);
  const approval =
    !terminal &&
    (turn.state === "waitingApproval" || turn.items.some(awaitingPermission));
  const waiting = approval || turn.state === "waitingUser";
  const working = !waiting && !terminal;
  return {
    chatId,
    statusLabel: approval ? "Awaiting approval" : getTurnStatusLabel(turn),
    statusTone:
      turn.state === "failed" || turn.state === "deliveryUnknown"
        ? "failed"
        : waiting
          ? "waiting"
          : working
            ? "working"
            : "idle",
    latestText: capLiveText(latestTextItem(turn)),
    streaming: !terminal || turn.items.some(isStreamingItem),
    waiting,
    working,
  };
}
