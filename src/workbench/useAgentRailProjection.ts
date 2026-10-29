import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, TurnState } from "../domain/acp";
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

export function mirrorAgentRailProjection(page: AcpChatPage) {
  const state = useWorkbenchStore.getState();
  if (state.projectTransitioning || state.project?.id !== page.projectId)
    return;
  const tab = state.tabs.find(
    (tab) => tab.id === page.chatId && tab.kind === "chat",
  );
  if (!tab) return;
  const turn = page.turns.at(-1);
  const status = railStatusForTurn(turn?.state);
  setAgentRailApproval(page.chatId, turn?.state === "waitingApproval");
  if (tab.title === page.title && tab.status === status) return;
  const updatedAt = turn
    ? Math.max(tab.updatedAt ?? 0, turn.updatedAt)
    : tab.updatedAt;
  useWorkbenchStore.setState({
    tabs: state.tabs.map((item) =>
      item === tab ? { ...item, title: page.title, status, updatedAt } : item,
    ),
  });
}
