import { Info } from "../ui/leanIcons";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { useChatsAwaitingApproval } from "../features/chat/acp/awaitingApproval";
import type { SessionStatus } from "../domain/workbench";
import { sessionRailStatusLabel } from "../features/chat/acp/sessionRail";
import { useAgentRailInteractions } from "./agentRailInteractions";
import { useParentChatStatus } from "./sideChatState";

export function AgentTaskStatus({
  chatId,
  snapshot,
  harnessStatus,
  status,
}: {
  chatId: string;
  snapshot: ChatLiveSnapshot | null;
  harnessStatus?: string;
  status?: SessionStatus;
}) {
  const awaiting = useChatsAwaitingApproval();
  const interactions = useAgentRailInteractions();
  const parentStatus = useParentChatStatus();
  const resolvedStatus = parentStatus(chatId, status);
  const label =
    awaiting.has(chatId) || interactions.approval.has(chatId)
      ? "Awaiting approval"
      : resolvedStatus
        ? sessionRailStatusLabel(resolvedStatus) || "Idle"
        : (snapshot?.statusLabel ?? "Idle");
  return (
    <>
      <span
        className="sr-only"
        role="status"
        aria-label={`Chat status: ${label}`}
        title={label}
      >
        {label}
      </span>
      {harnessStatus && !["Idle", "Ready", "Live"].includes(harnessStatus) ? (
        <details className="agent-task-header__harness-status">
          <summary
            aria-label={`Harness status: ${harnessStatus}`}
            title={harnessStatus}
            tabIndex={0}
          >
            <span
              className="agent-task-header__harness-icon"
              aria-hidden="true"
            >
              <Info size={14} />
            </span>
            <span className="sr-only">{harnessStatus}</span>
          </summary>
          <p>{harnessStatus}</p>
        </details>
      ) : null}
    </>
  );
}
