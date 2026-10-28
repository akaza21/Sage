import { Square } from "../../ui/leanIcons";
import type { BrowserOperationProjection } from "../../domain/browserOperation";

/**
 * Who controls this tab. Stop ends the chat's browser authority; Take over
 * pauses it until Hand back, and the Agent's next call asks permission again.
 */
export function BrowserAgentPresence({
  operation,
  userControl,
  chatTitle,
  busy = false,
  onStop,
  onTakeOver,
  onHandBack,
}: {
  operation: BrowserOperationProjection | null;
  userControl: boolean;
  chatTitle: string;
  busy?: boolean;
  onStop: () => void;
  onTakeOver: () => void;
  onHandBack: () => void;
}) {
  const label = userControl
    ? "You're in control"
    : operation?.phase === "started"
      ? "Agent has a browser action pending"
      : "Agent used this tab";
  return (
    <div
      className="browser-surface__agent-presence"
      data-control={userControl ? "user" : "agent"}
    >
      <span className="browser-surface__agent-presence-label" role="status">
        {`${label} · ${chatTitle}`}
      </span>
      <button
        type="button"
        className="browser-surface__agent-handoff"
        title={
          userControl
            ? "Let this chat's Agent use the tab again. It asks permission first."
            : "Pause this chat's Agent on this tab while you use it."
        }
        disabled={busy}
        onClick={userControl ? onHandBack : onTakeOver}
      >
        {userControl ? "Hand back" : "Take over"}
      </button>
      <button
        type="button"
        className="browser-surface__agent-stop"
        aria-label="Stop agent"
        title="Stop the Agent's browser action and detach this tab from the chat."
        disabled={busy}
        onClick={onStop}
      >
        <Square size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
