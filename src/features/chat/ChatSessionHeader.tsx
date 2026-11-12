import type { AcpChatPage } from "../../domain/acp";
import type { ChatMessage } from "../../domain/workbench";
import { SessionWorkingPill } from "./SessionWorkingPill";
import { ChatHandoffControl } from "./lazyParts";
export function ChatSessionHeader({
  page,
  messages,
  workingTurnCount,
  projectTransitioning,
  busy,
  canCloseSession,
  canEndSession,
  closeSession,
  endSession,
  harnessStatus,
  onCommitted,
}: {
  page: AcpChatPage | null;
  messages: ChatMessage[];
  workingTurnCount: number;
  projectTransitioning: boolean;
  busy: boolean;
  canCloseSession: boolean;
  canEndSession: boolean;
  closeSession: () => void;
  endSession: () => void;
  harnessStatus: string;
  onCommitted: () => void;
}) {
  return (
    <header className="surface-header">
      <SessionWorkingPill
        count={workingTurnCount}
        hidden={projectTransitioning}
      />
      <div className="session-controls">
        {/* Side chats render only under Agent chrome, so this header never
                belongs to one. */}
        <ChatHandoffControl
          acpPage={page}
          messages={messages}
          busy={busy}
          onCommitted={onCommitted}
        />
        {canCloseSession ? (
          <button
            className="button button--compact"
            type="button"
            disabled={busy || projectTransitioning}
            onClick={closeSession}
          >
            Close session
          </button>
        ) : null}
        {canEndSession ? (
          <button
            className="button button--compact"
            type="button"
            disabled={busy || projectTransitioning}
            onClick={endSession}
          >
            End session
          </button>
        ) : null}
        <div
          className="session-state"
          aria-label={`Harness status: ${harnessStatus}`}
        >
          {harnessStatus === "Live" ? (
            <span className="session-state__dot" />
          ) : null}
          {harnessStatus}
        </div>
      </div>
    </header>
  );
}
