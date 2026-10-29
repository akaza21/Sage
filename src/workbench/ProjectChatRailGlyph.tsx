import type { AgentRailTrailingState } from "./agentRailStatusView";

/**
 * A rail row's one trailing state: the approval spinner (the Turn still runs
 * behind the "Awaiting approval" pill, Codex s6/f0526), the focused row's
 * status chip, or the quiet glyph.
 */
export function ProjectChatTrailingState({
  awaitingApproval,
  focused,
  state,
  dateTime,
  relative,
}: {
  awaitingApproval: boolean;
  focused: boolean;
  state: AgentRailTrailingState | null;
  dateTime?: string;
  relative: boolean;
}) {
  if (awaitingApproval)
    return (
      <span
        className="project-chat__state project-chat__approval-spinner"
        aria-hidden="true"
      >
        <span className="spinner agent-chat-chip__spinner" />
      </span>
    );
  if (!focused || !state || state.kind === "time" || state.kind === "unread")
    return (
      <ProjectChatRailGlyph
        state={state}
        dateTime={dateTime}
        relative={relative}
      />
    );
  return (
    <span
      className={`project-chat__state project-chat__status-chip project-chat__status-chip--${state.kind}`}
    >
      {state.kind === "working" || state.kind === "recovering" ? (
        <span
          className="project-chat__working project-chat__working--status"
          aria-hidden="true"
        />
      ) : null}
      {state.label}
    </span>
  );
}

export function ProjectChatRailGlyph({
  state,
  dateTime,
  relative = false,
}: {
  state: AgentRailTrailingState | null;
  dateTime?: string;
  relative?: boolean;
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
        {relative && /^\d+(?:[mhd]|d\+)$/.test(state.label) ? " ago" : ""}
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
