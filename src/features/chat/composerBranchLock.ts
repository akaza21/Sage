import type { WorkbenchState } from "../../app/store";
import { hasActiveAcpTurn } from "../../app/acpProjectionRegistry";
import { sessionRailLive } from "./acp/sessionRail";

/** All chat ownership and tab statuses belong to the loaded project. Git
 * checkout changes its shared workspace, including chats hidden by a draft. */
export function selectComposerBranchTurnBusy(state: WorkbenchState): boolean {
  return (
    state.sending ||
    Object.values(state.chatUiById).some((chat) => chat.sending) ||
    state.tabs.some(
      (tab) =>
        tab.kind === "chat" &&
        (sessionRailLive(tab.status) || hasActiveAcpTurn(tab.id)),
    ) ||
    !!state.pendingFirstSend ||
    state.newChatDraft?.submission.status === "submitting"
  );
}
