import { useEffect, useEffectEvent } from "react";
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
  const updatedAt = turn
    ? Math.max(tab.updatedAt ?? 0, turn.updatedAt)
    : tab.updatedAt;
  if (
    tab.title === page.title &&
    tab.status === status &&
    tab.updatedAt === updatedAt
  )
    return;
  useWorkbenchStore.setState({
    tabs: state.tabs.map((item) =>
      item === tab ? { ...item, title: page.title, status, updatedAt } : item,
    ),
  });
}

export function useAgentRailProjection(page: AcpChatPage | null) {
  const mirror = useEffectEvent(() => {
    if (page) mirrorAgentRailProjection(page);
  });
  const turn = page?.turns.at(-1);
  useEffect(() => {
    mirror();
  }, [page?.projectId, page?.chatId, page?.title, turn?.id, turn?.state]);
}
