import { AlertTriangle, CircleAlert, Info } from "../ui/leanIcons";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";

export function AgentTaskStatus({
  snapshot,
  harnessStatus,
}: {
  snapshot: ChatLiveSnapshot | null;
  harnessStatus?: string;
}) {
  const label = snapshot?.statusLabel ?? "Idle";
  const tone = snapshot?.statusTone ?? "idle";
  const StatusIcon =
    tone === "waiting" ? AlertTriangle : tone === "failed" ? CircleAlert : null;
  return (
    <>
      <span
        className={`agent-task-header__status agent-chat-chip agent-chat-chip--${tone}`}
        role="status"
        aria-label={`Chat status: ${label}`}
        title={label}
        tabIndex={0}
      >
        {tone === "working" ? (
          <span className="agent-chat-chip__spinner" aria-hidden="true" />
        ) : null}
        {StatusIcon ? (
          <StatusIcon
            className="agent-task-header__status-icon"
            size={12}
            aria-hidden="true"
          />
        ) : null}
        <span className="agent-task-header__status-label">{label}</span>
      </span>
      {harnessStatus ? (
        <details className="agent-task-header__harness-status">
          <summary
            aria-label={`Harness status: ${harnessStatus}`}
            title={harnessStatus}
            tabIndex={0}
          >
            <Info
              className="agent-task-header__harness-icon"
              size={14}
              aria-hidden="true"
            />
            <span>{harnessStatus}</span>
          </summary>
          <p>{harnessStatus}</p>
        </details>
      ) : null}
    </>
  );
}

// The header's inline rename, opened by its pencil or the chat actions.
