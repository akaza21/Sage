import { useEffect } from "react";

import { useWorkbenchStore } from "../app/store";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { AcpChatStatus, TurnState } from "../domain/acp";
import type { SessionStatus } from "../domain/workbench";
import {
  sessionRailFingerprint,
  sessionRailLive,
} from "../features/chat/acp/sessionRail";
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
  const statusChanged = tab.status !== status;
  const metadataChanged = tab.title !== page.title || statusChanged;
  const updatedAt = turn
    ? Math.max(
        tab.updatedAt ?? 0,
        turn.createdAt,
        turn.terminalAt ?? 0,
        metadataChanged ? turn.updatedAt : 0,
      )
    : tab.updatedAt;
  if (!metadataChanged && updatedAt === tab.updatedAt) return;
  useWorkbenchStore.setState({
    tabs: state.tabs.map((item) =>
      item === tab
        ? {
            ...item,
            title: page.title,
            status,
            updatedAt,
            // The native viewed mark describes the chat as loaded; new turn
            // activity makes it stale until the chat is viewed again.
            viewed:
              statusChanged || updatedAt !== tab.updatedAt
                ? undefined
                : item.viewed,
          }
        : item,
    ),
  });
}

/**
 * Saves that the user has seen the chat they are viewing once it holds
 * finished output, so it does not come back as Unread after a reload.
 */
export function useMarkViewedChat(
  projectId: string,
  chat:
    | {
        id: string;
        status?: SessionStatus;
        updatedAt?: number;
        viewed?: boolean;
      }
    | undefined,
) {
  const chatId = chat?.id;
  const unseen =
    chat !== undefined &&
    chat.viewed !== true &&
    !sessionRailLive(chat.status) &&
    sessionRailFingerprint(chat.status, chat.updatedAt).length > 0;
  useEffect(() => {
    if (chatId && unseen) saveChatViewed(projectId, chatId);
  }, [chatId, unseen, projectId]);
}

/**
 * Saves the native viewed mark. In this session the rail's seen fingerprints
 * already show the chat as read; the mark is what a reload reads back. A
 * failed write leaves the chat unread after a reload until it is viewed
 * again, so it needs no report.
 */
export function saveChatViewed(projectId: string, chatId: string) {
  void getWorkbenchPorts()
    .chat.markChatViewed({ projectId, chatId })
    .catch(() => undefined);
}
