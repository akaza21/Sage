import { Info } from "../ui/leanIcons";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";

export function AgentTaskStatus({
  snapshot,
  harnessStatus,
}: {
  snapshot: ChatLiveSnapshot | null;
  harnessStatus?: string;
}) {
  const label = snapshot?.statusLabel ?? "Idle";
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
