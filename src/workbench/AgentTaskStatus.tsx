import { PickerLogo } from "../ui/PickerOption";
import { harnessIconKind } from "../domain/harness";
import { AlertTriangle, CircleAlert, Info } from "../ui/leanIcons";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";

export function AgentTaskStatus({
  snapshot,
  harnessStatus,
  harnessName,
}: {
  snapshot: ChatLiveSnapshot | null;
  harnessStatus?: string;
  harnessName?: string;
}) {
  const icon = harnessIconKind(harnessName);
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
            aria-description={harnessName}
            title={harnessStatus}
            tabIndex={0}
          >
            <span
              className="agent-task-header__harness-icon"
              aria-hidden="true"
            >
              {icon ? <PickerLogo icon={icon} /> : <Info size={14} />}
            </span>
            <span>{harnessStatus}</span>
          </summary>
          <p>{harnessStatus}</p>
        </details>
      ) : null}
    </>
  );
}
