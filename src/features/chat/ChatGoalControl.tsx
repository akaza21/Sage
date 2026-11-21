import { Maximize2, Trash2 } from "../../ui/leanIcons";
import { Pause, Target } from "../../ui/leanIconNodesLazy";
import { useRef, useState, type RefObject } from "react";
import type {
  AcpGoalCapabilities,
  AcpGoalState,
  GoalMutationAction,
} from "../../domain/acpGoal";
import { formatElapsedSeconds } from "../../domain/duration";
import "../../styles/chat-goal.css";
import type * as GoalContentChunk from "./GoalDetails";
import { lazyParts } from "./lazyParts";
const GoalContent = lazyParts<typeof GoalContentChunk>(
  () => import("./GoalDetails"),
);

export type GoalMutationOutcome =
  { kind: "applied" } | { kind: "refused" | "outcomeUnknown"; message: string };

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
  capabilities,
  editorOpen = false,
  onEditorClose,
  canMutate = false,
  onMutation,
  detailsRef: externalDetailsRef,
}: {
  state: AcpGoalState;
  capabilities?: AcpGoalCapabilities;
  editorOpen?: boolean;
  onEditorClose?: () => void;
  canMutate?: boolean;
  onMutation?: (
    action: GoalMutationAction,
    objective?: string,
  ) => Promise<GoalMutationOutcome>;
  detailsRef?: RefObject<HTMLDetailsElement | null>;
}) {
  const internalDetailsRef = useRef<HTMLDetailsElement>(null);
  const detailsRef = externalDetailsRef ?? internalDetailsRef;
  const [mutation, setMutation] = useState<
    | { kind: "idle" | "pending" }
    | { kind: "refused" | "outcomeUnknown"; message: string }
  >({ kind: "idle" });
  const [editing, setEditing] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const controls = canMutate && onMutation;
  const locked =
    mutation.kind === "pending" || mutation.kind === "outcomeUnknown";
  const run = async (
    action: GoalMutationAction,
    objective?: string,
  ): Promise<boolean> => {
    if (!controls || locked || !capabilities?.[action]) return false;
    setMutation({ kind: "pending" });
    try {
      const outcome = await onMutation(
        action,
        ...(action === "set" ? [objective] : []),
      );
      setMutation(outcome.kind === "applied" ? { kind: "idle" } : outcome);
      return outcome.kind === "applied";
    } catch {
      setMutation({
        kind: "outcomeUnknown",
        message: "Goal outcome unknown. Reload this chat to verify.",
      });
      return false;
    }
  };
  const closeEditor = () => {
    setEditing(false);
    onEditorClose?.();
  };
  const editor =
    controls && capabilities?.set && (editing || editorOpen) ? (
      <GoalContent
        part="GoalEditor"
        objective={state.kind === "goal" ? state.snapshot.objective : ""}
        disabled={locked}
        onSave={(objective) => run("set", objective)}
        onClose={closeEditor}
      />
    ) : null;
  const notice =
    mutation.kind === "pending" ? (
      <p className="chat-goal-strip__notice" role="status">
        Updating native Goal...
      </p>
    ) : mutation.kind === "refused" || mutation.kind === "outcomeUnknown" ? (
      <p className="chat-goal-strip__notice" role="alert">
        {mutation.message}
      </p>
    ) : null;
  if (state.kind !== "goal") {
    if (state.kind === "noGoal" && capabilities?.set)
      return (
        <>
          {editor}
          {notice}
        </>
      );
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
      <details
        className="chat-goal-strip"
        ref={detailsRef}
        onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
      >
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
          {snapshot.progress ? (
            <span>
              {snapshot.progress.iterations.toLocaleString()} iterations
            </span>
          ) : (
            <time>{formatElapsedSeconds(snapshot.timeUsedSeconds)}</time>
          )}
          <span className="chat-goal-strip__icons">
            {controls && capabilities?.set && (
              <button
                type="button"
                className="chat-goal-strip__icon"
                aria-label="Edit goal"
                disabled={locked}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  setEditing(true);
                }}
              >
                Edit
              </button>
            )}
            {controls &&
              capabilities?.pause &&
              snapshot.status === "active" &&
              actionButton("pause", "Pause goal", Pause)}
            {controls &&
              capabilities?.clear &&
              actionButton("clear", "Delete goal", Trash2)}
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
        {detailsOpen && (
          <GoalContent
            part="GoalDetails"
            snapshot={snapshot}
            canMutate={canMutate}
          />
        )}
      </details>
      {editor}
      {notice}
    </div>
  );
}
