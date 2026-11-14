import { Square } from "../../ui/leanIcons";

export function BrowserAgentPresence({
  pending,
  userControl,
  developerMode,
  chatTitle,
  busy,
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
  const description = `${label} · ${chatTitle}`;
  return (
    <div
      className="browser-surface__agent-presence"
      data-control={userControl ? "user" : "agent"}
    >
      {developerMode && !userControl ? (
        <span
          className="browser-surface__agent-developer"
          title="CDP access includes scripts. Stop or Take over ends it."
        >
          Developer mode
        </span>
      ) : null}
      {/* The label truncates; its title keeps the whole text reachable. */}
      <span
        className="browser-surface__agent-presence-label"
        role="status"
        title={description}
      >
        {description}
      </span>
      <button
        type="button"
        className="browser-surface__agent-handoff"
        data-overflow-control="handoff"
        title={
          userControl
            ? "Resume Agent; asks permission first."
            : "Pause Agent while you use this tab."
        }
        disabled={busy}
        onClick={userControl ? onHandBack : onTakeOver}
      >
        {userControl ? "Hand back" : "Take over"}
      </button>
      <button
        type="button"
        className="browser-surface__agent-stop"
        data-overflow-control="stop"
        aria-label="Stop agent"
        title="Stop and detach this tab from chat."
        disabled={busy}
        onClick={onStop}
      >
        <Square size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
