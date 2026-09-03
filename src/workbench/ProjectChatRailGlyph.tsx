import type { AgentRailTrailingState } from "./agentRailStatusView";

export function ProjectChatRailGlyph({
  state,
  dateTime,
}: {
  state: AgentRailTrailingState | null;
  dateTime?: string;
}) {
  if (!state) return null;
  const live = state.kind === "working" || state.kind === "recovering";
  if (state.kind === "time") {
    return (
      <time
        className="project-chat__state project-chat__state--time"
        dateTime={dateTime}
      >
        {state.label}
      </time>
    );
  }
  return (
    <span
      className={`project-chat__state project-chat__state--${state.kind}`}
      aria-label={
        state.kind === "unread"
          ? "Unread updates"
          : state.kind === "working"
            ? "Working"
            : state.kind === "waiting"
              ? "Needs attention"
              : state.label
      }
    >
      {live ? (
        <span className="project-chat__working" aria-hidden="true" />
      ) : null}
      <span>{state.label}</span>
    </span>
  );
}
