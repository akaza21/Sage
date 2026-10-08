import { Target } from "lucide-react";
import type { AcpGoalState } from "../../domain/acpGoal";
import "../../styles/chat-goal.css";

function elapsed(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  const rest = seconds % 60;
  return [
    days ? `${days}d` : null,
    hours ? `${hours}h` : null,
    minutes ? `${minutes}m` : null,
    `${rest}s`,
  ]
    .filter(Boolean)
    .join(" ");
}

function availability(state: Exclude<AcpGoalState, { kind: "goal" }>): string {
  switch (state.kind) {
    case "unsupported":
      return "Goals are not available for this binding.";
    case "offline":
      return "Chat offline. Reconnect to check Goal.";
    case "loading":
      return "Checking native Goal…";
    case "unavailable":
      return "Native Goal unavailable.";
    case "noGoal":
      return "No native Goal is set.";
  }
}

export function ChatGoalControl({ state }: { state: AcpGoalState }) {
  if (state.kind !== "goal") {
    return (
      <details className="chat-goal-entry">
        <summary>
          <Target size={13} aria-hidden="true" /> Goal
        </summary>
        <p>{availability(state)}</p>
      </details>
    );
  }
  const { snapshot } = state;
  return (
    <details className="chat-goal-strip">
      <summary aria-label="Goal details">
        <Target size={13} aria-hidden="true" />
        <strong>
          {snapshot.status === "active" ? "Pursuing goal" : "Goal"}
        </strong>
        <span className="chat-goal-strip__objective" title={snapshot.objective}>
          {snapshot.objective}
        </span>
        <time>{elapsed(snapshot.timeUsedSeconds)}</time>
      </summary>
      <div className="chat-goal-strip__details">
        <p>{snapshot.objective}</p>
        <dl>
          <div>
            <dt>Status</dt>
            <dd>{snapshot.status}</dd>
          </div>
          <div>
            <dt>Usage</dt>
            <dd>
              {snapshot.tokensUsed.toLocaleString()} /{" "}
              {snapshot.tokenBudget?.toLocaleString() ?? "unlimited"} tokens
            </dd>
          </div>
        </dl>
        <p className="chat-goal-strip__read-only">Goal controls unavailable.</p>
        <div className="chat-goal-strip__actions">
          <button
            type="button"
            disabled
            title="Goal controls are not yet available"
          >
            Pause goal
          </button>
          <button
            type="button"
            disabled
            title="Goal controls are not yet available"
          >
            Clear goal
          </button>
        </div>
      </div>
    </details>
  );
}
