import { Maximize2, Pause, Target, Trash2 } from "../../ui/leanIcons";
import { useRef, useState, type RefObject } from "react";
import type { AcpGoalState } from "../../domain/acpGoal";
import "../../styles/chat-goal.css";

export type GoalMutationOutcome =
  { kind: "applied" } | { kind: "refused" | "outcomeUnknown"; message: string };

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

export function ChatGoalControl({
  state,
  canMutate = false,
  onMutation,
  detailsRef: externalDetailsRef,
}: {
  state: AcpGoalState;
  canMutate?: boolean;
  onMutation?: (action: "pause" | "clear") => Promise<GoalMutationOutcome>;
  detailsRef?: RefObject<HTMLDetailsElement | null>;
}) {
  const internalDetailsRef = useRef<HTMLDetailsElement>(null);
  const detailsRef = externalDetailsRef ?? internalDetailsRef;
  const [mutation, setMutation] = useState<
    | { kind: "idle" | "pending" }
    | { kind: "refused" | "outcomeUnknown"; message: string }
  >({ kind: "idle" });
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
  const controls = canMutate && onMutation;
  const locked =
    mutation.kind === "pending" || mutation.kind === "outcomeUnknown";
  const run = async (action: "pause" | "clear") => {
    if (!controls || locked) return;
    setMutation({ kind: "pending" });
    try {
      const outcome = await onMutation(action);
      setMutation(outcome.kind === "applied" ? { kind: "idle" } : outcome);
    } catch {
      setMutation({
        kind: "outcomeUnknown",
        message: "Goal outcome unknown. Reload this chat to verify.",
      });
    }
  };
  const actionButton = (
    action: "pause" | "clear",
    label: string,
    Icon: typeof Pause,
  ) => (
    <button
      type="button"
      className="chat-goal-strip__icon"
      aria-label={label}
      title={label}
      disabled={locked}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        void run(action);
      }}
    >
      <Icon size={13} aria-hidden="true" />
    </button>
  );
  return (
    <div className="chat-goal-container">
      <details className="chat-goal-strip" ref={detailsRef}>
        <summary className="chat-goal-strip__row" aria-label="Goal details">
          <Target size={13} aria-hidden="true" />
          <strong>
            {snapshot.status === "active" ? "Pursuing goal" : "Goal"}
          </strong>
          <span
            className="chat-goal-strip__objective"
            title={snapshot.objective}
          >
            {snapshot.objective}
          </span>
          <time>{elapsed(snapshot.timeUsedSeconds)}</time>
          <span className="chat-goal-strip__icons">
            {controls &&
              snapshot.status === "active" &&
              actionButton("pause", "Pause goal", Pause)}
            {controls && actionButton("clear", "Delete goal", Trash2)}
            <button
              type="button"
              className="chat-goal-strip__icon"
              aria-label="Toggle goal details"
              title="Expand goal details"
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (detailsRef.current)
                  detailsRef.current.open = !detailsRef.current.open;
              }}
            >
              <Maximize2 size={13} aria-hidden="true" />
            </button>
          </span>
        </summary>
        <div className="chat-goal-strip__details">
          <p>{snapshot.objective}</p>
          {!canMutate ? (
            <p>
              Native Goal actions are unavailable for this session. Reconnect or
              reload to check availability.
            </p>
          ) : null}
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
        </div>
      </details>
      {mutation.kind === "pending" && (
        <p className="chat-goal-strip__notice" role="status">
          Updating native Goal…
        </p>
      )}
      {(mutation.kind === "refused" || mutation.kind === "outcomeUnknown") && (
        <p className="chat-goal-strip__notice" role="alert">
          {mutation.message}
        </p>
      )}
    </div>
  );
}
