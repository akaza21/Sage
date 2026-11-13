import { useWorkbenchStore } from "../app/store";
import type { AcpChatStatus, TurnState } from "../domain/acp";
import type { SessionStatus } from "../domain/workbench";
import { setAgentRailApproval } from "./agentRailInteractions";

export function railStatusForTurn(state?: TurnState): SessionStatus {
  if (!state) return "idle";
  if (state === "completed") return "complete";
  if (state === "failed" || state === "deliveryUnknown") return "failed";
  if (state === "interrupted" || state === "cancelled") return "idle";
  if (state === "waitingApproval" || state === "waitingUser") return "waiting";
  return "working";
}

export function mirrorAgentRailStatus(page: AcpChatStatus) {
  const state = useWorkbenchStore.getState();
  if (state.projectTransitioning || state.project?.id !== page.projectId)
    return;
  const tab = state.tabs.find(
    (tab) => tab.id === page.chatId && tab.kind === "chat",
  );
  if (!tab) return;
  const turn = page.latestTurn;
  const status = railStatusForTurn(turn?.state);
  setAgentRailApproval(page.chatId, turn?.state === "waitingApproval");
  const metadataChanged = tab.title !== page.title || tab.status !== status;
  const updatedAt = turn
    ? Math.max(
        tab.updatedAt ?? 0,
        turn.createdAt,
        turn.terminalAt ?? 0,
        metadataChanged ? turn.updatedAt : 0,
      )
    : tab.updatedAt;
  // The latest Turn's finish time is the read watermark; a running Turn
  // keeps the last one so the chat cannot turn Unread mid-run.
  const finishedAt =
    turn?.terminalAt != null
      ? Math.max(tab.finishedAt ?? 0, turn.terminalAt)
      : tab.finishedAt;
  if (
    !metadataChanged &&
    updatedAt === tab.updatedAt &&
    finishedAt === tab.finishedAt
  )
    return;
  useWorkbenchStore.setState({
    tabs: state.tabs.map((item) =>
      item === tab
        ? { ...item, title: page.title, status, updatedAt, finishedAt }
        : item,
    ),
  });
}
