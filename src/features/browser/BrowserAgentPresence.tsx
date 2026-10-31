import { Square } from "../../ui/leanIcons";

/**
 * Who controls this tab, shown for as long as a chat's Agent is attached to
 * it. Stop ends the chat's browser authority; Take over pauses it until Hand
 * back, and the Agent's next call asks permission again. While the user's
 * developer-mode approval is live, a red tag says so (ADR-0009b); Stop and
 * Take over end it.
 */
export function BrowserAgentPresence({
  pending,
  userControl,
  developerMode = false,
  chatTitle,
  busy = false,
  onStop,
  onTakeOver,
  onHandBack,
}: {
  pending: boolean;
  userControl: boolean;
  developerMode?: boolean;
  chatTitle: string;
  busy?: boolean;
  onStop: () => void;
  onTakeOver: () => void;
  onHandBack: () => void;
}) {
  const label = userControl
    ? "You're in control"
    : pending
      ? "Agent has a browser action pending"
      : "Agent attached";
  return (
    <div
      className="browser-surface__agent-presence"
      data-control={userControl ? "user" : "agent"}
    >
      {developerMode && !userControl ? (
        <span
          className="browser-surface__agent-developer"
          title="Developer mode is on: the Agent has CDP access to this tab, including running scripts. Stop or Take over ends it."
        >
          Developer mode
        </span>
      ) : null}
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
